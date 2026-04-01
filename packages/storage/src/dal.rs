//! 数据访问层 (Data Access Layer)

use crate::{
    MemoryStorage, StorageBackend, StorageBackendType, StorageConfig, StorageFactory,
    TimeSeriesStats, TimeSeriesStorage,
};
use alpha_core::{
    errors::{AlphaError, AlphaResult},
    models::MarketData,
};
use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// 统一数据访问层
pub struct DataAccessLayer {
    timeseries: TimeSeriesStorage,
    metadata: Box<dyn StorageBackend>,
    cache: MemoryStorage,
}

impl DataAccessLayer {
    /// 创建新的数据访问层
    pub async fn new(config: DataAccessConfig) -> AlphaResult<Self> {
        let timeseries = TimeSeriesStorage::new();
        let metadata = StorageFactory::create(config.metadata_storage.clone()).await?;
        let cache = MemoryStorage::new();

        Ok(Self {
            timeseries,
            metadata,
            cache,
        })
    }

    /// 存储市场数据
    pub async fn store_market_data(&self, data: &MarketData) -> AlphaResult<()> {
        self.timeseries.add_market_data(data).await?;

        let cache_key = format!("latest_price:{}", data.symbol);
        self.cache
            .store(&cache_key, serialize_value(&data.price)?)
            .await?;

        Ok(())
    }

    /// 批量存储市场数据
    pub async fn store_market_data_batch(&self, data_list: &[MarketData]) -> AlphaResult<()> {
        self.timeseries.add_market_data_batch(data_list).await?;

        for data in data_list {
            let cache_key = format!("latest_price:{}", data.symbol);
            self.cache
                .store(&cache_key, serialize_value(&data.price)?)
                .await?;
        }

        Ok(())
    }

    /// 获取最新价格（优先从缓存读取）
    pub async fn get_latest_price(&self, symbol: &str) -> AlphaResult<Option<f64>> {
        let cache_key = format!("latest_price:{}", symbol);

        if let Some(bytes) = self.cache.retrieve(&cache_key).await? {
            return deserialize_value(&bytes).map(Some);
        }

        let price = self.timeseries.get_latest_price(symbol).await?;

        if let Some(p) = price {
            self.cache
                .store(&cache_key, serialize_value(&p)?)
                .await?;
        }

        Ok(price)
    }

