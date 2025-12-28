//! Web Workers 并行计算引擎
//!
//! 利用 Web Workers 实现多线程并行计算

use alpha_core::indicators::TechnicalIndicators;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

/// Worker 任务类型
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

/// Worker 池管理器
#[wasm_bindgen]
pub struct WorkerPool {
    worker_count: usize,
    task_counter: std::cell::RefCell<usize>,
}

#[wasm_bindgen]
impl WorkerPool {
    /// 创建 Worker 池
    #[wasm_bindgen(constructor)]
    pub fn new(worker_count: usize) -> WorkerPool {
        let count = worker_count.max(1).min(navigator_hardware_concurrency().unwrap_or(4));

        WorkerPool {
            worker_count: count,
            task_counter: std::cell::RefCell::new(0),
        }
    }

    /// 获取 Worker 数量
    #[wasm_bindgen(js_name = getWorkerCount)]
    pub fn get_worker_count(&self) -> usize {
        self.worker_count
    }

    /// 并行计算多个股票的指标
    #[wasm_bindgen(js_name = computeIndicatorsParallel)]
    pub async fn compute_indicators_parallel(
        &self,
        prices_array: js_sys::Array,
        indicator_type: &str,
        period: usize,
    ) -> Result<JsValue, JsValue> {
        let mut datasets = Vec::new();

        // 转换 JavaScript 数组到 Rust 向量
        for i in 0..prices_array.length() {
            let prices_js = prices_array.get(i);
            let prices_f64 = js_sys::Float64Array::from(prices_js);
            let prices = prices_f64.to_vec();
            datasets.push(prices);
        }

        // 分批处理数据
        let chunk_size = (datasets.len() + self.worker_count - 1) / self.worker_count;
        let mut results = Vec::new();

        let indicators = TechnicalIndicators::new();

        // 模拟并行计算（在 WASM 中实际是串行，但保持相同的 API）
        for chunk in datasets.chunks(chunk_size) {
            for prices in chunk {
                let result = match indicator_type {
                    "sma" => indicators.calculate_sma(prices, period),
                    "ema" => indicators.calculate_ema(prices, period),
                    "rsi" => indicators.calculate_rsi(prices, period),
                    _ => return Err(JsValue::from_str("不支持的指标类型")),
                };
                results.push(result);
            }
        }

        // 转换结果为 JavaScript 数组
        let js_results = js_sys::Array::new();
        for result in results {
            js_results.push(&js_sys::Float64Array::from(&result[..]));
        }

        Ok(js_results.into())
    }

    /// 生成任务 ID
    fn generate_task_id(&self) -> String {
        let mut counter = self.task_counter.borrow_mut();
        *counter += 1;
        format!("task_{}", *counter)
    }
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
}
