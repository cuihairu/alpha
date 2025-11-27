//! Arrow 数据格式适配器
//!
//! 提供高性能零拷贝的列式数据处理能力

use alpha_core::models::MarketData;
use arrow_array::{
    Array, Float64Array, RecordBatch, StringArray, TimestampMillisecondArray, UInt64Array,
};
use arrow_schema::{DataType, Field, Schema};
use std::sync::Arc;
use wasm_bindgen::prelude::*;

/// Arrow 数据批次包装器
#[wasm_bindgen]
pub struct ArrowBatch {
    batch: RecordBatch,
}

impl ArrowBatch {
    /// 从市场数据创建 Arrow 批次
    pub fn from_market_data(data: &[MarketData]) -> Result<Self, JsValue> {
        if data.is_empty() {
            return Err(JsValue::from_str("市场数据不能为空"));
        }

        // 定义 Schema
        let schema = Arc::new(Schema::new(vec![
            Field::new("symbol", DataType::Utf8, false),
            Field::new("timestamp", DataType::Timestamp(arrow_schema::TimeUnit::Millisecond, None), false),
            Field::new("price", DataType::Float64, false),
            Field::new("volume", DataType::UInt64, false),
            Field::new("open", DataType::Float64, true),
            Field::new("high", DataType::Float64, true),
            Field::new("low", DataType::Float64, true),
            Field::new("bid", DataType::Float64, true),
            Field::new("ask", DataType::Float64, true),
        ]));

        // 提取列数据（零拷贝，直接使用引用）
        let symbols: Vec<&str> = data.iter().map(|d| d.symbol.as_str()).collect();
        let timestamps: Vec<i64> = data.iter().map(|d| d.timestamp.timestamp_millis()).collect();
        let prices: Vec<f64> = data.iter().map(|d| d.price).collect();
        let volumes: Vec<u64> = data.iter().map(|d| d.volume).collect();

        // Optional fields
        let opens: Vec<Option<f64>> = data.iter().map(|d| d.open).collect();
        let highs: Vec<Option<f64>> = data.iter().map(|d| d.high).collect();
        let lows: Vec<Option<f64>> = data.iter().map(|d| d.low).collect();
        let bids: Vec<Option<f64>> = data.iter().map(|d| d.bid).collect();
        let asks: Vec<Option<f64>> = data.iter().map(|d| d.ask).collect();

        // 构建 Arrow 数组
        let symbol_array = Arc::new(StringArray::from(symbols)) as Arc<dyn Array>;
        let timestamp_array = Arc::new(TimestampMillisecondArray::from(timestamps)) as Arc<dyn Array>;
        let price_array = Arc::new(Float64Array::from(prices)) as Arc<dyn Array>;
        let volume_array = Arc::new(UInt64Array::from(volumes)) as Arc<dyn Array>;
        let open_array = Arc::new(Float64Array::from(opens)) as Arc<dyn Array>;
        let high_array = Arc::new(Float64Array::from(highs)) as Arc<dyn Array>;
        let low_array = Arc::new(Float64Array::from(lows)) as Arc<dyn Array>;
        let bid_array = Arc::new(Float64Array::from(bids)) as Arc<dyn Array>;
        let ask_array = Arc::new(Float64Array::from(asks)) as Arc<dyn Array>;

        // 创建 RecordBatch
        let batch = RecordBatch::try_new(
            schema,
            vec![
                symbol_array,
                timestamp_array,
                price_array,
                volume_array,
                open_array,
                high_array,
                low_array,
                bid_array,
                ask_array,
            ],
        )
        .map_err(|e| JsValue::from_str(&format!("创建 Arrow 批次失败: {}", e)))?;

        Ok(Self { batch })
    }

    /// 获取价格列（零拷贝访问）
    pub fn get_prices(&self) -> Result<Vec<f64>, JsValue> {
        let column = self.batch.column(2);
        let price_array = column
            .as_any()
            .downcast_ref::<Float64Array>()
            .ok_or_else(|| JsValue::from_str("价格列类型错误"))?;

        Ok(price_array.values().to_vec())
    }

    /// 获取成交量列（零拷贝访问）
    pub fn get_volumes(&self) -> Result<Vec<u64>, JsValue> {
        let column = self.batch.column(3);
        let volume_array = column
            .as_any()
            .downcast_ref::<UInt64Array>()
            .ok_or_else(|| JsValue::from_str("成交量列类型错误"))?;

        Ok(volume_array.values().to_vec())
    }

