//! Web Workers 并行计算引擎
//!
//! 浏览器端跨核并行的编排层：wasm32 本身无线程（见
//! `packages/core/src/parallel.rs` 的平台策略表），真正的并行由
//! **Web Worker 线程池**承担，本模块负责任务切分、派发与结果归并。
//!
//! 两条执行路径（由 [`WorkerPool::strategy`] 决定）：
//! * `Worker`：任务 `postMessage` 给 `Worker` 线程，跨核真并行；
//! * `Inline`：主线程内经 `alpha_core::parallel` 计算——wasm32 上退化为
//!   顺序，但 API 与结果口径完全一致（可作 Worker 不可用时的降级）。
//!
//! 派发协议（[`WorkerTask`] / [`WorkerResult`]）用 serde 表达，
//! 与 `Worker` 的 `postMessage` 结构化克隆兼容；`handle_task` 是
//! **Worker 侧入口**——在 Worker 线程里调用同一个 wasm 实例处理任务，
//! 主线程与 Worker 共用同一份计算实现，杜绝两侧算法漂移。

use alpha_core::indicators::TechnicalIndicators;
use alpha_core::parallel::{self, IndicatorKind};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

/// Worker 任务类型（`postMessage` 载荷；`handle_task` 为 Worker 侧入口）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WorkerTask {
    /// 计算技术指标
    ComputeIndicators {
        prices: Vec<f64>,
        indicators: Vec<String>,
        periods: Vec<usize>,
    },
    /// 批量计算
    BatchCompute {
        datasets: Vec<Vec<f64>>,
        indicator_type: String,
        period: usize,
    },
    /// 复杂策略回测
    BacktestStrategy {
        prices: Vec<f64>,
        strategy_params: serde_json::Value,
    },
}

/// Worker 计算结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerResult {
    pub task_id: String,
    pub success: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
    pub duration_ms: f64,
}

/// 任务分片：把 N 个数据集切给 `workers` 个执行单元
///
/// 均衡策略：连续 chunk 而非 round-robin——单次计算代价与数据长度相关时，
/// 连续块能让长序列集中到同一 worker，chunk 数差异小则并行度自然退化到
/// 实际可用核数（对超长单序列也不会过度切分）。
pub fn plan_chunks(total: usize, workers: usize) -> Vec<(usize, usize)> {
    if total == 0 || workers == 0 {
        return Vec::new();
    }
    let workers = workers.min(total);
    let chunk = total.div_ceil(workers);
    (0..total)
        .step_by(chunk)
        .map(|start| (start, (start + chunk).min(total)))
        .collect()
}

/// 计算耗时（毫秒）——`std::time` 在 wasm32 由宿主 `performance.now` 支撑，
/// 不可用时退化为 0.0（调用方只把它当观测指标，不参与计算）
pub fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.performance())
            .map(|p| p.now())
            .unwrap_or(0.0)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0)
    }
}

/// 执行任务：Worker 侧入口（主线程 `Inline` 路径复用同一实现）
/// 亦导出供 Web Worker 线程直接调用（同一 wasm 模块在 worker 里加载）
/// ABI 口径：入参 `task` 为 JsValue（含 {type, ...}），返回 JsValue 序列化的 WorkerResult
#[wasm_bindgen(js_name = handleTask)]
pub fn handle_task(task_id: &str, task: JsValue) -> Result<JsValue, JsValue> {
    let task: WorkerTask = serde_wasm_bindgen::from_value(task)
        .map_err(|e| JsValue::from_str(&format!("任务反序列化失败: {e}")))?;
    let result = handle_task_inner(task_id, &task);
    serde_wasm_bindgen::to_value(&result)
        .map_err(|e| JsValue::from_str(&format!("结果序列化失败: {e}")))
}

