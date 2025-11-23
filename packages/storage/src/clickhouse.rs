//! Alpha Finance ClickHouse 存储适配器
//!
//! 专为金融数据分析设计的高性能 ClickHouse 存储层

use alpha_core::errors::AlphaError;
use chrono::{DateTime, Utc};
use clickhouse::Client;
use serde::{Deserialize, Serialize};

/// ClickHouse 连接配置
#[derive(Debug, Clone)]
pub struct ClickHouseConfig {
    pub url: String,
    pub native_url: String,
    pub database: String,
    pub user: String,
    pub password: String,
}

impl Default for ClickHouseConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:8123".to_string(),
            native_url: "tcp://localhost:9000".to_string(),
            database: "alpha_finance".to_string(),
            user: "admin".to_string(),
            password: "admin123".to_string(),
        }
    }
}

/// ClickHouse 存储客户端
pub struct ClickHouseStorage {
    client: Client,
    config: ClickHouseConfig,
}

impl ClickHouseStorage {
    /// 创建新的 ClickHouse 存储客户端
    pub async fn new(config: ClickHouseConfig) -> Result<Self, AlphaError> {
        let client = Client::default()
            .with_url(&config.url)
            .with_user(&config.user)
            .with_password(&config.password)
            .with_database(&config.database);

        // 简单的健康检查
        let health_check = client.query("SELECT 1").execute().await;
        if health_check.is_err() {
            return Err(AlphaError::StorageError("ClickHouse 连接失败".to_string()));
        }

        Ok(Self { client, config })
    }

    /// 执行 ClickHouse 查询
    pub async fn execute_query(&self, query: &str) -> Result<(), String> {
        self.client.query(query)
            .execute()
            .await
            .map_err(|e| format!("查询执行失败: {}", e))?;
        Ok(())
    }