    /// 获取批次行数
    pub fn row_count(&self) -> usize {
        self.batch.num_rows()
    }

    /// 获取列数
    pub fn column_count(&self) -> usize {
        self.batch.num_columns()
    }
}

#[wasm_bindgen]
impl ArrowBatch {
    /// 获取批次大小（字节）
    #[wasm_bindgen(js_name = getByteSize)]
    pub fn get_byte_size(&self) -> usize {
        self.batch.get_array_memory_size()
    }

    /// 获取行数
    #[wasm_bindgen(js_name = numRows)]
    pub fn num_rows(&self) -> usize {
        self.row_count()
    }

    /// 获取列数
    #[wasm_bindgen(js_name = numColumns)]
    pub fn num_columns(&self) -> usize {
        self.column_count()
    }

    /// 导出价格数据到 JavaScript
    #[wasm_bindgen(js_name = exportPrices)]
    pub fn export_prices(&self) -> Result<js_sys::Float64Array, JsValue> {
        let prices = self.get_prices()?;
        Ok(js_sys::Float64Array::from(&prices[..]))
    }

    /// 导出成交量数据到 JavaScript
    #[wasm_bindgen(js_name = exportVolumes)]
    pub fn export_volumes(&self) -> Result<js_sys::BigUint64Array, JsValue> {
        let volumes = self.get_volumes()?;
        let js_array = js_sys::BigUint64Array::new_with_length(volumes.len() as u32);
        for (i, &vol) in volumes.iter().enumerate() {
            js_array.set_index(i as u32, vol);
        }
        Ok(js_array)
    }
}

/// Arrow 内存池管理器
pub struct ArrowMemoryPool {
    batches: Vec<ArrowBatch>,
    max_batches: usize,
}

impl ArrowMemoryPool {
    /// 创建新的内存池
    pub fn new(max_batches: usize) -> Self {
        Self {
            batches: Vec::with_capacity(max_batches),
            max_batches,
        }
    }

    /// 添加批次到内存池
    pub fn add_batch(&mut self, batch: ArrowBatch) -> Result<(), String> {
        if self.batches.len() >= self.max_batches {
            return Err(format!("内存池已满，最大批次数: {}", self.max_batches));
        }
        self.batches.push(batch);
        Ok(())
    }

    /// 清空内存池
    pub fn clear(&mut self) {
        self.batches.clear();
    }

    /// 获取当前批次数
    pub fn batch_count(&self) -> usize {
        self.batches.len()
    }

    /// 获取总内存使用量（字节）
    pub fn total_memory_usage(&self) -> usize {
        self.batches.iter().map(|b| b.get_byte_size()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alpha_core::models::MarketData;
    use chrono::Utc;

    #[test]
    fn test_arrow_batch_creation() {
        let data = vec![
            MarketData::new("AAPL".to_string(), 150.0, 1000),
            MarketData::new("AAPL".to_string(), 151.0, 1100),
            MarketData::new("AAPL".to_string(), 152.0, 1200),
        ];

        let batch = ArrowBatch::from_market_data(&data).unwrap();
        assert_eq!(batch.row_count(), 3);
        assert_eq!(batch.column_count(), 9);
    }

    #[test]
    fn test_zero_copy_access() {
        let data = vec![
            MarketData::new("AAPL".to_string(), 100.0, 1000),
            MarketData::new("AAPL".to_string(), 101.0, 1100),
        ];

        let batch = ArrowBatch::from_market_data(&data).unwrap();
        let prices = batch.get_prices().unwrap();
        assert_eq!(prices, vec![100.0, 101.0]);

        let volumes = batch.get_volumes().unwrap();
        assert_eq!(volumes, vec![1000, 1100]);
    }

    #[test]
    fn test_memory_pool() {
        let mut pool = ArrowMemoryPool::new(10);

        let data = vec![MarketData::new("AAPL".to_string(), 150.0, 1000)];
        let batch = ArrowBatch::from_market_data(&data).unwrap();

        assert!(pool.add_batch(batch).is_ok());
        assert_eq!(pool.batch_count(), 1);

        pool.clear();
        assert_eq!(pool.batch_count(), 0);
    }
}
