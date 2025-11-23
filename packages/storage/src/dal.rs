//! 数据访问层 (Data Access Layer)

use super::{
    memory::MemoryStorage,
    timescale::TimescaleTimeSeriesStorage,
    timeseries::TimeSeriesStorage,
    StorageBackend, StorageBackendType,
};
use alpha_core::errors::{AlphaResult, AlphaError};
use alpha_core::models::MarketData;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// 统一数据访问层
#[derive(Debug)]
pub struct DataAccessLayer {
    timeseries: Arc<TimeSeriesStorage>,
    metadata: Arc<dyn StorageBackend<Error = Box<dyn std::error::Error + Send + Sync>>>,
    cache: Arc<MemoryStorage>,
}

impl DataAccessLayer {
    /// 创建新的数据访问层
    pub async fn new(config: DataAccessConfig) -> AlphaResult<Self> {
        // 创建时间序列存储
        let timeseries = Arc::new(TimeSeriesStorage::new());

        // 创建元数据存储
        let metadata = match config.metadata_storage {
            StorageBackendType::Memory => {
                Arc::new(MemoryStorage::new()) as Arc<dyn StorageBackend<Error = Box<dyn std::error::Error + Send + Sync>>>
            }
            StorageBackendType::LocalDisk => {
                // 磁盘存储需要实现 StorageBackend trait
                // 暂时使用内存存储替代
                Arc::new(MemoryStorage::new()) as Arc<dyn StorageBackend<Error = Box<dyn std::error::Error + Send + Sync>>>
            }
            _ => {
                return Err(AlphaError::InternalError("不支持的元数据存储类型".to_string()));
            }
        };

        // 创建缓存存储
        let cache = Arc::new(MemoryStorage::new());

        Ok(Self {
            timeseries,
            metadata,
            cache,
        })
    }

    /// 存储市场数据
    pub async fn store_market_data(&self, data: &MarketData) -> AlphaResult<()> {
        // 存储到时间序列存储
        self.timeseries.add_market_data(data).await?;

        // 更新缓存中的最新价格
        let cache_key = format!("latest_price:{}", data.symbol);
        self.cache.store(&cache_key, &data.price).await?;

        Ok(())
    }

    /// 批量存储市场数据
    pub async fn store_market_data_batch(&self, data_list: &[MarketData]) -> AlphaResult<()> {
        self.timeseries.add_market_data_batch(data_list).await?;

        // 更新缓存
        for data in data_list {
            let cache_key = format!("latest_price:{}", data.symbol);
            self.cache.store(&cache_key, &data.price).await?;
        }

        Ok(())
    }

    /// 获取最新价格（优先从缓存读取）
    pub async fn get_latest_price(&self, symbol: &str) -> AlphaResult<Option<f64>> {
        let cache_key = format!("latest_price:{}", symbol);

        // 先尝试从缓存读取
        if let Some(price) = self.cache.retrieve(&cache_key).await? {
            return Ok(Some(price));
        }

        // 缓存未命中，从时间序列存储读取
        let price = self.timeseries.get_latest_price(symbol).await?;

        // 更新缓存
        if let Some(p) = price {
            self.cache.store(&cache_key, &p).await?;
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

            let data = MarketData {
                symbol: symbol.to_string(),
                timestamp: point.timestamp,
                price: point.value,
                volume: point.volume.unwrap_or(0),
                bid: metadata.get("bid").and_then(|v| v.as_f64()),
                ask: metadata.get("ask").and_then(|v| v.as_f64()),
                open: metadata.get("open").and_then(|v| v.as_f64()),
                high: metadata.get("high").and_then(|v| v.as_f64()),
                low: metadata.get("low").and_then(|v| v.as_f64()),
            };

            market_data.push(data);
        }

        Ok(market_data)
    }

    /// 存储元数据
    pub async fn store_metadata<T>(&self, key: &str, metadata: &T) -> AlphaResult<()>
    where
        T: serde::Serialize + Send + Sync,
    {
        self.metadata.store(key, metadata).await
    }

