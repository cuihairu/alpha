//! Alpha Finance WASM 分析引擎
//!
//! 在浏览器中运行的高性能数据分析引擎

use alpha_core::{
    analytics::AnalysisEngine,
    indicators::{advanced::AdvancedIndicators, TechnicalIndicators},
    models::*,
};
use chrono::Utc;
use wasm_bindgen::prelude::*;

mod arrow_adapter;
mod shared_buffer;
mod storage;
mod streaming;
mod websocket;
mod worker;

pub use arrow_adapter::{ArrowBatch, ArrowMemoryPool};
pub use shared_buffer::{SharedF64Buffer, MAX_BUFFER_LEN};
pub use storage::{HybridStorage, IndexedDBStorage};
pub use streaming::{BatchStreamProcessor, StreamProcessor};
pub use websocket::WebSocketClient;
pub use worker::{
    handle_task, init_worker_panic_hook, plan_chunks, BatchComputer, ParallelScheduler,
    PoolStrategy, WorkerPool, WorkerResult, WorkerTask,
};

// 在浏览器控制台中显示 panic 信息
#[wasm_bindgen(start)]
pub fn init_panic_hook() {
    console_error_panic_hook::set_once();
}

/// WASM 分析引擎
#[wasm_bindgen]
pub struct WasmAnalyzer {
    engine: AnalysisEngine,
    indicators: TechnicalIndicators,
    advanced: AdvancedIndicators,
}

#[wasm_bindgen]
impl WasmAnalyzer {
    /// 创建新的 WASM 分析器
    #[wasm_bindgen(constructor)]
    pub fn new(precision: Option<usize>) -> WasmAnalyzer {
        let precision = precision.unwrap_or(4);
        WasmAnalyzer {
            engine: AnalysisEngine::with_precision(precision),
            indicators: TechnicalIndicators::with_precision(precision),
            advanced: AdvancedIndicators::with_precision(precision),
        }
    }

    /// 分析股票数据
    #[wasm_bindgen(js_name = analyzeSymbol)]
    pub async fn analyze_symbol(
        &self,
        _symbol: &str,
        data_js: &JsValue,
    ) -> Result<JsValue, JsValue> {
        // 转换 JavaScript 数据到 Rust 结构
        let market_data: Result<Vec<MarketData>, _> =
            serde_wasm_bindgen::from_value(data_js.clone());
        let market_data =
            market_data.map_err(|e| JsValue::from_str(&format!("数据转换错误: {}", e)))?;

        if market_data.is_empty() {
            return Err(JsValue::from_str("市场数据不能为空"));
        }

        // 执行分析
        let analysis_result = self
            .engine
            .analyze_symbol(&market_data, None)
            .await
            .map_err(|e| JsValue::from_str(&format!("分析失败: {}", e)))?;

        // 转换结果为 JavaScript 对象
        let result_js = serde_wasm_bindgen::to_value(&analysis_result)
            .map_err(|e| JsValue::from_str(&format!("结果序列化错误: {}", e)))?;

        Ok(result_js)
    }

    /// 计算 RSI 指标
    #[wasm_bindgen(js_name = calculateRSI)]
    pub fn calculate_rsi(
        &self,
        prices_js: &js_sys::Float64Array,
        period: usize,
    ) -> js_sys::Float64Array {
        let prices: Vec<f64> = prices_js.to_vec();
        let rsi = self.indicators.calculate_rsi(&prices, period);
        js_sys::Float64Array::from(&rsi[..])
    }

    /// 计算移动平均线
    #[wasm_bindgen(js_name = calculateSMA)]
    pub fn calculate_sma(
        &self,
        prices_js: &js_sys::Float64Array,
        period: usize,
    ) -> js_sys::Float64Array {
        let prices: Vec<f64> = prices_js.to_vec();
        let sma = self.indicators.calculate_sma(&prices, period);
        js_sys::Float64Array::from(&sma[..])
    }

    /// 计算指数移动平均线
    #[wasm_bindgen(js_name = calculateEMA)]
    pub fn calculate_ema(
        &self,
        prices_js: &js_sys::Float64Array,
        period: usize,
    ) -> js_sys::Float64Array {
        let prices: Vec<f64> = prices_js.to_vec();
        let ema = self.indicators.calculate_ema(&prices, period);
        js_sys::Float64Array::from(&ema[..])
    }

    /// 计算布林带
    #[wasm_bindgen(js_name = calculateBollingerBands)]
    pub fn calculate_bollinger_bands(
        &self,
        prices_js: &js_sys::Float64Array,
        period: usize,
        std_dev: f64,
    ) -> JsValue {
        let prices: Vec<f64> = prices_js.to_vec();
        let (upper, middle, lower) = self
            .indicators
            .calculate_bollinger_bands(&prices, period, std_dev);

        let result = serde_json::json!({
            "upper": upper,
            "middle": middle,
            "lower": lower
        });

        serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
    }