    /// 获取时间范围内的市场数据
    pub async fn get_market_data_range(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> AlphaResult<Vec<MarketData>> {
        let points = self.timeseries.get_data_in_range(symbol, start, end).await?;

        let mut market_data = Vec::new();
        for point in points {
            let metadata = point.metadata.unwrap_or(serde_json::Value::Null);

            market_data.push(MarketData {
                symbol: symbol.to_string(),
                timestamp: point.timestamp,
                price: point.value,
                volume: point.volume.unwrap_or(0),
                bid: metadata.get("bid").and_then(|v| v.as_f64()),
                ask: metadata.get("ask").and_then(|v| v.as_f64()),
                open: metadata.get("open").and_then(|v| v.as_f64()),
                high: metadata.get("high").and_then(|v| v.as_f64()),
                low: metadata.get("low").and_then(|v| v.as_f64()),
            });
        }

        Ok(market_data)
    }

    /// 存储元数据
    pub async fn store_metadata<T>(&self, key: &str, metadata: &T) -> AlphaResult<()>
    where
        T: Serialize,
    {
        self.metadata.store(key, serialize_value(metadata)?).await
    }

    /// 获取元数据
    pub async fn get_metadata<T>(&self, key: &str) -> AlphaResult<Option<T>>
    where
        T: DeserializeOwned,
    {
        match self.metadata.retrieve(key).await? {
            Some(bytes) => deserialize_value(&bytes).map(Some),
            None => Ok(None),
        }
    }

    /// 列出所有支持的符号
    pub async fn list_symbols(&self) -> AlphaResult<Vec<String>> {
        self.timeseries.list_symbols().await
    }

    /// 删除符号的所有数据
    pub async fn delete_symbol(&self, symbol: &str) -> AlphaResult<()> {
        self.timeseries.delete_symbol(symbol).await?;

        let cache_key = format!("latest_price:{}", symbol);
        self.cache.delete(&cache_key).await?;

        Ok(())
    }

    /// 获取存储统计信息
    pub async fn get_storage_statistics(&self) -> AlphaResult<StorageStatistics> {
        let timeseries_stats = self.timeseries.get_statistics().await?;

        Ok(StorageStatistics {
            total_symbols: timeseries_stats.total_symbols,
            total_data_points: timeseries_stats.total_points,
            storage_size_estimate: timeseries_stats.total_points * 64,
            timeseries_stats,
        })
    }

    /// 清理过期数据
    pub async fn cleanup_old_data(&self, _before: DateTime<Utc>) -> AlphaResult<usize> {
        Ok(0)
    }

    /// 导出数据
    pub async fn export_data(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> AlphaResult<Vec<u8>> {
        let data = self.get_market_data_range(symbol, start, end).await?;
        serde_json::to_vec(&data)
            .map_err(|e| AlphaError::SerializationError(e.to_string()))
    }

    /// 导入数据
    pub async fn import_data(&self, data: &[u8]) -> AlphaResult<usize> {
        let market_data: Vec<MarketData> = serde_json::from_slice(data)
            .map_err(|e| AlphaError::SerializationError(e.to_string()))?;

        self.store_market_data_batch(&market_data).await?;
        Ok(market_data.len())
    }
}

/// 数据访问层配置
#[derive(Debug, Clone)]
pub struct DataAccessConfig {
    pub metadata_storage: StorageConfig,
    pub cache_ttl_seconds: Option<u64>,
    pub max_cache_size: usize,
}

impl Default for DataAccessConfig {
    fn default() -> Self {
        Self {
            metadata_storage: StorageConfig {
                backend: StorageBackendType::Memory,
                connection_string: "memory://".to_string(),
                ttl_seconds: None,
                max_connections: None,
            },
            cache_ttl_seconds: Some(3600),
            max_cache_size: 10000,
        }
    }
}

/// 存储统计信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStatistics {
    pub total_symbols: usize,
    pub total_data_points: usize,
    pub storage_size_estimate: usize,
    pub timeseries_stats: TimeSeriesStats,
}

/// 数据查询构建器
#[derive(Debug)]
pub struct QueryBuilder {
    symbol: Option<String>,
    start_time: Option<DateTime<Utc>>,
    end_time: Option<DateTime<Utc>>,
    limit: Option<usize>,
    resample_interval: Option<i64>,
}

impl QueryBuilder {
    pub fn new() -> Self {
        Self {
            symbol: None,
            start_time: None,
            end_time: None,
            limit: None,
            resample_interval: None,
        }
    }

    pub fn symbol(mut self, symbol: impl Into<String>) -> Self {
        self.symbol = Some(symbol.into());
        self
    }

    pub fn start_time(mut self, time: DateTime<Utc>) -> Self {
        self.start_time = Some(time);
        self
    }

