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
    pub fn push_data_for_symbol(&mut self, symbol: &str, data_js: &JsValue) -> Result<(), JsValue> {
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

    /// 并行计算所有股票的指标（原生走 Rayon，wasm32 顺序执行，JS 侧可用 WorkerPool 派发真并行）
    #[wasm_bindgen(js_name = computeAllIndicatorsParallel)]
    pub fn compute_all_indicators_parallel(&self) -> Result<JsValue, JsValue> {
        use alpha_core::parallel::{compute, IndicatorKind};

        // 收集所有股票的价格序列
        let mut symbols = Vec::new();
        let mut datasets = Vec::new();
        for (symbol, processor) in &self.processors {
            let prices: Vec<f64> = processor.buffer.iter().map(|d| d.price).collect();
            if !prices.is_empty() {
                symbols.push(symbol.clone());
                datasets.push(prices);
            }
        }

        if datasets.is_empty() {
            return Ok(serde_wasm_bindgen::to_value(&serde_json::json!({"results": {}})).unwrap());
        }

        // 并行计算 SMA/EMA/RSI（复用 alpha_core::parallel，native 走 Rayon，wasm32 顺序）
        let sma_results = compute(&datasets, IndicatorKind::Sma, 20);
        let ema_results = compute(&datasets, IndicatorKind::Ema, 12);
        let rsi_results = compute(&datasets, IndicatorKind::Rsi, 14);

        // 组装结果
        let mut results_map = serde_json::Map::new();
        for (i, symbol) in symbols.iter().enumerate() {
            let obj = serde_json::json!({
                "sma_20": sma_results[i].last().unwrap_or(&0.0),
                "ema_12": ema_results[i].last().unwrap_or(&0.0),
                "rsi_14": rsi_results[i].last().unwrap_or(&0.0),
                "buffer_size": datasets[i].len(),
            });
            results_map.insert(symbol.clone(), obj);
        }

        serde_wasm_bindgen::to_value(&serde_json::json!({ "results": results_map }))
            .map_err(|e| JsValue::from_str(&format!("结果序列化错误: {}", e)))
    }
}

/// 并行流处理器（多数据集并行计算，原生 Rayon / wasm32 WorkerPool 双路径）
#[wasm_bindgen]
pub struct ParallelStreamProcessor {
    /// 每个标的的价格缓冲区
    buffers: std::collections::HashMap<String, VecDeque<f64>>,
    /// 窗口大小
    window_size: usize,
    /// Worker 池（wasm32 路径用，native 为 None）
    #[cfg(target_arch = "wasm32")]
    worker_pool: Option<crate::worker::WorkerPool>,
    #[cfg(not(target_arch = "wasm32"))]
    _phantom: std::marker::PhantomData<()>,
}

#[wasm_bindgen]
impl ParallelStreamProcessor {
    /// 创建并行流处理器
    #[wasm_bindgen(constructor)]
    pub fn new(window_size: usize) -> ParallelStreamProcessor {
        ParallelStreamProcessor {
            buffers: std::collections::HashMap::new(),
            window_size,
            #[cfg(target_arch = "wasm32")]
            worker_pool: None,
            #[cfg(not(target_arch = "wasm32"))]
            _phantom: std::marker::PhantomData,
        }
    }