    /// 计算 MACD
    #[wasm_bindgen(js_name = calculateMACD)]
    pub fn calculate_macd(
        &self,
        prices_js: &js_sys::Float64Array,
        fast_period: usize,
        slow_period: usize,
        signal_period: usize,
    ) -> JsValue {
        let prices: Vec<f64> = prices_js.to_vec();
        let (macd_line, signal_line, histogram) =
            self.indicators
                .calculate_macd(&prices, fast_period, slow_period, signal_period);

        let result = serde_json::json!({
            "macd": macd_line,
            "signal": signal_line,
            "histogram": histogram
        });

        serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
    }

    // ===== 高级指标 (AdvancedIndicators) =====

    /// 计算随机指标 (Stochastic Oscillator)
    /// 需要 high/low/close 三条价格序列
    #[wasm_bindgen(js_name = calculateStochastic)]
    pub fn calculate_stochastic(
        &self,
        highs_js: &js_sys::Float64Array,
        lows_js: &js_sys::Float64Array,
        closes_js: &js_sys::Float64Array,
        k_period: usize,
        d_period: usize,
    ) -> JsValue {
        let highs: Vec<f64> = highs_js.to_vec();
        let lows: Vec<f64> = lows_js.to_vec();
        let closes: Vec<f64> = closes_js.to_vec();
        let (k_values, d_values) = self
            .advanced
            .calculate_stochastic(&highs, &lows, &closes, k_period, d_period);

        let result = serde_json::json!({
            "k": k_values,
            "d": d_values
        });

        serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
    }

    /// 计算 KDJ 指标（随机指标 KDJ 语义，即 Stochastic Oscillator 的 %K/%D）
    /// %K = 100·(C−LL)/(HH−LL)（k_period 窗口），%D = %K 的 d_period 期 SMA。
    /// 双序列输出：单个 `js_sys::Float64Array` 装不下 %K/%D 两条线，故沿用同文件
    /// [`WasmAnalyzer::calculate_stochastic`] 的对象形态 `{k, d}`（键为 serde
    /// 默认 snake_case，与 calculateAllIndicators 输出键风格一致）。
    #[wasm_bindgen(js_name = calculateKDJ)]
    pub fn calculate_kdj(
        &self,
        highs_js: &js_sys::Float64Array,
        lows_js: &js_sys::Float64Array,
        closes_js: &js_sys::Float64Array,
        k_period: usize,
        d_period: usize,
    ) -> JsValue {
        let highs: Vec<f64> = highs_js.to_vec();
        let lows: Vec<f64> = lows_js.to_vec();
        let closes: Vec<f64> = closes_js.to_vec();
        let (k_values, d_values) = self
            .advanced
            .calculate_stochastic(&highs, &lows, &closes, k_period, d_period);

        let result = serde_json::json!({
            "k": k_values,
            "d": d_values
        });

        serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
    }

    /// 计算威廉指标 (Williams %R)
    #[wasm_bindgen(js_name = calculateWilliamsR)]
    pub fn calculate_williams_r(
        &self,
        highs_js: &js_sys::Float64Array,
        lows_js: &js_sys::Float64Array,
        closes_js: &js_sys::Float64Array,
        period: usize,
    ) -> js_sys::Float64Array {
        let highs: Vec<f64> = highs_js.to_vec();
        let lows: Vec<f64> = lows_js.to_vec();
        let closes: Vec<f64> = closes_js.to_vec();
        let wr = self
            .advanced
            .calculate_williams_r(&highs, &lows, &closes, period);
        js_sys::Float64Array::from(&wr[..])
    }

    /// 计算商品通道指数 (CCI)
    #[wasm_bindgen(js_name = calculateCCI)]
    pub fn calculate_cci(
        &self,
        highs_js: &js_sys::Float64Array,
        lows_js: &js_sys::Float64Array,
        closes_js: &js_sys::Float64Array,
        period: usize,
        constant: f64,
    ) -> js_sys::Float64Array {
        let highs: Vec<f64> = highs_js.to_vec();
        let lows: Vec<f64> = lows_js.to_vec();
        let closes: Vec<f64> = closes_js.to_vec();
        let cci = self
            .advanced
            .calculate_cci(&highs, &lows, &closes, period, constant);
        js_sys::Float64Array::from(&cci[..])
    }

    /// 计算平均真实波幅 (ATR)
    #[wasm_bindgen(js_name = calculateATR)]
    pub fn calculate_atr(
        &self,
        highs_js: &js_sys::Float64Array,
        lows_js: &js_sys::Float64Array,
        closes_js: &js_sys::Float64Array,
        period: usize,
    ) -> js_sys::Float64Array {
        let highs: Vec<f64> = highs_js.to_vec();
        let lows: Vec<f64> = lows_js.to_vec();
        let closes: Vec<f64> = closes_js.to_vec();
        let atr = self.advanced.calculate_atr(&highs, &lows, &closes, period);
        js_sys::Float64Array::from(&atr[..])
    }