/// 内部实现（Rust 侧直调、测试复用，避免 JsValue 往返开销）
pub fn handle_task_inner(task_id: &str, task: &WorkerTask) -> WorkerResult {
    let start = now_ms();
    let outcome = match task {
        WorkerTask::ComputeIndicators {
            prices,
            indicators,
            periods,
        } => {
            let ind = TechnicalIndicators::new();
            let mut ok = serde_json::Map::new();
            let mut errs = Vec::new();
            for (name, &period) in indicators.iter().zip(periods.iter()) {
                match IndicatorKind::from_str_lossy(name) {
                    Some(kind) => {
                        let values = match kind {
                            IndicatorKind::Sma => ind.calculate_sma(prices, period),
                            IndicatorKind::Ema => ind.calculate_ema(prices, period),
                            IndicatorKind::Rsi => ind.calculate_rsi(prices, period),
                            // 三元组取上轨，与 `ParallelScheduler` 既有口径一致
                            IndicatorKind::Bollinger => {
                                ind.calculate_bollinger_bands(prices, period, 2.0).0
                            }
                        };
                        ok.insert(name.clone(), serde_json::json!(values));
                    }
                    None => errs.push(format!("不支持的指标类型: {name}")),
                }
            }
            if errs.is_empty() {
                Ok(serde_json::Value::Object(ok))
            } else {
                Err(errs.join("; "))
            }
        }
        WorkerTask::BatchCompute {
            datasets,
            indicator_type,
            period,
        } => match IndicatorKind::from_str_lossy(indicator_type) {
            Some(kind) => {
                let results = parallel::compute(datasets, kind, *period);
                Ok(serde_json::json!({ "results": results }))
            }
            None => Err(format!("不支持的指标类型: {indicator_type}")),
        },
        WorkerTask::BacktestStrategy {
            prices,
            strategy_params,
        } => {
            // 策略参数最小口径：{ "fast": n, "slow": m, "fee_bps": n }
            let fast = strategy_params
                .get("fast")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(5) as usize;
            let slow = strategy_params
                .get("slow")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(20) as usize;
            let fee_bps = strategy_params
                .get("fee_bps")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0);
            // 边界参数来自 JS，必须先校验再进引擎（引擎内部用 assert!，
            // 库代码禁 panic，见 docs/rust-code-standards.md §3）
            if fast == 0 || slow <= 1 || fast >= slow {
                Err(format!(
                    "非法策略参数: 要求 1 <= fast < slow，收到 fast={fast} slow={slow}"
                ))
            } else if prices.is_empty() {
                Err("价格序列为空".to_string())
            } else {
                let mut strategy = alpha_core::backtest::SmaCrossStrategy::new(fast, slow);
                let engine = alpha_core::backtest::BacktestEngine::new(fee_bps);
                let report = engine.run(prices, &mut strategy);
                serde_json::to_value(&report).map_err(|e| e.to_string())
            }
        }
    };

    let duration_ms = now_ms() - start;
    match outcome {
        Ok(data) => WorkerResult {
            task_id: task_id.to_string(),
            success: true,
            data: Some(data),
            error: None,
            duration_ms,
        },
        Err(error) => WorkerResult {
            task_id: task_id.to_string(),
            success: false,
            data: None,
            error: Some(error),
            duration_ms,
        },
    }
}

/// 执行策略（JS 侧以字符串传入，避免枚举 ABI 绑定）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolStrategy {
    /// 派发给 Web Worker 线程（浏览器真并行）
    Worker,
    /// 主线程内计算（降级路径，结果口径一致）
    Inline,
}

impl PoolStrategy {
    /// 从字符串解析（"worker" | "inline"，其余一律降级为 Inline）
    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "worker" => Self::Worker,
            _ => Self::Inline,
        }
    }

    /// 规范化名称
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::Inline => "inline",
        }
    }
}

/// Worker 池管理器
#[wasm_bindgen]
pub struct WorkerPool {
    worker_count: usize,
    task_counter: std::cell::RefCell<usize>,
    strategy: PoolStrategy,
    /// 已派发任务数（观测用）
    dispatched: std::cell::Cell<usize>,
}

#[wasm_bindgen]
impl WorkerPool {
    /// 创建 Worker 池（默认 Inline 策略：Worker 脚本未部署时功能仍可用）
    #[wasm_bindgen(constructor)]
    pub fn new(worker_count: usize) -> WorkerPool {
        Self::with_strategy(worker_count, "inline")
    }

