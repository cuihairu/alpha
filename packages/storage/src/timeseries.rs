//! 时间序列数据存储实现

use alpha_core::errors::AlphaResult;
use alpha_core::models::MarketData;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 时间序列数据点
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeriesPoint {
    pub timestamp: DateTime<Utc>,
    pub value: f64,
    pub volume: Option<u64>,
    pub metadata: Option<serde_json::Value>,
}

/// 时间序列数据段
#[derive(Debug, Clone)]
pub struct TimeSeries {
    pub symbol: String,
    pub data: Vec<TimeSeriesPoint>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TimeSeries {
    pub fn new(symbol: String) -> Self {
        let now = Utc::now();
        Self {
            symbol,
            data: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn add_point(&mut self, point: TimeSeriesPoint) {
        self.data.push(point);
        self.data.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        self.updated_at = Utc::now();
    }

    pub fn get_latest(&self) -> Option<&TimeSeriesPoint> {
        self.data.last()
    }

    pub fn get_points_in_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Vec<&TimeSeriesPoint> {
        self.data
            .iter()
            .filter(|point| point.timestamp >= start && point.timestamp <= end)
            .collect()
    }

    pub fn resample(&self, interval_seconds: i64) -> Vec<TimeSeriesPoint> {
        if self.data.is_empty() {
            return Vec::new();
        }

        let mut resampled = Vec::new();
        let mut current_interval = self.data[0].timestamp;
        let mut interval_points = Vec::new();

        for point in &self.data {
            if point.timestamp < current_interval + chrono::Duration::seconds(interval_seconds) {
                interval_points.push(point);
            } else {
                // 处理当前区间
                if !interval_points.is_empty() {
                    let avg_price = interval_points.iter().map(|p| p.value).sum::<f64>()
                        / interval_points.len() as f64;
                    let total_volume: u64 = interval_points.iter().filter_map(|p| p.volume).sum();

                    resampled.push(TimeSeriesPoint {
                        timestamp: current_interval,
                        value: avg_price,
                        volume: Some(total_volume),
                        metadata: None,
                    });
                }

                // 开始新区间
                current_interval = point.timestamp;
                interval_points.clear();
                interval_points.push(point);
            }
        }

        // 处理最后一个区间
        if !interval_points.is_empty() {
            let avg_price =
                interval_points.iter().map(|p| p.value).sum::<f64>() / interval_points.len() as f64;
            let total_volume: u64 = interval_points.iter().filter_map(|p| p.volume).sum();

            resampled.push(TimeSeriesPoint {
                timestamp: current_interval,
                value: avg_price,
                volume: Some(total_volume),
                metadata: None,
            });
        }

        resampled
    }
}

/// 内存时间序列存储
#[derive(Debug)]
pub struct TimeSeriesStorage {
    series: Arc<RwLock<BTreeMap<String, TimeSeries>>>,
}

impl TimeSeriesStorage {
    pub fn new() -> Self {
        Self {
            series: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    /// 添加市场数据
    pub async fn add_market_data(&self, data: &MarketData) -> AlphaResult<()> {
        let point = TimeSeriesPoint {
            timestamp: data.timestamp,
            value: data.price,
            volume: Some(data.volume),
            metadata: Some(serde_json::json!({
                "bid": data.bid,
                "ask": data.ask,
                "open": data.open,
                "high": data.high,
                "low": data.low,
            })),
        };

        let mut series_map = self.series.write().await;
        let series = series_map
            .entry(data.symbol.clone())
            .or_insert_with(|| TimeSeries::new(data.symbol.clone()));
        series.add_point(point);

        Ok(())
    }

    /// 批量添加市场数据
    pub async fn add_market_data_batch(&self, data_list: &[MarketData]) -> AlphaResult<()> {
        let mut series_map = self.series.write().await;

        for market_data in data_list {
            let point = TimeSeriesPoint {
                timestamp: market_data.timestamp,
                value: market_data.price,
                volume: Some(market_data.volume),
                metadata: Some(serde_json::json!({
                    "bid": market_data.bid,
                    "ask": market_data.ask,
                    "open": market_data.open,
                    "high": market_data.high,
                    "low": market_data.low,
                })),
            };

            let series = series_map
                .entry(market_data.symbol.clone())
                .or_insert_with(|| TimeSeries::new(market_data.symbol.clone()));
            series.add_point(point);
        }

        Ok(())
    }

    /// 获取时间序列
    pub async fn get_series(&self, symbol: &str) -> AlphaResult<Option<TimeSeries>> {
        let series_map = self.series.read().await;
        Ok(series_map.get(symbol).cloned())
    }

    /// 获取指定时间范围内的数据
    pub async fn get_data_in_range(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> AlphaResult<Vec<TimeSeriesPoint>> {
        let series_map = self.series.read().await;

        if let Some(series) = series_map.get(symbol) {
            Ok(series
                .get_points_in_range(start, end)
                .into_iter()
                .cloned()
                .collect())
        } else {
            Ok(Vec::new())
        }
    }

    /// 获取最新价格
    pub async fn get_latest_price(&self, symbol: &str) -> AlphaResult<Option<f64>> {
        let series_map = self.series.read().await;

        if let Some(series) = series_map.get(symbol) {
            Ok(series.get_latest().map(|point| point.value))
        } else {
            Ok(None)
        }
    }

    /// 列出所有符号
    pub async fn list_symbols(&self) -> AlphaResult<Vec<String>> {
        let series_map = self.series.read().await;
        Ok(series_map.keys().cloned().collect())
    }

    /// 删除符号的所有数据
    pub async fn delete_symbol(&self, symbol: &str) -> AlphaResult<bool> {
        let mut series_map = self.series.write().await;
        Ok(series_map.remove(symbol).is_some())
    }

    /// 获取统计信息
    pub async fn get_statistics(&self) -> AlphaResult<TimeSeriesStats> {
        let series_map = self.series.read().await;
        let mut total_points = 0;
        let total_symbols = series_map.len();
        let mut oldest_timestamp = None;
        let mut newest_timestamp = None;

        for series in series_map.values() {
            total_points += series.data.len();

            if let Some(first_point) = series.data.first() {
                oldest_timestamp = match oldest_timestamp {
                    None => Some(first_point.timestamp),
                    Some(oldest) => Some(oldest.min(first_point.timestamp)),
                };
            }

            if let Some(last_point) = series.data.last() {
                newest_timestamp = match newest_timestamp {
                    None => Some(last_point.timestamp),
                    Some(newest) => Some(newest.max(last_point.timestamp)),
                };
            }
        }

        Ok(TimeSeriesStats {
            total_symbols,
            total_points,
            oldest_timestamp,
            newest_timestamp,
        })
    }
}

/// 时间序列存储统计信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeriesStats {
    pub total_symbols: usize,
    pub total_points: usize,
    pub oldest_timestamp: Option<DateTime<Utc>>,
    pub newest_timestamp: Option<DateTime<Utc>>,
}

impl Default for TimeSeriesStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_time_series_storage() {
        let storage = TimeSeriesStorage::new();

        // 添加市场数据
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

        storage.add_market_data(&data).await.unwrap();

        // 检索系列
        let series = storage.get_series("AAPL").await.unwrap();
        assert!(series.is_some());

        let series = series.unwrap();
        assert_eq!(series.symbol, "AAPL");
        assert_eq!(series.data.len(), 1);
        assert_eq!(series.data[0].value, 150.0);
    }

    #[tokio::test]
    async fn test_time_series_range_query() {
        let storage = TimeSeriesStorage::new();

        let base_time = Utc::now();
        let mut data_list = Vec::new();

        // 创建10天的数据
        for i in 0..10 {
            let data = MarketData {
                symbol: "AAPL".to_string(),
                timestamp: base_time + chrono::Duration::days(i),
                price: 150.0 + i as f64,
                volume: 1000,
                bid: Some(149.5 + i as f64),
                ask: Some(150.5 + i as f64),
                open: Some(149.0 + i as f64),
                high: Some(151.0 + i as f64),
                low: Some(148.5 + i as f64),
            };
            data_list.push(data);
        }

        storage.add_market_data_batch(&data_list).await.unwrap();

        // 查询前5天的数据
        let start_time = base_time;
        let end_time = base_time + chrono::Duration::days(4);
        let range_data = storage
            .get_data_in_range("AAPL", start_time, end_time)
            .await
            .unwrap();

        assert_eq!(range_data.len(), 5);
        assert_eq!(range_data[0].value, 150.0);
        assert_eq!(range_data[4].value, 154.0);
    }
}