    /// 计算平均趋向指数 (ADX, Wilder 平滑)
    /// 单序列输出，形态与 calculateCCI/calculateATR 一致（`js_sys::Float64Array`）。
    #[wasm_bindgen(js_name = calculateADX)]
    pub fn calculate_adx(
        &self,
        highs_js: &js_sys::Float64Array,
        lows_js: &js_sys::Float64Array,
        closes_js: &js_sys::Float64Array,
        period: usize,
    ) -> js_sys::Float64Array {
        let highs: Vec<f64> = highs_js.to_vec();
        let lows: Vec<f64> = lows_js.to_vec();
        let closes: Vec<f64> = closes_js.to_vec();
        let adx = self.advanced.calculate_adx(&highs, &lows, &closes, period);
        js_sys::Float64Array::from(&adx[..])
    }

    /// 计算动量指标 (Momentum)
    #[wasm_bindgen(js_name = calculateMomentum)]
    pub fn calculate_momentum(
        &self,
        prices_js: &js_sys::Float64Array,
        period: usize,
    ) -> js_sys::Float64Array {
        let prices: Vec<f64> = prices_js.to_vec();
        let momentum = self.advanced.calculate_momentum(&prices, period);
        js_sys::Float64Array::from(&momentum[..])
    }

    /// 计算变化率 (Rate of Change)
    #[wasm_bindgen(js_name = calculateROC)]
    pub fn calculate_roc(
        &self,
        prices_js: &js_sys::Float64Array,
        period: usize,
    ) -> js_sys::Float64Array {
        let prices: Vec<f64> = prices_js.to_vec();
        let roc = self.advanced.calculate_roc(&prices, period);
        js_sys::Float64Array::from(&roc[..])
    }

    /// 计算布林带宽度
    #[wasm_bindgen(js_name = calculateBollingerBandWidth)]
    pub fn calculate_bollinger_band_width(
        &self,
        upper_js: &js_sys::Float64Array,
        lower_js: &js_sys::Float64Array,
        middle_js: &js_sys::Float64Array,
    ) -> js_sys::Float64Array {
        let upper: Vec<f64> = upper_js.to_vec();
        let lower: Vec<f64> = lower_js.to_vec();
        let middle: Vec<f64> = middle_js.to_vec();
        let width = self
            .advanced
            .calculate_bollinger_band_width(&upper, &lower, &middle);
        js_sys::Float64Array::from(&width[..])
    }

    /// 计算布林带位置 (%B)
    #[wasm_bindgen(js_name = calculateBollingerBandPercentB)]
    pub fn calculate_bollinger_band_percent_b(
        &self,
        price: f64,
        upper_band: f64,
        lower_band: f64,
    ) -> f64 {
        self.advanced
            .calculate_bollinger_band_percent_b(price, upper_band, lower_band)
    }

    /// 识别艾略特波浪模式
    #[wasm_bindgen(js_name = identifyElliottWaves)]
    pub fn identify_elliott_waves(&self, prices_js: &js_sys::Float64Array) -> JsValue {
        let prices: Vec<f64> = prices_js.to_vec();
        let waves = self.advanced.identify_elliott_waves(&prices);

        let result = serde_json::json!({
            "waves": waves.iter().map(|w| serde_json::json!({
                "startIndex": w.start_index,
                "endIndex": w.end_index,
                "waveType": format!("{:?}", w.wave_type),
                "confidence": w.confidence
            })).collect::<Vec<_>>()
        });

        serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
    }