    /// 按显式策略创建（`strategy`: "worker" | "inline"，未知值降级为 inline）
    #[wasm_bindgen(js_name = withStrategy)]
    pub fn with_strategy(worker_count: usize, strategy: &str) -> WorkerPool {
        let count = worker_count
            .max(1)
            .min(navigator_hardware_concurrency().unwrap_or(4));

        WorkerPool {
            worker_count: count,
            task_counter: std::cell::RefCell::new(0),
            strategy: PoolStrategy::from_str_lossy(strategy),
            dispatched: std::cell::Cell::new(0),
        }
    }

    /// 获取 Worker 数量
    #[wasm_bindgen(js_name = getWorkerCount)]
    pub fn get_worker_count(&self) -> usize {
        self.worker_count
    }

    /// 当前策略（"worker" | "inline"）
    #[wasm_bindgen(js_name = getStrategy)]
    pub fn get_strategy(&self) -> String {
        self.strategy.as_str().to_string()
    }

    /// 已派发任务总数
    #[wasm_bindgen(js_name = getDispatchedCount)]
    pub fn get_dispatched_count(&self) -> usize {
        self.dispatched.get()
    }

    /// 派发任务：返回 task_id（`Worker` 策略下由 JS 侧 postMessage 消费）
    #[wasm_bindgen(js_name = dispatchTask)]
    pub fn dispatch_task(&self, task_js: &JsValue) -> Result<String, JsValue> {
        let task: WorkerTask = serde_wasm_bindgen::from_value(task_js.clone())
            .map_err(|e| JsValue::from_str(&format!("任务反序列化失败: {e}")))?;
        let task_id = self.generate_task_id();
        self.dispatched.set(self.dispatched.get() + 1);
        // 载荷形状固定为 { task_id, task }，与 Worker 侧 handle_task 对齐
        serde_wasm_bindgen::to_value(&serde_json::json!({ "task_id": task_id, "task": task }))
            .map_err(|e| JsValue::from_str(&format!("任务序列化失败: {e}")))?;
        Ok(task_id)
    }

    /// 内联执行批量任务（Worker 侧入口的同步版：主线程直接算，不跨线程）
    #[wasm_bindgen(js_name = runTaskInline)]
    pub fn run_task_inline(&self, task_js: &JsValue) -> Result<JsValue, JsValue> {
        let task: WorkerTask = serde_wasm_bindgen::from_value(task_js.clone())
            .map_err(|e| JsValue::from_str(&format!("任务反序列化失败: {e}")))?;
        let task_id = self.generate_task_id();
        self.dispatched.set(self.dispatched.get() + 1);
        let result = handle_task_inner(&task_id, &task);
        serde_wasm_bindgen::to_value(&result)
            .map_err(|e| JsValue::from_str(&format!("结果序列化失败: {e}")))
    }

    /// 并行计算多个股票的指标
    ///
    /// 数据集按 [`plan_chunks`] 切给 `worker_count` 个执行单元；`Inline` 策略下
    /// 各 chunk 经 `alpha_core::parallel` 顺序执行（wasm32 无线程），
    /// `Worker` 策略下由 JS 侧把各 chunk 分别 postMessage。
    #[wasm_bindgen(js_name = computeIndicatorsParallel)]
    pub async fn compute_indicators_parallel(
        &self,
        prices_array: js_sys::Array,
        indicator_type: &str,
        period: usize,
    ) -> Result<JsValue, JsValue> {
        let kind = IndicatorKind::from_str_lossy(indicator_type)
            .ok_or_else(|| JsValue::from_str(&format!("不支持的指标类型: {indicator_type}")))?;

        let mut datasets = Vec::with_capacity(prices_array.length() as usize);
        for i in 0..prices_array.length() {
            let prices_js = prices_array.get(i);
            let prices_f64 = js_sys::Float64Array::from(prices_js);
            datasets.push(prices_f64.to_vec());
        }

        // 分片：worker_count 个 chunk，每块独立计算
        let chunks = plan_chunks(datasets.len(), self.worker_count);
        let mut results: Vec<Vec<f64>> = Vec::with_capacity(datasets.len());
        for (from, to) in chunks {
            results.extend(parallel::compute(&datasets[from..to], kind, period));
        }

        // 转换结果为 JavaScript 数组
        let js_results = js_sys::Array::new();
        for result in results {
            js_results.push(&js_sys::Float64Array::from(&result[..]));
        }

        Ok(js_results.into())
    }

