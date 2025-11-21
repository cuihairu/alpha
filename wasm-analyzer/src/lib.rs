//! Alpha Finance WASM 分析引擎
//!
//! 在浏览器中运行的高性能数据分析引擎

use wasm_bindgen::prelude::*;
use alpha_core::{models::*, analytics::AnalysisEngine, indicators::TechnicalIndicators};
use chrono::Utc;

// 在浏览器控制台中显示 panic 信息
#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    tracing_wasm::set_as_global_default();
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
    pub fn new() -> WasmAnalyzer {
        WasmAnalyzer {
            engine: AnalysisEngine::new(),
            indicators: TechnicalIndicators::new(),
        }
    }

    /// 创建带精度的分析器
    #[wasm_bindgen(constructor)]
    pub fn with_precision(precision: usize) -> WasmAnalyzer {
        WasmAnalyzer {
            engine: AnalysisEngine::with_precision(precision),
            indicators: TechnicalIndicators::with_precision(precision),
        }
    }

    /// 分析股票数据
    #[wasm_bindgen(js_name = analyzeSymbol)]
    pub async fn analyze_symbol(&self, symbol: &str, data_js: &JsValue) -> Result<JsValue, JsValue> {
        // 转换 JavaScript 数据到 Rust 结构
        let market_data: Vec<MarketData> = data_js
            .into_serde()
            .map_err(|e| JsValue::from_str(&format!("数据转换错误: {}", e)))?;

        if market_data.is_empty() {
            return Err(JsValue::from_str("市场数据不能为空"));
        }

        // 执行分析
        let analysis_result = self.engine.analyze_symbol(&market_data, None)
            .await
            .map_err(|e| JsValue::from_str(&format!("分析失败: {}", e)))?;

        // 转换结果为 JavaScript 对象
        let result_js = JsValue::from_serde(&analysis_result)
            .map_err(|e| JsValue::from_str(&format!("结果序列化错误: {}", e)))?;

        Ok(result_js)
    }

    /// 计算 RSI 指标
    #[wasm_bindgen(js_name = calculateRSI)]
    pub fn calculate_rsi(&self, prices_js: &js_sys::Float64Array, period: usize) -> js_sys::Float64Array {
        let prices: Vec<f64> = prices_js.to_vec();
        let rsi = self.indicators.calculate_rsi(&prices, period);
        js_sys::Float64Array::from(&rsi[..])
    }

    /// 计算移动平均线
    #[wasm_bindgen(js_name = calculateSMA)]
    pub fn calculate_sma(&self, prices_js: &js_sys::Float64Array, period: usize) -> js_sys::Float64Array {
        let prices: Vec<f64> = prices_js.to_vec();
        let sma = self.indicators.calculate_sma(&prices, period);
        js_sys::Float64Array::from(&sma[..])
    }

    /// 计算指数移动平均线
    #[wasm_bindgen(js_name = calculateEMA)]
    pub fn calculate_ema(&self, prices_js: &js_sys::Float64Array, period: usize) -> js_sys::Float64Array {
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
        let (upper, middle, lower) = self.indicators.calculate_bollinger_bands(&prices, period, std_dev);

        let result = serde_json::json!({
            "upper": upper,
            "middle": middle,
            "lower": lower
        });

        JsValue::from_serde(&result).unwrap_or(JsValue::NULL)
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
        let (macd_line, signal_line, histogram) = self.indicators.calculate_macd(&prices, fast_period, slow_period, signal_period);

        let result = serde_json::json!({
            "macd": macd_line,
            "signal": signal_line,
            "histogram": histogram
        });

        JsValue::from_serde(&result).unwrap_or(JsValue::NULL)
    }

    /// 批量计算多个指标
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
        let (macd_line, signal_line, histogram) = self.indicators.calculate_macd(&prices, macd_fast, macd_slow, macd_signal);
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

        JsValue::from_serde(&result).unwrap_or(JsValue::NULL)
    }

    /// 获取性能指标
    #[wasm_bindgen(js_name = getPerformanceMetrics)]
    pub fn get_performance_metrics(&self) -> JsValue {
        let window = web_sys::window().unwrap();
        let performance = window.performance().unwrap();

        let metrics = serde_json::json!({
            "timestamp": Utc::now().to_rfc3339(),
            "memory": {
                "used": performance.memory().map(|m| m.used_js_heap_size()).unwrap_or(0),
                "total": performance.memory().map(|m| m.total_js_heap_size()).unwrap_or(0),
                "limit": performance.memory().map(|m| m.js_heap_size_limit()).unwrap_or(0)
            },
            "timing": {
                "now": performance.now()
            }
        });

        JsValue::from_serde(&metrics).unwrap_or(JsValue::NULL)
    }

    /// 强制垃圾回收（如果支持）
    #[wasm_bindgen(js_name = forceGC)]
    pub fn force_gc() {
        let window = web_sys::window().unwrap();
        if let Some(gc) = js_sys::Reflect::get(&window, &JsValue::from_str("gc")).ok() {
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
        !symbol.is_empty() && symbol.len() <= 10 && symbol.chars().all(|c| c.is_alphanumeric() || c == '.')
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

// 导入 tracing-wasm 用于浏览器日志
#[cfg(target_arch = "wasm32")]
mod tracing_wasm {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(module = "/tracing_wasm.js")]
    extern "C" {
        #[wasm_bindgen(js_name = setAsGlobalDefault)]
        pub fn set_as_global_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    #[wasm_bindgen_test]
    fn test_utils_functions() {
        assert!(Utils::round_to(3.14159, 2) - 3.14 < f64::EPSILON);
        assert_eq!(Utils::percent_change(100.0, 110.0), 10.0);
        assert!(Utils::validate_symbol("AAPL"));
        assert!(!Utils::validate_symbol(""));
        assert!(!Utils::validate_symbol("TOO_LONG_SYMBOL_12345"));
    }

    #[wasm_bindgen_test]
    fn test_analyzer_creation() {
        let analyzer = WasmAnalyzer::new();
        let analyzer_with_precision = WasmAnalyzer::with_precision(4);

        // 这些应该能成功创建
        assert!(true);
    }
}