    /// 批量计算多个指标
    /// （参数集镜像 web 前端表单：价格序列 + RSI/SMA 周期组，wasm_bindgen 契约不拆对象）
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = calculateAllIndicators)]
    pub fn calculate_all_indicators(
        &self,
        prices_js: &js_sys::Float64Array,
        rsi_period: usize,
        sma_short: usize,
        sma_long: usize,
        macd_fast: usize,
        macd_slow: usize,
        macd_signal: usize,
    ) -> JsValue {
        let prices: Vec<f64> = prices_js.to_vec();

        // 并行计算多个指标
        let rsi = self.indicators.calculate_rsi(&prices, rsi_period);
        let sma_short_values = self.indicators.calculate_sma(&prices, sma_short);
        let sma_long_values = self.indicators.calculate_sma(&prices, sma_long);
        let (macd_line, signal_line, histogram) =
            self.indicators
                .calculate_macd(&prices, macd_fast, macd_slow, macd_signal);
        let (upper, middle, lower) = self.indicators.calculate_bollinger_bands(&prices, 20, 2.0);

        let result = serde_json::json!({
            "rsi": rsi,
            "sma_short": sma_short_values,
            "sma_long": sma_long_values,
            "macd": {
                "line": macd_line,
                "signal": signal_line,
                "histogram": histogram
            },
            "bollinger": {
                "upper": upper,
                "middle": middle,
                "lower": lower
            }
        });

        serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
    }

    /// SMA 双均线交叉回测（便捷接口：内部 `to_vec()` 一次 memcpy 拷入）。
    /// 一次性调用用它即可；热路径（逐 bar/逐标的反复回测）请走零拷贝组合：
    /// `allocPriceBuffer` → JS 直写视图 → [`WasmAnalyzer::backtest_sma_cross_ptr`]
    /// （复用缓冲）→ `freePriceBuffer`（TODO「实现零拷贝内存管理」，A/B 基准见
    /// examples/zero_copy_bench.rs 与 TODO 注）。
    /// 记账与信号逻辑全部在 alpha-core（L0 纯计算层，单测覆盖），此处仅做 JS 边界转换。
    /// JS 侧字段为 serde 默认 snake_case（equity_curve/total_return_pct/…，
    /// 与 calculateAllIndicators 输出键风格一致），方法名 camelCase 与既有导出一致。
    /// 非法参数（fast >= slow、空序列）会触发 wasm trap——与全 crate 的边界口径一致，
    /// 参数校验由 JS 调用方负责。
    #[wasm_bindgen(js_name = backtestSmaCross)]
    pub fn backtest_sma_cross(
        &self,
        prices_js: &js_sys::Float64Array,
        fast_period: usize,
        slow_period: usize,
        fee_bps: f64,
    ) -> JsValue {
        let prices: Vec<f64> = prices_js.to_vec();
        let mut strategy = alpha_core::backtest::SmaCrossStrategy::new(fast_period, slow_period);
        let report = alpha_core::backtest::BacktestEngine::new(fee_bps).run(&prices, &mut strategy);
        serde_wasm_bindgen::to_value(&report).unwrap_or(JsValue::NULL)
    }

    /// 零拷贝回测：直接读 `allocPriceBuffer` 返回的 `(ptr, len)` 所指价格，
    /// 不做任何拷贝。其余口径与 [`WasmAnalyzer::backtest_sma_cross`] 一致。
    /// 内存扩容警告：本调用内部会为 equity_curve 分配堆内存，可能触发 wasm
    /// 内存 grow——此后 JS 持有的旧视图已失效，需复用时须重新 `allocPriceBuffer`。
    #[wasm_bindgen(js_name = backtestSmaCrossPtr)]
    pub fn backtest_sma_cross_ptr(
        &self,
        ptr: u32,
        len: usize,
        fast_period: usize,
        slow_period: usize,
        fee_bps: f64,
    ) -> Result<JsValue, JsValue> {
        if ptr == 0 || len == 0 {
            return Err(JsValue::from_str(
                "backtestSmaCrossPtr: 非法 (ptr, len)——须来自 allocPriceBuffer 且未释放",
            ));
        }
        // SAFETY: (ptr, len) 契约来自 allocPriceBuffer（8 字节对齐、地址恒定、
        // 未释放），wasm 无法校验外来指针，见 shared_buffer 模块文档
        let prices = unsafe { std::slice::from_raw_parts(ptr as *const f64, len) };
        let mut strategy = alpha_core::backtest::SmaCrossStrategy::new(fast_period, slow_period);
        let report = alpha_core::backtest::BacktestEngine::new(fee_bps).run(prices, &mut strategy);
        Ok(serde_wasm_bindgen::to_value(&report).unwrap_or(JsValue::NULL))
    }

    /// 获取性能指标
    #[wasm_bindgen(js_name = getPerformanceMetrics)]
    pub fn get_performance_metrics(&self) -> JsValue {
        let window = web_sys::window().unwrap();
        let performance = window.performance().unwrap();

        let metrics = serde_json::json!({
            "timestamp": Utc::now().to_rfc3339(),
            "timing": {
                "now": performance.now()
            }
        });

        serde_wasm_bindgen::to_value(&metrics).unwrap_or(JsValue::NULL)
    }

    /// 强制垃圾回收（如果支持）
    #[wasm_bindgen(js_name = forceGC)]
    pub fn force_gc() {
        let window = web_sys::window().unwrap();
        if let Ok(gc) = js_sys::Reflect::get(&window, &JsValue::from_str("gc")) {
            if gc.is_function() {
                js_sys::Function::from(gc).call0(&window).unwrap();
            }
        }
    }
}