    /// 批量指标计算（带统计报告：并行度/耗时/数据集数）
    #[wasm_bindgen(js_name = computeBatchWithReport)]
    pub fn compute_batch_with_report(
        &self,
        prices_array: js_sys::Array,
        indicator_type: &str,
        period: usize,
    ) -> Result<JsValue, JsValue> {
        let kind = IndicatorKind::from_str_lossy(indicator_type)
            .ok_or_else(|| JsValue::from_str(&format!("不支持的指标类型: {indicator_type}")))?;

        let mut datasets = Vec::with_capacity(prices_array.length() as usize);
        for i in 0..prices_array.length() {
            let prices_f64 = js_sys::Float64Array::from(prices_array.get(i));
            datasets.push(prices_f64.to_vec());
        }

        let start = now_ms();
        let report = parallel::compute_with_report(&datasets, kind, period, 0.0);
        let report = parallel::BatchReport {
            elapsed_ms: now_ms() - start,
            ..report
        };
        serde_wasm_bindgen::to_value(&report)
            .map_err(|e| JsValue::from_str(&format!("报告序列化失败: {e}")))
    }

    /// 生成任务 ID（单调递增，供调用方关联结果）
    fn generate_task_id(&self) -> String {
        let mut counter = self.task_counter.borrow_mut();
        *counter += 1;
        format!("task_{}", *counter)
    }
}

/// Worker 线程初始化钩子（wasm 在 WorkerGlobalScope 加载时自动运行）：
/// Worker 无 `window`，用 `self`/`console`；复用同一 panic hook。
#[wasm_bindgen(start)]
pub fn init_worker_panic_hook() {
    console_error_panic_hook::set_once();
}

/// 获取浏览器支持的硬件并发数
fn navigator_hardware_concurrency() -> Option<usize> {
    let window = web_sys::window()?;
    let concurrency = window.navigator().hardware_concurrency() as usize;
    Some(concurrency)
}

/// 并行任务调度器
#[wasm_bindgen]
pub struct ParallelScheduler {
    max_concurrent: usize,
    active_tasks: std::cell::RefCell<usize>,
}

#[wasm_bindgen]
impl ParallelScheduler {
    /// 创建调度器
    #[wasm_bindgen(constructor)]
    pub fn new(max_concurrent: usize) -> ParallelScheduler {
        ParallelScheduler {
            max_concurrent,
            active_tasks: std::cell::RefCell::new(0),
        }
    }

    /// 提交计算任务
    #[wasm_bindgen(js_name = submitTask)]
    pub async fn submit_task(
        &self,
        prices: js_sys::Float64Array,
        indicator_type: &str,
        period: usize,
    ) -> Result<js_sys::Float64Array, JsValue> {
        // 检查并发限制
        {
            let active = self.active_tasks.borrow();
            if *active >= self.max_concurrent {
                return Err(JsValue::from_str("达到最大并发任务数"));
            }
        }

        // 增加活动任务计数
        {
            let mut active = self.active_tasks.borrow_mut();
            *active += 1;
        }

        // 执行计算
        let prices_vec = prices.to_vec();
        let indicators = TechnicalIndicators::new();

        let result = match indicator_type {
            "sma" => indicators.calculate_sma(&prices_vec, period),
            "ema" => indicators.calculate_ema(&prices_vec, period),
            "rsi" => indicators.calculate_rsi(&prices_vec, period),
            "bollinger" => {
                let (upper, _, _) = indicators.calculate_bollinger_bands(&prices_vec, period, 2.0);
                upper
            }
            _ => {
                // 减少活动任务计数
                let mut active = self.active_tasks.borrow_mut();
                *active -= 1;
                return Err(JsValue::from_str("不支持的指标类型"));
            }
        };

        // 减少活动任务计数
        {
            let mut active = self.active_tasks.borrow_mut();
            *active -= 1;
        }

        Ok(js_sys::Float64Array::from(&result[..]))
    }

