//! Alpha Finance WASM 分析引擎
//!
//! 在浏览器中运行的高性能数据分析引擎

use alpha_core::{
    analytics::AnalysisEngine,
    indicators::{TechnicalIndicators, advanced::AdvancedIndicators},
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
pub use worker::{BatchComputer, ParallelScheduler, WorkerPool};

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

    /// SMA 双均线交叉回测（TODO「开发高性能 Rust WASM 核心计算库」最小可用版本）。
    /// 记账与信号逻辑全部在 alpha-core（L0 纯计算层，单测覆盖），此处仅做 JS 边界转换。
    /// JS 侧字段为 serde 默认 snake_case（equity_curve/total_return_pct/…，
    /// 与 calculateAllIndicators 输出键风格一致），方法名 camelCase 与既有导出一致。
    /// 简化口径：`to_vec()` 一次 memcpy 拷入；零拷贝视图留待「零拷贝内存管理」立项统一处理。
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

    /// 回测绑定：上涨序列净值应为持有收益（逻辑断言在 alpha-core 单测，
    /// 此处验证 JS 边界转换后报告结构完整、字段为 serde 默认 snake_case）
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