/// 零拷贝价格缓冲区分配：在 wasm 线性内存中分配定长 f64 缓冲区，
/// 返回给 JS 的对象包含 `{ptr, len, view}`，其中 `view` 为
/// `Float64Array` 直接映射 wasm 内存（数据 ingress 零拷贝）。
/// 契约见 [`shared_buffer`] 模块文档。
/// 简化口径：此处不携带 `capacity` 字段（`SharedF64Buffer` 保证 capacity==len）；
/// 释放路径见 `freePriceBuffer`。
#[wasm_bindgen(js_name = allocPriceBuffer)]
pub fn alloc_price_buffer(len: usize) -> Result<JsValue, JsValue> {
    if len == 0 {
        return Err(JsValue::from_str(
            "allocPriceBuffer: 长度须大于 0（空序列请直接走便捷接口 backtestSmaCross）",
        ));
    }
    if len > MAX_BUFFER_LEN {
        return Err(JsValue::from_str(&format!(
            "allocPriceBuffer: 长度 {len} 超上限 {MAX_BUFFER_LEN}（防 wasm32 堆 OOM trap）",
        )));
    }
    let buf = SharedF64Buffer::alloc(len);
    let ptr = buf.as_ptr() as u32;
    // SAFETY: ptr 来自刚分配且随即 forget 的 Vec<f64>（8 字节对齐、地址恒定），
    // 此调用仅建视图、不分配；视图失效条件（后续 grow）已在文档契约声明
    let view = unsafe { js_sys::Float64Array::view_mut_raw(buf.as_ptr() as *mut f64, len) };
    let handle = js_sys::Object::new();
    js_sys::Reflect::set(
        &handle,
        &JsValue::from_str("ptr"),
        &JsValue::from_f64(ptr as f64),
    )?;
    js_sys::Reflect::set(
        &handle,
        &JsValue::from_str("len"),
        &JsValue::from_f64(len as f64),
    )?;
    js_sys::Reflect::set(&handle, &JsValue::from_str("view"), &view)?;
    // 所有权移交 JS 侧（ forgotten 直到 freePriceBuffer 回传 (ptr, len) ）
    std::mem::forget(buf);
    Ok(handle.into())
}

/// 释放 `allocPriceBuffer` 分配的缓冲区。同一 `(ptr, len)` 只能调用一次。
#[wasm_bindgen(js_name = freePriceBuffer)]
pub fn free_price_buffer(ptr: u32, len: usize) -> Result<(), JsValue> {
    if ptr == 0 || len == 0 {
        return Err(JsValue::from_str("freePriceBuffer: 非法 (ptr, len)"));
    }
    // SAFETY: (ptr, len) 契约来自 allocPriceBuffer 且未释放过；
    // wasm 无法校验外来指针，双 free/伪造指针 = UB，由调用方保证
    drop(unsafe { SharedF64Buffer::from_raw(ptr as *mut f64, len) });
    Ok(())
}

/// 工具函数
#[wasm_bindgen]
pub struct Utils;

#[wasm_bindgen]
impl Utils {
    /// 格式化数字为指定精度
    #[wasm_bindgen(js_name = roundTo)]
    pub fn round_to(value: f64, precision: usize) -> f64 {
        let multiplier = 10_f64.powi(precision as i32);
        (value * multiplier).round() / multiplier
    }

    /// 计算百分比变化
    #[wasm_bindgen(js_name = percentChange)]
    pub fn percent_change(old_value: f64, new_value: f64) -> f64 {
        if old_value == 0.0 {
            return 0.0;
        }
        ((new_value - old_value) / old_value) * 100.0
    }

    /// 验证股票代码
    #[wasm_bindgen(js_name = validateSymbol)]
    pub fn validate_symbol(symbol: &str) -> bool {
        !symbol.is_empty()
            && symbol.len() <= 10
            && symbol.chars().all(|c| c.is_alphanumeric() || c == '.')
    }

    /// 获取当前时间戳
    #[wasm_bindgen(js_name = getCurrentTimestamp)]
    pub fn get_current_timestamp() -> f64 {
        Utc::now().timestamp_millis() as f64
    }

    /// 格式化数字为货币格式
    #[wasm_bindgen(js_name = formatCurrency)]
    pub fn format_currency(value: f64, currency: &str) -> String {
        match currency.to_uppercase().as_str() {
            "USD" => format!("${:.2}", value),
            "CNY" => format!("¥{:.2}", value),
            "EUR" => format!("€{:.2}", value),
            _ => format!("{:.2}", value),
        }
    }