    /// 获取活动任务数
    #[wasm_bindgen(js_name = getActiveTaskCount)]
    pub fn get_active_task_count(&self) -> usize {
        *self.active_tasks.borrow()
    }

    /// 获取最大并发数
    #[wasm_bindgen(js_name = getMaxConcurrent)]
    pub fn get_max_concurrent(&self) -> usize {
        self.max_concurrent
    }
}

/// 批量并行计算工具
#[wasm_bindgen]
pub struct BatchComputer {
    chunk_size: usize,
}

#[wasm_bindgen]
impl BatchComputer {
    /// 创建批量计算器
    #[wasm_bindgen(constructor)]
    pub fn new(chunk_size: usize) -> BatchComputer {
        BatchComputer {
            chunk_size: chunk_size.max(10),
        }
    }

    /// 批量计算 SMA
    #[wasm_bindgen(js_name = batchComputeSMA)]
    pub fn batch_compute_sma(
        &self,
        prices: js_sys::Float64Array,
        periods: js_sys::Uint32Array,
    ) -> Result<js_sys::Array, JsValue> {
        let prices_vec = prices.to_vec();
        let periods_vec: Vec<u32> = periods.to_vec();
        let indicators = TechnicalIndicators::new();

        let results = js_sys::Array::new();

        for &period in &periods_vec {
            let sma = indicators.calculate_sma(&prices_vec, period as usize);
            let result_obj = js_sys::Object::new();

            js_sys::Reflect::set(
                &result_obj,
                &JsValue::from_str("period"),
                &JsValue::from_f64(period as f64),
            )?;

            js_sys::Reflect::set(
                &result_obj,
                &JsValue::from_str("values"),
                &js_sys::Float64Array::from(&sma[..]),
            )?;

            results.push(&result_obj);
        }

        Ok(results)
    }

    /// 批量计算多个指标
    #[wasm_bindgen(js_name = batchComputeMultiple)]
    pub fn batch_compute_multiple(
        &self,
        prices: js_sys::Float64Array,
        sma_period: usize,
        ema_period: usize,
        rsi_period: usize,
    ) -> Result<JsValue, JsValue> {
        let prices_vec = prices.to_vec();
        let indicators = TechnicalIndicators::new();

        // 并发计算多个指标
        let sma = indicators.calculate_sma(&prices_vec, sma_period);
        let ema = indicators.calculate_ema(&prices_vec, ema_period);
        let rsi = indicators.calculate_rsi(&prices_vec, rsi_period);
        let (upper, middle, lower) = indicators.calculate_bollinger_bands(&prices_vec, 20, 2.0);

        let result = serde_json::json!({
            "sma": sma,
            "ema": ema,
            "rsi": rsi,
            "bollinger": {
                "upper": upper,
                "middle": middle,
                "lower": lower
            }
        });

        serde_wasm_bindgen::to_value(&result)
            .map_err(|e| JsValue::from_str(&format!("序列化错误: {}", e)))
    }

    /// 获取块大小
    #[wasm_bindgen(js_name = getChunkSize)]
    pub fn get_chunk_size(&self) -> usize {
        self.chunk_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(not(target_arch = "wasm32"), ignore = "requires wasm32 (web-sys)")]
    fn test_worker_pool_creation() {
        let pool = WorkerPool::new(4);
        assert!(pool.get_worker_count() <= 4);
        assert!(pool.get_worker_count() >= 1);
    }

    #[test]
    fn test_parallel_scheduler() {
        let scheduler = ParallelScheduler::new(5);
        assert_eq!(scheduler.get_max_concurrent(), 5);
        assert_eq!(scheduler.get_active_task_count(), 0);
    }

    #[test]
    fn test_batch_computer() {
        let computer = BatchComputer::new(100);
        assert_eq!(computer.get_chunk_size(), 100);

        let computer_small = BatchComputer::new(5);
        assert_eq!(computer_small.get_chunk_size(), 10); // 最小值限制
    }

    fn prices(n: usize) -> Vec<f64> {
        (0..n).map(|i| 100.0 + (i as f64) * 0.7).collect()
    }

