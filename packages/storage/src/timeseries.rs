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

fn market_data_to_point(data: &MarketData) -> TimeSeriesPoint {
    TimeSeriesPoint {
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
    }
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

    /// 写入单点：按 ts 二分定位，有序插入或覆盖同 ts 旧点（UPSERT，最后写赢，
    /// 与 Timescale 层 ON CONFLICT DO UPDATE 口径一致）。
    /// 不变量：`data` 恒按 ts 升序且 (symbol, ts) 唯一——所有读路径
    /// （get_latest / get_points_in_range / resample / get_series）零改动。
    /// 此前实现是 push + 全量排序（逐条灌入 O(n² log n)，48 万点需数分钟）。
    pub fn add_point(&mut self, point: TimeSeriesPoint) {
        match self
            .data
            .binary_search_by(|p| p.timestamp.cmp(&point.timestamp))
        {
            Ok(idx) => self.data[idx] = point, // 同 ts：覆盖旧点（最后写赢）
            Err(idx) => self.data.insert(idx, point),
        }
        self.updated_at = Utc::now();
    }

    /// 批量写入：一次追加 + 一次排序 + 一次去重，把批量灌入从
    /// 逐条全排序的 O(n² log n) 降到 O((n+m) log(n+m))。
    /// 同 ts 冲突同为最后写赢（含批内重复、批与存量重复两种情况）。
    pub fn add_points<I: IntoIterator<Item = TimeSeriesPoint>>(&mut self, points: I) {
        let mut merged = std::mem::take(&mut self.data);
        merged.extend(points);
        // 稳定排序：同 ts 保持插入顺序，下面的去重因此天然「后者覆盖前者」
        merged.sort_by_key(|a| a.timestamp);

        let mut deduped: Vec<TimeSeriesPoint> = Vec::with_capacity(merged.len());
        for point in merged {
            match deduped.last_mut() {
                Some(last) if last.timestamp == point.timestamp => *last = point,
                _ => deduped.push(point),
            }
        }
        self.data = deduped;
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
        let point = market_data_to_point(data);

        let mut series_map = self.series.write().await;
        let series = series_map
            .entry(data.symbol.clone())
            .or_insert_with(|| TimeSeries::new(data.symbol.clone()));
        series.add_point(point);

        Ok(())
    }

    /// 批量添加市场数据：按 symbol 分组，每个序列一次批量写入
    /// （一次排序/去重），不再逐条触发全量排序。
    pub async fn add_market_data_batch(&self, data_list: &[MarketData]) -> AlphaResult<()> {
        let mut grouped: BTreeMap<String, Vec<TimeSeriesPoint>> = BTreeMap::new();
        for market_data in data_list {
            grouped
                .entry(market_data.symbol.clone())
                .or_default()
                .push(market_data_to_point(market_data));
        }

        let mut series_map = self.series.write().await;
        for (symbol, points) in grouped {
            let series = series_map
                .entry(symbol.clone())
                .or_insert_with(|| TimeSeries::new(symbol));
            series.add_points(points);
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

    fn md(symbol: &str, ts: DateTime<Utc>, price: f64) -> MarketData {
        MarketData {
            symbol: symbol.to_string(),
            timestamp: ts,
            price,
            volume: 100,
            bid: None,
            ask: None,
            open: None,
            high: None,
            low: None,
        }
    }

    /// 批量乱序灌入后：内存序列恒有序，区间查询有序返回（读路径不变式）。
    #[tokio::test]
    async fn batch_out_of_order_insert_is_sorted_and_queryable() {
        let storage = TimeSeriesStorage::new();
        let base = Utc::now();

        let shuffled: Vec<MarketData> = [3_i64, 0, 7, 2, 9, 1, 5, 8, 4, 6]
            .iter()
            .map(|&i| {
                md(
                    "SHUF",
                    base + chrono::Duration::seconds(i),
                    100.0 + i as f64,
                )
            })
            .collect();
        storage.add_market_data_batch(&shuffled).await.unwrap();

        let series = storage.get_series("SHUF").await.unwrap().unwrap();
        let timestamps: Vec<i64> = series
            .data
            .iter()
            .map(|p| (p.timestamp - base).num_seconds())
            .collect();
        assert_eq!(
            timestamps,
            vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
            "series must stay sorted"
        );

        let range = storage
            .get_data_in_range("SHUF", base, base + chrono::Duration::seconds(4))
            .await
            .unwrap();
        assert_eq!(range.len(), 5);
        assert_eq!(range[0].value, 100.0);
        assert_eq!(range[4].value, 104.0);
        assert_eq!(storage.get_latest_price("SHUF").await.unwrap(), Some(109.0));
    }

    /// 重复 (symbol, ts) 去重：单点与批量路径同为「最后写赢」，只留一条。
    #[tokio::test]
    async fn duplicate_symbol_ts_upserts_last_write_wins() {
        let storage = TimeSeriesStorage::new();
        let base = Utc::now();
        let ts1 = base;
        let ts2 = base + chrono::Duration::seconds(1);

        // 单点路径：同 ts 后写覆盖前写
        storage
            .add_market_data(&md("DUP", ts1, 10.0))
            .await
            .unwrap();
        storage
            .add_market_data(&md("DUP", ts1, 20.0))
            .await
            .unwrap();

        // 批量路径：批内重复 + 与存量重复，均保留最后写入
        storage
            .add_market_data_batch(&[
                md("DUP", ts1, 30.0),
                md("DUP", ts2, 99.0),
                md("DUP", ts2, 88.0),
            ])
            .await
            .unwrap();

        let series = storage.get_series("DUP").await.unwrap().unwrap();
        assert_eq!(
            series.data.len(),
            2,
            "duplicate (symbol, ts) must collapse to one point"
        );
        assert_eq!(series.data[0].timestamp, ts1);
        assert_eq!(
            series.data[0].value, 30.0,
            "batch later write must win over existing point"
        );
        assert_eq!(series.data[1].timestamp, ts2);
        assert_eq!(
            series.data[1].value, 88.0,
            "within-batch later write must win"
        );
    }

    /// 写后立即查询可见（单点路径，含乱序插入后 get_latest 语义）。
    #[tokio::test]
    async fn single_write_is_immediately_visible_in_range_query() {
        let storage = TimeSeriesStorage::new();
        let base = Utc::now();
        let late = base + chrono::Duration::hours(1);
        let early = base - chrono::Duration::hours(1);

        storage
            .add_market_data(&md("VIS", late, 42.0))
            .await
            .unwrap();
        // 写入乱序的更早点后，区间查询与最新价都要立即反映
        let visible = storage.get_data_in_range("VIS", late, late).await.unwrap();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].value, 42.0);

        storage
            .add_market_data(&md("VIS", early, 41.0))
            .await
            .unwrap();
        let all = storage.get_data_in_range("VIS", early, late).await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].value, 41.0, "earlier point must sort first");
        assert_eq!(storage.get_latest_price("VIS").await.unwrap(), Some(42.0));
    }
}