    /// 插入市场数据
    pub async fn insert_market_data(
        &self,
        data: Vec<MarketDataInsert>,
    ) -> Result<(), String> {
        if data.is_empty() {
            return Ok(());
        }

        // 使用 HTTP 接口插入数据
        let url = format!("{}/?database={}&user={}&password={}",
            self.config.url, self.config.database, self.config.user, self.config.password);

        let mut payload = String::new();
        for item in data {
            let insert_sql = format!(
                "INSERT INTO market_data (timestamp, symbol_id, symbol, open_price, high_price, low_price, close_price, adj_close_price, volume, source) VALUES ('{}', {}, '{}', {}, {}, {}, {}, {}, {}, '{}')\n",
                item.timestamp.format("%Y-%m-%d %H:%M:%S"),
                item.symbol_id,
                item.symbol,
                item.open_price,
                item.high_price,
                item.low_price,
                item.close_price,
                item.adj_close_price,
                item.volume,
                item.source
            );
            payload.push_str(&insert_sql);
        }

        let client = reqwest::Client::new();
        let response = client.post(&url)
            .body(payload)
            .send()
            .await
            .map_err(|e| format!("HTTP 请求失败: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("插入失败: {}", response.status()));
        }

        Ok(())
    }

    /// 查询市场数据
    pub async fn query_market_data(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        limit: Option<u64>,
    ) -> Result<Vec<MarketDataRow>, String> {
        let mut query = format!(
            "SELECT timestamp, symbol, open_price, high_price, low_price, close_price, volume
             FROM market_data
             WHERE symbol = '{}'
             AND timestamp BETWEEN '{}' AND '{}'",
            symbol,
            start.format("%Y-%m-%d %H:%M:%S"),
            end.format("%Y-%m-%d %H:%M:%S")
        );

        if let Some(limit) = limit {
            query.push_str(&format!(" LIMIT {}", limit));
        }

        let url = format!("{}/?database={}&user={}&password={}&query={}",
            self.config.url, self.config.database, self.config.user, self.config.password,
            urlencoding::encode(&query));

        let client = reqwest::Client::new();
        let response = client.get(&url)
            .send()
            .await
            .map_err(|e| format!("HTTP 请求失败: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("查询失败: {}", response.status()));
        }

        let text = response.text().await
            .map_err(|e| format!("响应解析失败: {}", e))?;

        // 简单的 CSV 解析
        let mut results = Vec::new();
        if !text.trim().is_empty() {
            for line in text.trim().lines().skip(1) {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() >= 7 {
                    if let (Ok(timestamp), Ok(open_price), Ok(high_price), Ok(low_price), Ok(close_price), Ok(volume)) = (
                        parts[0].parse::<DateTime<Utc>>(),
                        parts[2].parse::<f64>(),
                        parts[3].parse::<f64>(),
                        parts[4].parse::<f64>(),
                        parts[5].parse::<f64>(),
                        parts[6].parse::<u64>(),
                    ) {
                        results.push(MarketDataRow {
                            timestamp,
                            symbol: symbol.to_string(),
                            open_price,
                            high_price,
                            low_price,
                            close_price,
                            volume,
                        });
                    }
                }
            }
        }

        Ok(results)
    }

    /// 插入技术指标
    pub async fn insert_technical_indicators(
        &self,
        indicators: Vec<TechnicalIndicatorInsert>,
    ) -> Result<(), String> {
        if indicators.is_empty() {
            return Ok(());
        }

        // 使用 HTTP 接口插入数据
        let url = format!("{}/?database={}&user={}&password={}",
            self.config.url, self.config.database, self.config.user, self.config.password);

        let mut payload = String::new();
        for item in indicators {
            let insert_sql = format!(
                "INSERT INTO technical_indicators (timestamp, symbol_id, symbol, indicator_name, period, value, value_upper, value_lower, source) VALUES ('{}', {}, '{}', '{}', {}, {}, {}, {}, '{}')\n",
                item.timestamp.format("%Y-%m-%d %H:%M:%S"),
                item.symbol_id,
                item.symbol,
                item.indicator_name,
                item.period,
                item.value,
                item.value_upper.unwrap_or(0.0),
                item.value_lower.unwrap_or(0.0),
                item.source
            );
            payload.push_str(&insert_sql);
        }

        let client = reqwest::Client::new();
        let response = client.post(&url)
            .body(payload)
            .send()
            .await
            .map_err(|e| format!("HTTP 请求失败: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("插入失败: {}", response.status()));
        }

        Ok(())
    }

    /// 获取实时报价
    pub async fn get_realtime_quotes(&self) -> Result<Vec<RealtimeQuoteRow>, String> {
        let query = "SELECT symbol, last_price, bid_price, ask_price, volume, timestamp, change_amount, change_percent FROM realtime_quotes";

        let url = format!("{}/?database={}&user={}&password={}&query={}",
            self.config.url, self.config.database, self.config.user, self.config.password,
            urlencoding::encode(query));

        let client = reqwest::Client::new();
        let response = client.get(&url)
            .send()
            .await
            .map_err(|e| format!("HTTP 请求失败: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("查询失败: {}", response.status()));
        }

        let text = response.text().await
            .map_err(|e| format!("响应解析失败: {}", e))?;

        // 简单的 CSV 解析
        let mut results = Vec::new();
        if !text.trim().is_empty() {
            for line in text.trim().lines().skip(1) {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() >= 8 {
                    if let (Ok(last_price), Ok(bid_price), Ok(ask_price), Ok(volume), Ok(timestamp), Ok(change_amount), Ok(change_percent)) = (
                        parts[1].parse::<f64>(),
                        parts[2].parse::<f64>(),
                        parts[3].parse::<f64>(),
                        parts[4].parse::<u64>(),
                        parts[5].parse::<DateTime<Utc>>(),
                        parts[6].parse::<f64>(),
                        parts[7].parse::<f64>(),
                    ) {
                        results.push(RealtimeQuoteRow {
                            symbol: parts[0].to_string(),
                            last_price,
                            bid_price,
                            ask_price,
                            volume,
                            timestamp,
                            change_amount,
                            change_percent,
                        });
                    }
                }
            }
        }

        Ok(results)
    }
}

/// 市场数据插入结构
#[derive(Debug, Serialize)]
pub struct MarketDataInsert {
    pub timestamp: DateTime<Utc>,
    pub symbol_id: u32,
    pub symbol: String,
    pub open_price: f64,
    pub high_price: f64,
    pub low_price: f64,
    pub close_price: f64,
    pub adj_close_price: f64,
    pub volume: u64,
    pub source: String,
}

/// 市场数据查询结果
#[derive(Debug, Deserialize)]
pub struct MarketDataRow {
    pub timestamp: DateTime<Utc>,
    pub symbol: String,
    pub open_price: f64,
    pub high_price: f64,
    pub low_price: f64,
    pub close_price: f64,
    pub volume: u64,
}

/// 技术指标插入结构
#[derive(Debug, Serialize)]
pub struct TechnicalIndicatorInsert {
    pub timestamp: DateTime<Utc>,
    pub symbol_id: u32,
    pub symbol: String,
    pub indicator_name: String,
    pub period: u16,
    pub value: f64,
    pub value_upper: Option<f64>,
    pub value_lower: Option<f64>,
    pub source: String,
}

/// 实时报价查询结果
#[derive(Debug, Deserialize)]
pub struct RealtimeQuoteRow {
    pub symbol: String,
    pub last_price: f64,
    pub bid_price: f64,
    pub ask_price: f64,
    pub volume: u64,
    pub timestamp: DateTime<Utc>,
    pub change_amount: f64,
    pub change_percent: f64,
}

/// ClickHouse 查询构建器
pub struct ClickHouseQueryBuilder {
    query: String,
}

impl ClickHouseQueryBuilder {
    pub fn new() -> Self {
        Self {
            query: String::new(),
        }
    }

    pub fn select(mut self, columns: &str) -> Self {
        self.query = format!("SELECT {}", columns);
        self
    }

    pub fn from(mut self, table: &str) -> Self {
        self.query.push_str(&format!(" FROM {}", table));
        self
    }

    pub fn where_clause(mut self, condition: &str) -> Self {
        self.query.push_str(&format!(" WHERE {}", condition));
        self
    }

    pub fn order_by(mut self, order: &str) -> Self {
        self.query.push_str(&format!(" ORDER BY {}", order));
        self
    }

    pub fn limit(mut self, limit: u64) -> Self {
        self.query.push_str(&format!(" LIMIT {}", limit));
        self
    }

    pub fn build(self) -> String {
        self.query
    }
}

impl Default for ClickHouseQueryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_clickhouse_config_default() {
        let config = ClickHouseConfig::default();
        assert_eq!(config.database, "alpha_finance");
        assert_eq!(config.user, "admin");
    }

    #[test]
    fn test_market_data_insert_serialization() {
        let data = MarketDataInsert {
            timestamp: Utc::now(),
            symbol_id: 1,
            symbol: "AAPL".to_string(),
            open_price: 150.0,
            high_price: 155.0,
            low_price: 149.0,
            close_price: 154.0,
            adj_close_price: 154.0,
            volume: 1000000,
            source: "test".to_string(),
        };

        // 测试序列化
        let json = serde_json::to_string(&data).unwrap();
        assert!(json.contains("AAPL"));
    }

    #[test]
    fn test_query_builder() {
        let query = ClickHouseQueryBuilder::new()
            .select("symbol, close_price, volume")
            .from("market_data")
            .where_clause("symbol = 'AAPL'")
            .order_by("timestamp DESC")
            .limit(10)
            .build();

        assert!(query.contains("SELECT symbol, close_price, volume"));
        assert!(query.contains("FROM market_data"));
        assert!(query.contains("WHERE symbol = 'AAPL'"));
        assert!(query.contains("ORDER BY timestamp DESC"));
        assert!(query.contains("LIMIT 10"));
    }
}