    /// 分片必须完整覆盖 [0, total) 且不重叠（漏一片 = 静默丢数据）
    #[test]
    fn chunks_cover_all_datasets_without_overlap() {
        for total in [0usize, 1, 3, 8, 16, 17, 100] {
            for workers in [1usize, 2, 3, 4, 8, 64] {
                let chunks = plan_chunks(total, workers);
                let mut covered = vec![false; total];
                for (from, to) in &chunks {
                    assert!(from < to, "空分片不应产生：{from}..{to}");
                    for slot in &mut covered[*from..*to] {
                        assert!(!*slot, "数据集被重复分片");
                        *slot = true;
                    }
                }
                assert!(
                    covered.iter().all(|c| *c),
                    "total={total} workers={workers} 分片未完整覆盖"
                );
                assert!(chunks.len() <= workers.max(1).min(total.max(1)));
            }
        }
    }

    /// workers=0 不 panic（除零防护）
    #[test]
    fn chunks_with_zero_workers_is_empty() {
        assert!(plan_chunks(10, 0).is_empty());
    }

    /// 批量任务：结果条数与顺序与输入一致
    #[test]
    fn batch_task_results_match_input_order() {
        let datasets: Vec<Vec<f64>> = (0..7).map(|i| prices(50 + i * 10)).collect();
        let task = WorkerTask::BatchCompute {
            datasets,
            indicator_type: "sma".to_string(),
            period: 5,
        };
        let result = handle_task_inner("task_1", &task);
        assert!(result.success, "批量任务应成功：{:?}", result.error);
        assert_eq!(result.task_id, "task_1");
        let data = result.data.expect("应有数据");
        let results = data["results"].as_array().expect("results 应为数组");
        assert_eq!(results.len(), 7, "结果条数应等于输入数据集数");
        for (i, r) in results.iter().enumerate() {
            let arr = r.as_array().expect("每项为数组");
            assert_eq!(arr.len(), 50 + i * 10, "数据集 {i} 结果长度应守恒");
        }
    }

    /// 未知指标类型走错误路径，不 panic
    #[test]
    fn unknown_indicator_yields_error_result() {
        let task = WorkerTask::BatchCompute {
            datasets: vec![prices(20)],
            indicator_type: "unknown_indicator".to_string(),
            period: 5,
        };
        let result = handle_task_inner("task_err", &task);
        assert!(!result.success);
        assert!(result.data.is_none());
        assert!(
            result
                .error
                .unwrap_or_default()
                .contains("不支持的指标类型"),
            "错误信息应指明指标类型不支持"
        );
        assert_eq!(result.task_id, "task_err", "失败结果也须回传 task_id");
    }

    /// 回测任务：非法策略参数被拦截（引擎内部是 assert!，不能在边界炸）
    #[test]
    fn backtest_task_rejects_invalid_params() {
        let task = WorkerTask::BacktestStrategy {
            prices: prices(60),
            strategy_params: serde_json::json!({"fast": 20, "slow": 5}),
        };
        let result = handle_task_inner("task_bad", &task);
        assert!(!result.success, "fast >= slow 应被拒绝");
        assert!(result.error.unwrap_or_default().contains("非法策略参数"));
    }

    /// 回测任务：空价格序列被拦截
    #[test]
    fn backtest_task_rejects_empty_prices() {
        let task = WorkerTask::BacktestStrategy {
            prices: vec![],
            strategy_params: serde_json::json!({"fast": 5, "slow": 20}),
        };
        let result = handle_task_inner("task_empty", &task);
        assert!(!result.success);
        assert!(result.error.unwrap_or_default().contains("空"));
    }

    /// 回测任务：合法参数产出报告
    #[test]
    fn backtest_task_produces_report() {
        let task = WorkerTask::BacktestStrategy {
            prices: (1..=100).map(|i| 10.0 + i as f64).collect(),
            strategy_params: serde_json::json!({"fast": 5, "slow": 20, "fee_bps": 5.0}),
        };
        let result = handle_task_inner("task_bt", &task);
        assert!(result.success, "回测应成功：{:?}", result.error);
        let data = result.data.expect("应有报告");
        assert!(data["total_return_pct"].is_number(), "报告含累计收益");
        assert!(data["equity_curve"].is_array(), "报告含净值曲线");
    }