    /// 获取元数据
    pub async fn get_metadata<T>(&self, key: &str) -> AlphaResult<Option<T>>
    where
        T: for<'de> serde::Deserialize<'de> + Send + Sync,
    {
        self.metadata.retrieve(key).await
    }

    /// 列出所有支持的符号
    pub async fn list_symbols(&self) -> AlphaResult<Vec<String>> {
        self.timeseries.list_symbols().await
    }

    /// 删除符号的所有数据
    pub async fn delete_symbol(&self, symbol: &str) -> AlphaResult<()> {
        // 删除时间序列数据
        self.timeseries.delete_symbol(symbol).await?;

        // 删除缓存
        let cache_key = format!("latest_price:{}", symbol);
        self.cache.delete(&cache_key).await?;

        Ok(())
    }

    /// 获取存储统计信息
    pub async fn get_storage_statistics(&self) -> AlphaResult<StorageStatistics> {
        let timeseries_stats = self.timeseries.get_statistics().await?;

        Ok(StorageStatistics {
            timeseries_stats,
            total_symbols: timeseries_stats.total_symbols,
            total_data_points: timeseries_stats.total_points,
            storage_size_estimate: timeseries_stats.total_points * 64, // 估算每个点64字节
        })
    }

    /// 清理过期数据
    pub async fn cleanup_old_data(&self, before: DateTime<Utc>) -> AlphaResult<usize> {
        // 这里可以实现数据清理逻辑
        // 简化实现，返回0表示没有清理任何数据
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

        // 序列化为 JSON
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
    pub metadata_storage: StorageBackendType,
    pub cache_ttl_seconds: Option<u64>,
    pub max_cache_size: usize,
}

impl Default for DataAccessConfig {
    fn default() -> Self {
        Self {
            metadata_storage: StorageBackendType::Memory,
            cache_ttl_seconds: Some(3600), // 1小时
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
    pub timeseries_stats: super::TimeSeriesStats,
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
        let symbol = self.symbol.as_ref().ok_or_else(|| {
            AlphaError::InvalidInput("查询必须指定符号".to_string())
        })?;

        let start = self.start_time.unwrap_or_else(|| {
            Utc::now() - chrono::Duration::days(30) // 默认30天前
        });

        let end = self.end_time.unwrap_or_else(Utc::now);

        let mut data = dal.get_market_data_range(symbol, start, end).await?;

        // 应用限制
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_data_access_layer() {
        let config = DataAccessConfig::default();
        let dal = DataAccessLayer::new(config).await.unwrap();

        // 创建测试数据
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

        // 存储数据
        dal.store_market_data(&data).await.unwrap();

        // 获取最新价格
        let latest_price = dal.get_latest_price("AAPL").await.unwrap();
        assert_eq!(latest_price, Some(150.0));

        // 列出符号
        let symbols = dal.list_symbols().await.unwrap();
        assert!(symbols.contains(&"AAPL".to_string()));

        // 获取统计信息
        let stats = dal.get_storage_statistics().await.unwrap();
        assert_eq!(stats.total_symbols, 1);
        assert_eq!(stats.total_data_points, 1);
    }

    #[tokio::test]
    async fn test_query_builder() {
        let config = DataAccessConfig::default();
        let dal = DataAccessLayer::new(config).await.unwrap();

        // 创建测试数据
        let base_time = Utc::now();
        for i in 0..10 {
            let data = MarketData {
                symbol: "AAPL".to_string(),
                timestamp: base_time + chrono::Duration::hours(i),
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

        // 使用查询构建器
        let result = QueryBuilder::new()
            .symbol("AAPL")
            .start_time(base_time)
            .end_time(base_time + chrono::Duration::hours(4))
            .limit(5)
            .execute(&dal)
            .await
            .unwrap();

        assert_eq!(result.len(), 5);
        assert_eq!(result[0].price, 150.0);
        assert_eq!(result[4].price, 154.0);
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

        // 存储元数据
        dal.store_metadata("AAPL:info", &metadata).await.unwrap();

        // 获取元数据
        let retrieved: Option<TestMetadata> = dal.get_metadata("AAPL:info").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), metadata);
    }
}