    /// 设置 Worker 池（wasm32 路径：启用 Web Worker 真并行）
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen(js_name = setWorkerPool)]
    pub fn set_worker_pool(&mut self, pool: crate::worker::WorkerPool) {
        self.worker_pool = Some(pool);
    }

    /// 推送单标的数据点
    #[wasm_bindgen(js_name = pushData)]
    pub fn push_data(&mut self, symbol: &str, price: f64) {
        let buf = self
            .buffers
            .entry(symbol.to_string())
            .or_insert_with(|| VecDeque::with_capacity(self.window_size));
        if buf.len() >= self.window_size {
            buf.pop_front();
        }
        buf.push_back(price);
    }

    /// 批量推送单标的数据
    #[wasm_bindgen(js_name = pushBatch)]
    pub fn push_batch(&mut self, symbol: &str, prices_js: &js_sys::Float64Array) {
        let prices: Vec<f64> = prices_js.to_vec();
        let buf = self
            .buffers
            .entry(symbol.to_string())
            .or_insert_with(|| VecDeque::with_capacity(self.window_size));
        for price in prices {
            if buf.len() >= self.window_size {
                buf.pop_front();
            }
            buf.push_back(price);
        }
    }

    /// 并行计算所有标的的指标（返回每标的最新 SMA/EMA/RSI）
    #[wasm_bindgen(js_name = computeIndicatorsParallel)]
    pub fn compute_indicators_parallel(&self) -> Result<JsValue, JsValue> {
        use alpha_core::parallel::{compute, IndicatorKind};

        // 收集非空缓冲区
        let mut symbols = Vec::new();
        let mut datasets = Vec::new();
        for (symbol, buf) in &self.buffers {
            if !buf.is_empty() {
                symbols.push(symbol.clone());
                datasets.push(buf.iter().cloned().collect::<Vec<_>>());
            }
        }

        if datasets.is_empty() {
            return Ok(serde_wasm_bindgen::to_value(&serde_json::json!({"results": {}})).unwrap());
        }

        // 并行计算（native Rayon / wasm32 顺序，语义等价）
        let sma_results = compute(&datasets, IndicatorKind::Sma, 20);
        let ema_results = compute(&datasets, IndicatorKind::Ema, 12);
        let rsi_results = compute(&datasets, IndicatorKind::Rsi, 14);

        // 组装结果
        let mut results_map = serde_json::Map::new();
        for (i, symbol) in symbols.iter().enumerate() {
            let obj = serde_json::json!({
                "sma_20": sma_results[i].last().unwrap_or(&0.0),
                "ema_12": ema_results[i].last().unwrap_or(&0.0),
                "rsi_14": rsi_results[i].last().unwrap_or(&0.0),
                "buffer_size": datasets[i].len(),
            });
            results_map.insert(symbol.clone(), obj);
        }

        serde_wasm_bindgen::to_value(&serde_json::json!({ "results": results_map }))
            .map_err(|e| JsValue::from_str(&format!("结果序列化错误: {}", e)))
    }

    /// 带报告的并行计算（含耗时、并行度、数据集数）
    #[wasm_bindgen(js_name = computeIndicatorsParallelWithReport)]
    pub fn compute_indicators_parallel_with_report(&self) -> Result<JsValue, JsValue> {
        use alpha_core::parallel::{compute_with_report, IndicatorKind};

        let mut symbols = Vec::new();
        let mut datasets = Vec::new();
        for (symbol, buf) in &self.buffers {
            if !buf.is_empty() {
                symbols.push(symbol.clone());
                datasets.push(buf.iter().cloned().collect::<Vec<_>>());
            }
        }

        if datasets.is_empty() {
            return Ok(serde_wasm_bindgen::to_value(&serde_json::json!({"results": {}})).unwrap());
        }

        let start = crate::worker::now_ms();
        let sma_report = compute_with_report(&datasets, IndicatorKind::Sma, 20, 0.0);
        let ema_report = compute_with_report(&datasets, IndicatorKind::Ema, 12, 0.0);
        let rsi_report = compute_with_report(&datasets, IndicatorKind::Rsi, 14, 0.0);
        let elapsed = crate::worker::now_ms() - start;

        let mut results_map = serde_json::Map::new();
        for (i, symbol) in symbols.iter().enumerate() {
            let obj = serde_json::json!({
                "sma_20": sma_report.results[i].last().unwrap_or(&0.0),
                "ema_12": ema_report.results[i].last().unwrap_or(&0.0),
                "rsi_14": rsi_report.results[i].last().unwrap_or(&0.0),
                "buffer_size": datasets[i].len(),
            });
            results_map.insert(symbol.clone(), obj);
        }

        let report = serde_json::json!({
            "results": results_map,
            "dataset_count": datasets.len(),
            "threads": sma_report.threads,
            "elapsed_ms": elapsed,
        });

        serde_wasm_bindgen::to_value(&report)
            .map_err(|e| JsValue::from_str(&format!("结果序列化错误: {}", e)))
    }

    /// 获取标的数量
    #[wasm_bindgen(js_name = getSymbolCount)]
    pub fn get_symbol_count(&self) -> usize {
        self.buffers.len()
    }

    /// 清空指定标的
    #[wasm_bindgen(js_name = clearSymbol)]
    pub fn clear_symbol(&mut self, symbol: &str) -> bool {
        self.buffers.remove(symbol).is_some()
    }

    /// 清空所有
    #[wasm_bindgen(js_name = clearAll)]
    pub fn clear_all(&mut self) {
        self.buffers.clear();
    }

    /// 获取标的列表
    #[wasm_bindgen(js_name = getSymbolList)]
    pub fn get_symbol_list(&self) -> js_sys::Array {
        let array = js_sys::Array::new();
        for symbol in self.buffers.keys() {
            array.push(&JsValue::from_str(symbol));
        }
        array
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(
        not(target_arch = "wasm32"),
        ignore = "requires wasm32 (js-sys/wasm-bindgen)"
    )]
    fn test_stream_processor() {
        let mut processor = StreamProcessor::new(10);

        let data = MarketData::new("AAPL".to_string(), 150.0, 1000);
        let data_js = serde_wasm_bindgen::to_value(&data).unwrap();

        assert!(processor.push_data(&data_js).is_ok());
        assert_eq!(processor.get_buffer_size(), 1);
        assert_eq!(processor.get_processed_count(), 1);
    }

    #[test]
    #[cfg_attr(
        not(target_arch = "wasm32"),
        ignore = "requires wasm32 (js-sys/wasm-bindgen)"
    )]
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

    /// 批量流处理器并行计算（native 路径）
    #[test]
    #[cfg(target_arch = "wasm32")]
    fn test_batch_stream_processor_parallel_compute() {
        let mut batch = BatchStreamProcessor::new(50);
        for symbol in ["AAPL", "GOOGL", "MSFT"] {
            for i in 0..60 {
                let data = MarketData::new(symbol.to_string(), 100.0 + i as f64 * 0.5, 1000);
                let data_js = serde_wasm_bindgen::to_value(&data).unwrap();
                batch.push_data_for_symbol(symbol, &data_js).unwrap();
            }
        }
        assert_eq!(batch.get_symbol_count(), 3);

        let result = batch
            .compute_all_indicators_parallel()
            .expect("并行计算应成功");
        let result_val: serde_json::Value = serde_wasm_bindgen::from_value(result).unwrap();
        let results = result_val["results"].as_object().expect("results 应为对象");
        assert_eq!(results.len(), 3, "应有 3 个股票的结果");
        for symbol in ["AAPL", "GOOGL", "MSFT"] {
            let obj = results[symbol].as_object().expect("每标的结果应为对象");
            assert!(obj["sma_20"].is_number(), "{symbol} 应含 sma_20");
            assert!(obj["ema_12"].is_number(), "{symbol} 应含 ema_12");
            assert!(obj["rsi_14"].is_number(), "{symbol} 应含 rsi_14");
            assert_eq!(
                obj["buffer_size"].as_u64().unwrap(),
                50,
                "{symbol} 窗口应为 50"
            );
        }
    }

    /// 并行流处理器：基本推送与并行计算
    #[test]
    #[cfg(target_arch = "wasm32")]
    fn test_parallel_stream_processor_basic() {
        let mut processor = ParallelStreamProcessor::new(30);
        for i in 0..40 {
            processor.push_data("SYM1", 100.0 + i as f64 * 0.3);
            processor.push_data("SYM2", 200.0 + i as f64 * 0.2);
        }
        assert_eq!(processor.get_symbol_count(), 2);

        let result = processor
            .compute_indicators_parallel()
            .expect("并行计算应成功");
        let result_val: serde_json::Value = serde_wasm_bindgen::from_value(result).unwrap();
        let results = result_val["results"].as_object().expect("results 应为对象");
        assert_eq!(results.len(), 2);
        for symbol in ["SYM1", "SYM2"] {
            let obj = results[symbol].as_object().expect("每标的结果应为对象");
            assert!(obj["sma_20"].is_number(), "{symbol} 应含 sma_20");
            assert!(obj["ema_12"].is_number(), "{symbol} 应含 ema_12");
            assert!(obj["rsi_14"].is_number(), "{symbol} 应含 rsi_14");
            assert_eq!(obj["buffer_size"].as_u64().unwrap(), 30);
        }
    }

    /// 并行流处理器：带报告的并行计算
    #[test]
    #[cfg(target_arch = "wasm32")]
    fn test_parallel_stream_processor_with_report() {
        let mut processor = ParallelStreamProcessor::new(25);
        for i in 0..35 {
            processor.push_data("TEST", 150.0 + i as f64 * 0.4);
        }

        let result = processor
            .compute_indicators_parallel_with_report()
            .expect("带报告计算应成功");
        let report: serde_json::Value = serde_wasm_bindgen::from_value(result).unwrap();
        assert_eq!(report["dataset_count"], 1);
        assert!(report["threads"].as_u64().unwrap() >= 1);
        assert!(report["elapsed_ms"].as_f64().unwrap() >= 0.0);
        let results = report["results"].as_object().unwrap();
        assert!(results["TEST"]["sma_20"].is_number());
        assert!(results["TEST"]["ema_12"].is_number());
        assert!(results["TEST"]["rsi_14"].is_number());
    }

    /// 并行流处理器：空缓冲区返回空结果
    #[test]
    #[cfg(target_arch = "wasm32")]
    fn test_parallel_stream_processor_empty() {
        let processor = ParallelStreamProcessor::new(10);
        let result = processor
            .compute_indicators_parallel()
            .expect("空计算应成功");
        let result_val: serde_json::Value = serde_wasm_bindgen::from_value(result).unwrap();
        assert_eq!(result_val["results"], serde_json::json!({}));
    }

    /// 批量推送 Float64Array
    #[test]
    #[cfg(target_arch = "wasm32")]
    fn test_parallel_stream_processor_push_batch() {
        let mut processor = ParallelStreamProcessor::new(20);
        let prices =
            js_sys::Float64Array::from(&(0..25).map(|i| 100.0 + i as f64).collect::<Vec<_>>()[..]);
        processor.push_batch("BATCH_SYM", &prices);
        assert_eq!(processor.get_symbol_count(), 1);

        let result = processor
            .compute_indicators_parallel()
            .expect("批量推送后计算应成功");
        let result_val: serde_json::Value = serde_wasm_bindgen::from_value(result).unwrap();
        let results = result_val["results"].as_object().unwrap();
        assert_eq!(results["BATCH_SYM"]["buffer_size"].as_u64().unwrap(), 20);
    }

    /// 标的列表与清空
    #[test]
    #[cfg(target_arch = "wasm32")]
    fn test_parallel_stream_processor_symbol_list_and_clear() {
        let mut processor = ParallelStreamProcessor::new(10);
        processor.push_data("A", 1.0);
        processor.push_data("B", 2.0);
        processor.push_data("C", 3.0);

        let list = processor.get_symbol_list();
        assert_eq!(list.length(), 3);

        assert!(processor.clear_symbol("B"));
        assert_eq!(processor.get_symbol_count(), 2);
        let list2 = processor.get_symbol_list();
        assert_eq!(list2.length(), 2);

        processor.clear_all();
        assert_eq!(processor.get_symbol_count(), 0);
        assert!(processor.get_symbol_list().length() == 0);
    }
}