    pub fn end_time(mut self, time: DateTime<Utc>) -> Self {
        self.end_time = Some(time);
        self
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn resample(mut self, interval_seconds: i64) -> Self {
        self.resample_interval = Some(interval_seconds);
        self
    }

    pub async fn execute(&self, dal: &DataAccessLayer) -> AlphaResult<Vec<MarketData>> {
        let symbol = self
            .symbol
            .as_ref()
            .ok_or_else(|| AlphaError::InvalidInput("查询必须指定符号".to_string()))?;

        let start =
            self.start_time
                .unwrap_or_else(|| Utc::now() - chrono::Duration::days(30));
        let end = self.end_time.unwrap_or_else(Utc::now);

        let mut data = dal.get_market_data_range(symbol, start, end).await?;

        if let Some(interval) = self.resample_interval {
            data = resample_market_data(data, interval);
        }

        if let Some(limit) = self.limit {
            data.truncate(limit);
        }

        Ok(data)
    }
}

impl Default for QueryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

fn serialize_value<T: Serialize>(value: &T) -> AlphaResult<Vec<u8>> {
    bincode::serialize(value).map_err(|e| AlphaError::SerializationError(e.to_string()))
}

fn deserialize_value<T: DeserializeOwned>(bytes: &[u8]) -> AlphaResult<T> {
    bincode::deserialize(bytes).map_err(|e| AlphaError::SerializationError(e.to_string()))
}

fn resample_market_data(data: Vec<MarketData>, interval_seconds: i64) -> Vec<MarketData> {
    if data.is_empty() || interval_seconds <= 0 {
        return data;
    }

    let mut resampled = Vec::new();
    let mut bucket_start = data[0].timestamp;
    let mut bucket = Vec::new();

    for item in data {
        if item.timestamp < bucket_start + chrono::Duration::seconds(interval_seconds) {
            bucket.push(item);
            continue;
        }

        if let Some(aggregated) = aggregate_bucket(&bucket, bucket_start) {
            resampled.push(aggregated);
        }

        bucket_start = item.timestamp;
        bucket.clear();
        bucket.push(item);
    }

    if let Some(aggregated) = aggregate_bucket(&bucket, bucket_start) {
        resampled.push(aggregated);
    }

    resampled
}

fn aggregate_bucket(bucket: &[MarketData], timestamp: DateTime<Utc>) -> Option<MarketData> {
    let first = bucket.first()?;
    let last = bucket.last()?;

    let mut high = first.high.unwrap_or(first.price);
    let mut low = first.low.unwrap_or(first.price);
    let mut volume = 0u64;

    for item in bucket {
        high = high.max(item.high.unwrap_or(item.price));
        low = low.min(item.low.unwrap_or(item.price));
        volume = volume.saturating_add(item.volume);
    }

    Some(MarketData {
        symbol: first.symbol.clone(),
        timestamp,
        price: last.price,
        volume,
        bid: last.bid,
        ask: last.ask,
        open: first.open.or(Some(first.price)),
        high: Some(high),
        low: Some(low),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_data_access_layer() {
        let config = DataAccessConfig::default();
        let dal = DataAccessLayer::new(config).await.unwrap();

        let data = MarketData {
            symbol: "AAPL".to_string(),
            timestamp: Utc::now(),
            price: 150.0,
            volume: 1000,
            bid: Some(149.5),
            ask: Some(150.5),
            open: Some(149.0),
            high: Some(151.0),
            low: Some(148.5),
        };

        dal.store_market_data(&data).await.unwrap();

        let latest_price = dal.get_latest_price("AAPL").await.unwrap();
        assert_eq!(latest_price, Some(150.0));

        let symbols = dal.list_symbols().await.unwrap();
        assert!(symbols.contains(&"AAPL".to_string()));

        let stats = dal.get_storage_statistics().await.unwrap();
        assert_eq!(stats.total_symbols, 1);
        assert_eq!(stats.total_data_points, 1);
    }

    #[tokio::test]
    async fn test_query_builder_limit_and_resample() {
        let config = DataAccessConfig::default();
        let dal = DataAccessLayer::new(config).await.unwrap();

        let base_time = Utc::now();
        for i in 0..10 {
            let data = MarketData {
                symbol: "AAPL".to_string(),
                timestamp: base_time + chrono::Duration::minutes(i),
                price: 150.0 + i as f64,
                volume: 1000,
                bid: Some(149.5 + i as f64),
                ask: Some(150.5 + i as f64),
                open: Some(149.0 + i as f64),
                high: Some(151.0 + i as f64),
                low: Some(148.5 + i as f64),
            };
            dal.store_market_data(&data).await.unwrap();
        }

        let result = QueryBuilder::new()
            .symbol("AAPL")
            .start_time(base_time)
            .end_time(base_time + chrono::Duration::minutes(9))
            .resample(300)
            .limit(2)
            .execute(&dal)
            .await
            .unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].volume, 5000);
        assert_eq!(result[1].volume, 5000);
    }

    #[tokio::test]
    async fn test_metadata_operations() {
        let config = DataAccessConfig::default();
        let dal = DataAccessLayer::new(config).await.unwrap();

        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct TestMetadata {
            description: String,
            category: String,
        }

        let metadata = TestMetadata {
            description: "Apple Inc.".to_string(),
            category: "Technology".to_string(),
        };

        dal.store_metadata("AAPL:info", &metadata).await.unwrap();

        let retrieved: Option<TestMetadata> = dal.get_metadata("AAPL:info").await.unwrap();
        assert_eq!(retrieved, Some(metadata));
    }
}