    /// 生成唯一 ID
    #[wasm_bindgen(js_name = generateId)]
    pub fn generate_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

/// 实时数据同步的 wasm 薄绑定（算法核心在 `alpha_core::sync`，此处只做 JS 边界转换）
///
/// 架构注：浏览器侧 WebSocket 与同步状态机归属 JS 主线程（wasm-analyzer 的
/// `WebSocketClient` 只管传输），本段提供两条缝：
/// * [`sync_build_delta`]/[`sync_apply_delta`]：增量帧生成/合入落在 Rust 侧，
///   与 real-time-feed 服务端复用同一 `build_delta` 实现，两端协议不漂移；
/// * [`WasmSyncEngine`]：把 alpha-core 的版本/丢帧状态机暴露为 JS 对象句柄，
///   Full/Delta 接入、Gap 检测、Resync 基线恢复由同一实现驱动（逻辑单测在
///   alpha-core，此处验证 JS 边界转换形态）。

/// 增量生成：`prev`/`next` 对象 → 仅变化字段的 Delta（与服务端同实现）。
/// `#[allow(clippy::empty_line_after_outer_attr)]`：此块 `///` 后留有空行是故意的，
/// 保持多行文档注释在视觉上与随后的 `#[wasm_bindgen]` 属性分组整洁。
#[allow(clippy::empty_line_after_outer_attr)]
#[wasm_bindgen(js_name = buildSyncDelta)]
pub fn sync_build_delta(prev: JsValue, next: JsValue) -> Result<JsValue, JsValue> {
    let prev: serde_json::Value = serde_wasm_bindgen::from_value(prev)
        .map_err(|e| JsValue::from_str(&format!("prev 解析失败: {e}")))?;
    let next: serde_json::Value = serde_wasm_bindgen::from_value(next)
        .map_err(|e| JsValue::from_str(&format!("next 解析失败: {e}")))?;
    serde_wasm_bindgen::to_value(&alpha_core::sync::build_delta(&prev, &next))
        .map_err(|e| JsValue::from_str(&format!("delta 序列化失败: {e}")))
}

/// 增量合入：`base` 快照 ⊕ `delta` → 新快照（与 `buildSyncDelta` 互为逆操作）
#[wasm_bindgen(js_name = applySyncDelta)]
pub fn sync_apply_delta(base: JsValue, delta: JsValue) -> Result<JsValue, JsValue> {
    let base: serde_json::Value = serde_wasm_bindgen::from_value(base)
        .map_err(|e| JsValue::from_str(&format!("base 解析失败: {e}")))?;
    let delta: serde_json::Value = serde_wasm_bindgen::from_value(delta)
        .map_err(|e| JsValue::from_str(&format!("delta 解析失败: {e}")))?;
    serde_wasm_bindgen::to_value(&alpha_core::sync::apply_delta(&base, &delta))
        .map_err(|e| JsValue::from_str(&format!("合并结果序列化失败: {e}")))
}

/// 版本化同步状态机（JS 对象句柄）：Full/Delta 接入、丢帧检测、Resync 基线恢复。
/// 语义与 `alpha_core::sync::SyncEngine` 完全一致（同一实现），此处仅包
/// `RefCell` 暴露可变状态（JS 单线程，无并发借用）。
#[wasm_bindgen]
pub struct WasmSyncEngine {
    inner: std::cell::RefCell<alpha_core::sync::SyncEngine>,
}

#[wasm_bindgen]
impl WasmSyncEngine {
    /// 新建空状态机（未跟踪任何通道）
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: std::cell::RefCell::new(alpha_core::sync::SyncEngine::new()),
        }
    }

    /// 应用一帧。`op` 取 `"full"` | `"delta"`（大小写不敏感）。
    /// 返回结果对象（snake_case）：
    /// `{"kind":"Advanced","seq":N}` / `{"kind":"Idempotent","seq":N}` /
    /// `{"kind":"Gap","expected":N,"got":M}`；Delta 无本地快照报错。
    ///
    /// `seq` 以 f64 传入（JS number），通道版本号远小于 2^53 精度上限。
    pub fn apply(
        &self,
        channel: &str,
        seq: f64,
        op: &str,
        data: JsValue,
    ) -> Result<JsValue, JsValue> {
        let data: serde_json::Value = serde_wasm_bindgen::from_value(data)
            .map_err(|e| JsValue::from_str(&format!("data 解析失败: {e}")))?;
        let outcome = match op.to_ascii_lowercase().as_str() {
            "full" => self
                .inner
                .borrow_mut()
                .apply_full(channel, seq as u64, data),
            "delta" => match self
                .inner
                .borrow_mut()
                .apply_delta(channel, seq as u64, data)
            {
                Ok(outcome) => outcome,
                Err(e) => return Err(JsValue::from_str(&e.to_string())),
            },
            other => {
                return Err(JsValue::from_str(&format!(
                    "未知 op: {other}（应为 full|delta）"
                )))
            }
        };
        let result = match outcome {
            alpha_core::sync::SyncOutcome::Advanced { seq } => {
                serde_json::json!({"kind": "Advanced", "seq": seq})
            }
            alpha_core::sync::SyncOutcome::Idempotent { seq } => {
                serde_json::json!({"kind": "Idempotent", "seq": seq})
            }
            alpha_core::sync::SyncOutcome::Gap { expected, got } => {
                serde_json::json!({"kind": "Gap", "expected": expected, "got": got})
            }
        };
        serde_wasm_bindgen::to_value(&result)
            .map_err(|e| JsValue::from_str(&format!("结果序列化失败: {e}")))
    }

    /// 通道本地快照（对象），未跟踪/无快照返回 `null`
    pub fn snapshot(&self, channel: &str) -> Result<JsValue, JsValue> {
        match self.inner.borrow().snapshot(channel) {
            Some(value) => serde_wasm_bindgen::to_value(value)
                .map_err(|e| JsValue::from_str(&format!("快照序列化失败: {e}"))),
            None => Ok(JsValue::NULL),
        }
    }

    /// 通道最近应用版本（number），未跟踪返回 `null`
    pub fn last_seq(&self, channel: &str) -> JsValue {
        match self.inner.borrow().last_seq(channel) {
            Some(seq) => JsValue::from_f64(seq as f64),
            None => JsValue::NULL,
        }
    }

    /// 检测到 Gap 后 Resync 应发起的基准版本（number）
    pub fn resync_from(&self, channel: &str) -> JsValue {
        JsValue::from_f64(self.inner.borrow().resync_from(channel) as f64)
    }

    /// 重连清理：丢弃通道本地状态（下个 `full` 帧重建基线）
    pub fn reset_channel(&self, channel: &str) {
        self.inner.borrow_mut().reset_channel(channel);
    }
}