    /// 指标任务：多指标混合，坏类型不阻断好类型（错误聚合）
    #[test]
    fn compute_indicators_aggregates_errors() {
        let task = WorkerTask::ComputeIndicators {
            prices: prices(80),
            indicators: vec!["sma".to_string(), "bad_one".to_string()],
            periods: vec![10, 10],
        };
        let result = handle_task_inner("task_mix", &task);
        assert!(!result.success, "含坏类型应整体失败");
        assert!(result.error.unwrap_or_default().contains("bad_one"));
    }

    /// 指标任务：全合法时逐项产出
    #[test]
    fn compute_indicators_emits_each_series() {
        let task = WorkerTask::ComputeIndicators {
            prices: prices(60),
            indicators: vec!["sma".to_string(), "rsi".to_string()],
            periods: vec![5, 14],
        };
        let result = handle_task_inner("task_ok", &task);
        assert!(result.success, "{:?}", result.error);
        let data = result.data.expect("应有数据");
        assert_eq!(data["sma"].as_array().map(|a| a.len()), Some(60));
        assert_eq!(data["rsi"].as_array().map(|a| a.len()), Some(60));
    }

    /// 任务可序列化往返（postMessage 载荷协议锁定）
    #[test]
    fn task_serde_roundtrip_preserves_payload() {
        let task = WorkerTask::BatchCompute {
            datasets: vec![vec![1.0, 2.0, 3.0]],
            indicator_type: "ema".to_string(),
            period: 2,
        };
        let json = serde_json::to_string(&task).expect("任务应可序列化");
        let back: WorkerTask = serde_json::from_str(&json).expect("任务应可反序列化");
        match back {
            WorkerTask::BatchCompute {
                datasets,
                indicator_type,
                period,
            } => {
                assert_eq!(datasets, vec![vec![1.0, 2.0, 3.0]]);
                assert_eq!(indicator_type, "ema");
                assert_eq!(period, 2);
            }
            other => panic!("反序列化得到错误变体: {other:?}"),
        }
        // 标签字段存在（JS 侧按 type 分派）
        assert!(json.contains("\"type\""), "任务载荷须带 type 标签");
    }

    /// Worker 路径集成：handle_task 直接计算多指标，结果逐项可验
    #[test]
    fn worker_handle_task_compute_indicators_integration() {
        let task = WorkerTask::ComputeIndicators {
            prices: prices(80),
            indicators: vec!["sma".to_string(), "ema".to_string(), "rsi".to_string()],
            periods: vec![10, 10, 14],
        };
        let result = handle_task_inner("integration_1", &task);
        assert!(result.success, "Worker 计算应成功: {:?}", result.error);
        assert_eq!(result.task_id, "integration_1");
        let data = result.data.expect("应有数据");
        assert_eq!(data["sma"].as_array().map(|a| a.len()), Some(80));
        assert_eq!(data["ema"].as_array().map(|a| a.len()), Some(80));
        assert_eq!(data["rsi"].as_array().map(|a| a.len()), Some(80));
        assert!(result.duration_ms >= 0.0, "耗时应非负");
    }

    /// Worker 路径集成：BatchCompute 多标的并行（native 走 Rayon，wasm32 顺序），
    /// 结果条数/顺序/长度与输入守恒
    #[test]
    fn worker_handle_task_batch_compute_integration() {
        let datasets: Vec<Vec<f64>> = (0..12).map(|i| prices(100 + i * 20)).collect();
        let task = WorkerTask::BatchCompute {
            datasets,
            indicator_type: "sma".to_string(),
            period: 20,
        };
        let result = handle_task_inner("integration_batch", &task);
        assert!(result.success, "批量计算应成功: {:?}", result.error);
        let data = result.data.expect("应有数据");
        let results = data["results"].as_array().expect("results 为数组");
        assert_eq!(results.len(), 12, "12 个数据集应产出 12 个结果");
        for (i, r) in results.iter().enumerate() {
            let arr = r.as_array().expect("每项为数组");
            assert_eq!(arr.len(), 100 + i * 20, "数据集 {i} 长度守恒");
        }
    }

