//! 流式数据处理模块
//!
//! 提供高效的增量数据处理和实时计算能力

use alpha_core::{indicators::TechnicalIndicators, models::MarketData};
use std::collections::VecDeque;
use wasm_bindgen::prelude::*;

/// 流式数据处理器
#[wasm_bindgen]
pub struct StreamProcessor {
    /// 滑动窗口缓冲区
    buffer: VecDeque<MarketData>,
    /// 窗口大小
    window_size: usize,
    /// 技术指标计算器
    indicators: TechnicalIndicators,
    /// 处理的数据点总数
    processed_count: usize,
}

#[wasm_bindgen]
impl StreamProcessor {
    /// 创建新的流处理器
    #[wasm_bindgen(constructor)]
    pub fn new(window_size: usize) -> StreamProcessor {
        StreamProcessor {
            buffer: VecDeque::with_capacity(window_size),
            window_size,
            indicators: TechnicalIndicators::new(),
            processed_count: 0,
        }
    }

    /// 推送新数据点（增量处理）
    #[wasm_bindgen(js_name = pushData)]
    pub fn push_data(&mut self, data_js: &JsValue) -> Result<(), JsValue> {
        let data: MarketData = serde_wasm_bindgen::from_value(data_js.clone())
            .map_err(|e| JsValue::from_str(&format!("数据转换错误: {}", e)))?;

        // 如果缓冲区已满，移除最旧的数据
        if self.buffer.len() >= self.window_size {
            self.buffer.pop_front();
        }

        self.buffer.push_back(data);
        self.processed_count += 1;

        Ok(())
    }

    /// 批量推送数据
    #[wasm_bindgen(js_name = pushBatch)]
    pub fn push_batch(&mut self, data_array_js: &JsValue) -> Result<usize, JsValue> {
        let data_array: Vec<MarketData> = serde_wasm_bindgen::from_value(data_array_js.clone())
            .map_err(|e| JsValue::from_str(&format!("数据数组转换错误: {}", e)))?;

        let count = data_array.len();
        for data in data_array {
            if self.buffer.len() >= self.window_size {
                self.buffer.pop_front();
            }
            self.buffer.push_back(data);
            self.processed_count += 1;
        }

        Ok(count)
    }

    /// 计算当前窗口的技术指标
    #[wasm_bindgen(js_name = computeIndicators)]
    pub fn compute_indicators(&self) -> Result<JsValue, JsValue> {
        if self.buffer.is_empty() {
            return Err(JsValue::from_str("缓冲区为空"));
        }

        let prices: Vec<f64> = self.buffer.iter().map(|d| d.price).collect();

        // 计算多个指标
        let sma_20 = self.indicators.calculate_sma(&prices, 20.min(prices.len()));
        let ema_12 = self.indicators.calculate_ema(&prices, 12.min(prices.len()));
        let rsi_14 = self.indicators.calculate_rsi(&prices, 14.min(prices.len()));

        let result = serde_json::json!({
            "sma_20": sma_20.last().unwrap_or(&0.0),
            "ema_12": ema_12.last().unwrap_or(&0.0),
            "rsi_14": rsi_14.last().unwrap_or(&0.0),
            "buffer_size": self.buffer.len(),
            "processed_count": self.processed_count,
        });

        serde_wasm_bindgen::to_value(&result)
            .map_err(|e| JsValue::from_str(&format!("结果序列化错误: {}", e)))
    }

    /// 获取当前缓冲区大小
    #[wasm_bindgen(js_name = getBufferSize)]
    pub fn get_buffer_size(&self) -> usize {
        self.buffer.len()
    }

    /// 获取处理的数据总数
    #[wasm_bindgen(js_name = getProcessedCount)]
    pub fn get_processed_count(&self) -> usize {
        self.processed_count
    }

    /// 清空缓冲区
    #[wasm_bindgen(js_name = clearBuffer)]
    pub fn clear_buffer(&mut self) {
        self.buffer.clear();
        self.processed_count = 0;
    }

    /// 获取窗口内的最新价格
    #[wasm_bindgen(js_name = getLatestPrice)]
    pub fn get_latest_price(&self) -> Option<f64> {
        self.buffer.back().map(|d| d.price)
    }