/// Default 契约：等价于 `Self::new()`，便于 `#[serde]` / 反序列化场景默认值。
#[allow(clippy::new_without_default)]
impl Default for WasmSyncEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    #[wasm_bindgen_test]
    fn test_utils_functions() {
        let pi = std::f64::consts::PI;
        let expected = (pi * 100.0).round() / 100.0;
        assert!((Utils::round_to(pi, 2) - expected).abs() < f64::EPSILON);
        assert_eq!(Utils::percent_change(100.0, 110.0), 10.0);
        assert!(Utils::validate_symbol("AAPL"));
        assert!(!Utils::validate_symbol(""));
        assert!(!Utils::validate_symbol("TOO_LONG_SYMBOL_12345"));
    }

    #[wasm_bindgen_test]
    fn test_analyzer_creation() {
        let _analyzer = WasmAnalyzer::new(None);
        let _analyzer_with_precision = WasmAnalyzer::new(Some(4));
    }

    /// 同步绑定：增量生成/合入 + 状态机全链路（Full→Delta→Gap→Resync 恢复）。
    /// 逻辑断言在 alpha-core 单测，此处锁定 JS 边界转换形态（snake_case 结果对象）
    #[wasm_bindgen_test]
    fn test_sync_binding_delta_and_engine() {
        // 纯函数：buildSyncDelta 只保留变化字段
        let prev = serde_wasm_bindgen::to_value(&serde_json::json!({"price": 10.0, "volume": 100}))
            .unwrap();
        let next = serde_wasm_bindgen::to_value(&serde_json::json!({"price": 10.5, "volume": 100}))
            .unwrap();
        let delta = sync_build_delta(prev, next).unwrap();
        let delta_val: serde_json::Value = serde_wasm_bindgen::from_value(delta).unwrap();
        assert_eq!(delta_val, serde_json::json!({"price": 10.5}));

        // 纯函数：applySyncDelta 与 buildSyncDelta 互逆
        let merged = sync_apply_delta(
            serde_wasm_bindgen::to_value(&serde_json::json!({"price": 10.0, "volume": 100}))
                .unwrap(),
            serde_wasm_bindgen::to_value(&serde_json::json!({"price": 10.5})).unwrap(),
        )
        .unwrap();
        let merged_val: serde_json::Value = serde_wasm_bindgen::from_value(merged).unwrap();
        assert_eq!(
            merged_val,
            serde_json::json!({"price": 10.5, "volume": 100})
        );

        // 状态机：Full 建基线 → Delta 丢帧（seq 3）→ Gap → Resync Full 恢复
        let engine = WasmSyncEngine::new();
        let out_full = engine
            .apply(
                "rtq",
                1.0,
                "full",
                serde_wasm_bindgen::to_value(&serde_json::json!({"price": 10.0})).unwrap(),
            )
            .unwrap();
        let out_full_val: serde_json::Value = serde_wasm_bindgen::from_value(out_full).unwrap();
        assert_eq!(out_full_val["kind"], "Advanced");
        assert_eq!(out_full_val["seq"], 1);

        let out_gap = engine
            .apply(
                "rtq",
                3.0,
                "delta",
                serde_wasm_bindgen::to_value(&serde_json::json!({"price": 12.0})).unwrap(),
            )
            .unwrap();
        let out_gap_val: serde_json::Value = serde_wasm_bindgen::from_value(out_gap).unwrap();
        assert_eq!(out_gap_val["kind"], "Gap");
        assert_eq!(out_gap_val["expected"], 2);
        assert_eq!(out_gap_val["got"], 3);
        // Gap 不污染快照
        let snap_val: serde_json::Value =
            serde_wasm_bindgen::from_value(engine.snapshot("rtq").unwrap()).unwrap();
        assert_eq!(snap_val["price"], 10.0);
        // resync_from 给出恢复基准
        assert_eq!(engine.resync_from("rtq").as_f64().unwrap(), 1.0);

        // Resync 恢复：Full 以最新版本重建基线
        let out_recover = engine
            .apply(
                "rtq",
                3.0,
                "full",
                serde_wasm_bindgen::to_value(&serde_json::json!({"price": 12.0, "volume": 99}))
                    .unwrap(),
            )
            .unwrap();
        let out_rec_val: serde_json::Value = serde_wasm_bindgen::from_value(out_recover).unwrap();
        assert_eq!(out_rec_val["kind"], "Advanced");
        let restored: serde_json::Value =
            serde_wasm_bindgen::from_value(engine.snapshot("rtq").unwrap()).unwrap();
        assert_eq!(restored["price"], 12.0);
        assert_eq!(restored["volume"], 99);
        assert_eq!(engine.last_seq("rtq").as_f64().unwrap(), 3.0);

        // Delta 无本地快照：报错（协议违约直接暴露给 JS）
        assert!(engine
            .apply(
                "nope",
                1.0,
                "delta",
                serde_wasm_bindgen::to_value(&serde_json::json!({})).unwrap(),
            )
            .is_err());
        // 未知 op：报错
        assert!(engine
            .apply(
                "rtq",
                4.0,
                "bogus",
                serde_wasm_bindgen::to_value(&serde_json::json!({})).unwrap(),
            )
            .is_err());

        // reset_channel：清理后 last_seq 回 null
        engine.reset_channel("rtq");
        assert!(engine.last_seq("rtq").is_null());
    }

    /// 回测绑定：上涨序列净值应为持有收益（逻辑断言在 alpha-core 单测，
    /// 此处验证 JS 边界转换后报告结构完整、字段为 serde 默认 snake_case）
    /// 零拷贝全流程（alloc → JS 直写视图 → ptr 计算 → free）：
    /// 与便捷接口同口径，且计算侧读到的就是 JS 写入的数据
    #[wasm_bindgen_test]
    fn test_zero_copy_backtest_flow() {
        let handle = alloc_price_buffer(5).expect("alloc 应成功");
        let ptr = js_sys::Reflect::get(&handle, &JsValue::from_str("ptr"))
            .unwrap()
            .as_f64()
            .unwrap() as u32;
        let len = js_sys::Reflect::get(&handle, &JsValue::from_str("len"))
            .unwrap()
            .as_f64()
            .unwrap() as usize;
        assert_eq!(len, 5);
        let view: js_sys::Float64Array = js_sys::Reflect::get(&handle, &JsValue::from_str("view"))
            .unwrap()
            .into();
        view.set(
            &js_sys::Float64Array::from(&[10.0, 20.0, 30.0, 40.0, 50.0][..]),
            0,
        );

        let analyzer = WasmAnalyzer::new(None);
        let report = analyzer
            .backtest_sma_cross_ptr(ptr, len, 1, 2, 0.0)
            .expect("ptr 回测应成功");
        let total_return = js_sys::Reflect::get(&report, &JsValue::from_str("total_return_pct"))
            .expect("报告应含 total_return_pct");
        assert!(
            (total_return.as_f64().unwrap_or(0.0) - 150.0).abs() < 1e-9,
            "零拷贝路径与便捷路径同口径：bar1 入场 20 持有到 50 = 2.5x 应 +150%"
        );

        free_price_buffer(ptr, len).expect("free 应成功");
        // 双 free 拒绝（契约前置检查，非 UB 路径）
        assert!(
            free_price_buffer(ptr, len).is_err(),
            "同一 (ptr, len) 二次 free 必须报错"
        );
    }

    #[wasm_bindgen_test]
    fn test_backtest_sma_cross_binding() {
        let analyzer = WasmAnalyzer::new(None);
        let prices = js_sys::Float64Array::from(&[10.0, 20.0, 30.0, 40.0, 50.0][..]);
        let report = analyzer.backtest_sma_cross(&prices, 1, 2, 0.0);
        let equity = js_sys::Reflect::get(&report, &JsValue::from_str("equity_curve"))
            .expect("报告应含 equity_curve");
        assert!(js_sys::Array::is_array(&equity), "equity_curve 应为数组");
        let equity_len = js_sys::Array::from(&equity).length();
        assert_eq!(equity_len, 5, "逐 bar 净值长度应等于价格序列长度");
        let total_return = js_sys::Reflect::get(&report, &JsValue::from_str("total_return_pct"))
            .expect("报告应含 total_return_pct");
        assert!(
            (total_return.as_f64().unwrap_or(0.0) - 150.0).abs() < 1e-9,
            "bar1 入场 20 持有到 50 = 2.5x 应 +150%"
        );
    }
}