    /// Worker 路径集成：BacktestStrategy 完整回测报告
    #[test]
    fn worker_handle_task_backtest_integration() {
        let task = WorkerTask::BacktestStrategy {
            prices: (1..=200).map(|i| 10.0 + i as f64 * 0.5).collect(),
            strategy_params: serde_json::json!({"fast": 5, "slow": 20, "fee_bps": 5.0}),
        };
        let result = handle_task_inner("integration_bt", &task);
        assert!(result.success, "回测应成功: {:?}", result.error);
        let data = result.data.expect("应有报告");
        assert!(data["total_return_pct"].is_number(), "报告含累计收益");
        assert!(data["equity_curve"].is_array(), "报告含净值曲线");
        assert!(data["trade_count"].as_u64().is_some(), "报告含交易计数");
        assert!(data["max_drawdown_pct"].is_number(), "报告含最大回撤");
        assert!(data["sharpe_ratio"].is_number(), "报告含夏普");
        assert!(data["win_rate_pct"].is_number(), "报告含胜率");
    }

    /// Inline 路径（WorkerPool::run_task_inline）与 handle_task 结果完全一致
    #[test]
    #[cfg_attr(
        not(target_arch = "wasm32"),
        ignore = "requires wasm32 (serde-wasm-bindgen)"
    )]
    fn inline_path_matches_worker_handle_task() {
        let pool = WorkerPool::new(4); // 默认 inline 策略
        let task = WorkerTask::BatchCompute {
            datasets: vec![prices(50), prices(80), prices(120)],
            indicator_type: "ema".to_string(),
            period: 12,
        };
        let task_js = serde_wasm_bindgen::to_value(&task).unwrap();
        let inline_result = pool.run_task_inline(&task_js).expect("inline 执行应成功");
        let inline: WorkerResult = serde_wasm_bindgen::from_value(inline_result).unwrap();

        let worker_result = handle_task_inner("cmp_worker", &task);

        assert_eq!(inline.success, worker_result.success);
        assert!(inline.task_id.starts_with("task_"));
        assert!(inline.duration_ms >= 0.0);
        let inline_data = inline.data.unwrap();
        let worker_data = worker_result.data.unwrap();
        assert_eq!(inline_data["results"].as_array().map(|a| a.len()), Some(3));
        assert_eq!(worker_data["results"].as_array().map(|a| a.len()), Some(3));
        // 逐元素比对（浮点精度 1e-12）
        for (i, (iv, wv)) in inline_data["results"]
            .as_array()
            .unwrap()
            .iter()
            .zip(worker_data["results"].as_array().unwrap().iter())
            .enumerate()
        {
            let ia = iv.as_array().unwrap();
            let wa = wv.as_array().unwrap();
            assert_eq!(ia.len(), wa.len(), "数据集 {i} 长度一致");
            for (j, (a, b)) in ia.iter().zip(wa.iter()).enumerate() {
                assert!(
                    (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 1e-12,
                    "inline vs worker 数据集 {i} 位置 {j} 不一致"
                );
            }
        }
    }

    /// 零数据集 / 单数据集边界
    #[test]
    fn worker_batch_compute_edge_cases() {
        // 空输入
        let empty_task = WorkerTask::BatchCompute {
            datasets: vec![],
            indicator_type: "sma".to_string(),
            period: 10,
        };
        let empty_result = handle_task_inner("edge_empty", &empty_task);
        assert!(empty_result.success);
        let empty_data = empty_result.data.unwrap();
        assert!(empty_data["results"].as_array().unwrap().is_empty());

        // 单数据集
        let single_task = WorkerTask::BatchCompute {
            datasets: vec![prices(30)],
            indicator_type: "rsi".to_string(),
            period: 14,
        };
        let single_result = handle_task_inner("edge_single", &single_task);
        assert!(single_result.success);
        let single_data = single_result.data.unwrap();
        let res_arr = single_data["results"].as_array().unwrap();
        assert_eq!(res_arr.len(), 1);
        assert_eq!(res_arr[0].as_array().unwrap().len(), 30);
    }
}