    /// 获取窗口内的价格范围
    #[wasm_bindgen(js_name = getPriceRange)]
    pub fn get_price_range(&self) -> JsValue {
        if self.buffer.is_empty() {
            return JsValue::NULL;
        }

        let prices: Vec<f64> = self.buffer.iter().map(|d| d.price).collect();
        let min = prices.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        let result = serde_json::json!({
            "min": min,
            "max": max,
            "range": max - min,
        });

        serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
    }

    /// 计算窗口内的成交量统计
    #[wasm_bindgen(js_name = getVolumeStats)]
    pub fn get_volume_stats(&self) -> JsValue {
        if self.buffer.is_empty() {
            return JsValue::NULL;
        }

        let volumes: Vec<u64> = self.buffer.iter().map(|d| d.volume).collect();
        let total: u64 = volumes.iter().sum();
        let avg = total as f64 / volumes.len() as f64;

        let result = serde_json::json!({
            "total": total,
            "average": avg,
            "count": volumes.len(),
        });

        serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
    }
}

/// 批量流处理器（支持多股票同时处理）
#[wasm_bindgen]
pub struct BatchStreamProcessor {
    processors: std::collections::HashMap<String, StreamProcessor>,
    window_size: usize,
}

#[wasm_bindgen]
impl BatchStreamProcessor {
    /// 创建批量流处理器
    #[wasm_bindgen(constructor)]
    pub fn new(window_size: usize) -> BatchStreamProcessor {
        BatchStreamProcessor {
            processors: std::collections::HashMap::new(),
            window_size,
        }
    }

    /// 为指定股票推送数据
    #[wasm_bindgen(js_name = pushDataForSymbol)]
    pub fn push_data_for_symbol(
        &mut self,
        symbol: &str,
        data_js: &JsValue,
    ) -> Result<(), JsValue> {
        let processor = self
            .processors
            .entry(symbol.to_string())
            .or_insert_with(|| StreamProcessor::new(self.window_size));

        processor.push_data(data_js)
    }

    /// 获取指定股票的指标
    #[wasm_bindgen(js_name = getIndicatorsForSymbol)]
    pub fn get_indicators_for_symbol(&self, symbol: &str) -> Result<JsValue, JsValue> {
        let processor = self
            .processors
            .get(symbol)
            .ok_or_else(|| JsValue::from_str(&format!("未找到股票: {}", symbol)))?;

        processor.compute_indicators()
    }

    /// 获取所有股票的数量
    #[wasm_bindgen(js_name = getSymbolCount)]
    pub fn get_symbol_count(&self) -> usize {
        self.processors.len()
    }

    /// 清空指定股票的数据
    #[wasm_bindgen(js_name = clearSymbol)]
    pub fn clear_symbol(&mut self, symbol: &str) -> bool {
        self.processors.remove(symbol).is_some()
    }

    /// 清空所有数据
    #[wasm_bindgen(js_name = clearAll)]
    pub fn clear_all(&mut self) {
        self.processors.clear();
    }

    /// 获取所有股票代码列表
    #[wasm_bindgen(js_name = getSymbolList)]
    pub fn get_symbol_list(&self) -> js_sys::Array {
        let array = js_sys::Array::new();
        for symbol in self.processors.keys() {
            array.push(&JsValue::from_str(symbol));
        }
        array
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    #[cfg_attr(not(target_arch = "wasm32"), ignore = "requires wasm32 (js-sys/wasm-bindgen)")]
    fn test_stream_processor() {
        let mut processor = StreamProcessor::new(10);

        let data = MarketData::new("AAPL".to_string(), 150.0, 1000);
        let data_js = serde_wasm_bindgen::to_value(&data).unwrap();

        assert!(processor.push_data(&data_js).is_ok());
        assert_eq!(processor.get_buffer_size(), 1);
        assert_eq!(processor.get_processed_count(), 1);
    }

    #[test]
    #[cfg_attr(not(target_arch = "wasm32"), ignore = "requires wasm32 (js-sys/wasm-bindgen)")]
    fn test_window_overflow() {
        let mut processor = StreamProcessor::new(3);

        for i in 0..5 {
            let data = MarketData::new("AAPL".to_string(), 100.0 + i as f64, 1000);
            let data_js = serde_wasm_bindgen::to_value(&data).unwrap();
            processor.push_data(&data_js).unwrap();
        }

        // 窗口大小限制为3，所以只保留最后3个数据点
        assert_eq!(processor.get_buffer_size(), 3);
        assert_eq!(processor.get_processed_count(), 5);
    }
}
