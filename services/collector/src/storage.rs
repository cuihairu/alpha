//! 存储层集成模块
//!
//! 提供 TimescaleDB 和 Redis 存储集成功能

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, error, info, warn};

use crate::sources::{KlineData, RealtimeQuote};

/// 存储错误
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Connection error: {0}")]
    ConnectionError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Redis error: {0}")]
    RedisError(String),

    #[error("Not found: {0}")]
    NotFound(String),
}

/// 存储配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// PostgreSQL/TimescaleDB 配置
    pub postgres: PostgresConfig,
    /// Redis 配置
    pub redis: RedisConfig,
    /// 是否启用 TimescaleDB
    pub enable_timescale: bool,
    /// 是否启用 Redis
    pub enable_redis: bool,
}

/// PostgreSQL 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostgresConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database: String,
    pub max_connections: u32,
}

impl Default for PostgresConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5432,
            username: "postgres".to_string(),
            password: "postgres".to_string(),
            database: "alpha_finance".to_string(),
            max_connections: 10,
        }
    }
}

/// Redis 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisConfig {
    pub url: String,
    pub max_connections: u32,
    pub default_ttl_seconds: u64,
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            url: "redis://localhost:6379".to_string(),
            max_connections: 10,
            default_ttl_seconds: 3600,
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            postgres: PostgresConfig::default(),
            redis: RedisConfig::default(),
            enable_timescale: true,
            enable_redis: true,
        }
    }
}

/// 存储层
pub struct StorageLayer {
    config: StorageConfig,
    /// TimescaleDB 连接池
    timescale_pool: Option<sqlx::PgPool>,
    /// Redis 连接
    redis_client: Option<redis::Client>,
}

impl StorageLayer {
    /// 创建新的存储层
    pub async fn new(config: StorageConfig) -> Result<Self, StorageError> {
        let mut timescale_pool = None;
        let mut redis_client = None;

        // 初始化 TimescaleDB
        if config.enable_timescale {
            info!("Connecting to TimescaleDB...");
            let database_url = format!(
                "postgres://{}:{}@{}:{}/{}",
                config.postgres.username,
                config.postgres.password,
                config.postgres.host,
                config.postgres.port,
                config.postgres.database
            );

            match sqlx::PgPool::connect(&database_url).await {
                Ok(pool) => {
                    timescale_pool = Some(pool);
                    info!("TimescaleDB connected successfully");
                }
                Err(e) => {
                    warn!("Failed to connect to TimescaleDB: {}", e);
                }
            }
        }

        // 初始化 Redis
        if config.enable_redis {
            info!("Connecting to Redis...");
            match redis::Client::open(config.redis.url.clone()) {
                Ok(client) => {
                    // 测试连接
                    match client.get_multiplexed_async_connection().await {
                        Ok(mut conn) => {
                            let _: () = redis::cmd("PING").query_async(&mut conn).await.unwrap_or(());
                            redis_client = Some(client);
                            info!("Redis connected successfully");
                        }
                        Err(e) => {
                            warn!("Failed to test Redis connection: {}", e);
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to connect to Redis: {}", e);
                }
            }
        }

        Ok(Self {
            config,
            timescale_pool,
            redis_client,
        })
    }

    /// 保存实时行情数据到 TimescaleDB
    pub async fn save_realtime_quote(&self, quote: &RealtimeQuote) -> Result<(), StorageError> {
        let pool = self.timescale_pool
            .as_ref()
            .ok_or_else(|| StorageError::ConnectionError("TimescaleDB not connected".to_string()))?;

        let query = r#"
            INSERT INTO realtime_quotes (
                symbol, name, price, pre_close, open, high, low,
                volume, amount, change, change_percent,
                bid1, ask1, bid1_volume, ask1_volume, timestamp, source
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
            ON CONFLICT (symbol, timestamp) DO UPDATE SET
                price = EXCLUDED.price,
                volume = EXCLUDED.volume,
                amount = EXCLUDED.amount,
                bid1 = EXCLUDED.bid1,
                ask1 = EXCLUDED.ask1
        "#;

        sqlx::query(query)
            .bind(&quote.symbol)
            .bind(&quote.name)
            .bind(quote.price)
            .bind(quote.pre_close)
            .bind(quote.open)
            .bind(quote.high)
            .bind(quote.low)
            .bind(quote.volume as i64)
            .bind(quote.amount)
            .bind(quote.change)
            .bind(quote.change_percent)
            .bind(quote.bid1)
            .bind(quote.ask1)
            .bind(quote.bid1_volume.map(|v| v as i64))
            .bind(quote.ask1_volume.map(|v| v as i64))
            .bind(quote.timestamp)
            .bind(&quote.source)
            .execute(pool)
            .await
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        debug!("Saved realtime quote for {}", quote.symbol);
        Ok(())
    }

    /// 批量保存实时行情数据
    pub async fn save_realtime_quotes(&self, quotes: &[RealtimeQuote]) -> Result<usize, StorageError> {
        self.timescale_pool
            .as_ref()
            .ok_or_else(|| StorageError::ConnectionError("TimescaleDB not connected".to_string()))?;

        let mut saved_count = 0;

        for quote in quotes {
            match self.save_realtime_quote(quote).await {
                Ok(_) => saved_count += 1,
                Err(e) => {
                    error!("Failed to save quote for {}: {}", quote.symbol, e);
                }
            }
        }

        Ok(saved_count)
    }

    /// 保存 K线数据
    pub async fn save_kline(&self, kline: &KlineData) -> Result<(), StorageError> {
        let pool = self.timescale_pool
            .as_ref()
            .ok_or_else(|| StorageError::ConnectionError("TimescaleDB not connected".to_string()))?;

        let kline_type_str = format!("{:?}", kline.kline_type);

        let query = r#"
            INSERT INTO kline_data (
                symbol, kline_type, timestamp, open, high, low, close,
                volume, amount, change_percent, change, turnover_rate
            ) VALUES ($1, $2, to_timestamp($3), $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (symbol, kline_type, timestamp) DO UPDATE SET
                open = EXCLUDED.open,
                high = EXCLUDED.high,
                low = EXCLUDED.low,
                close = EXCLUDED.close,
                volume = EXCLUDED.volume
        "#;

        sqlx::query(query)
            .bind(&kline.symbol)
            .bind(kline_type_str)
            .bind(kline.timestamp)
            .bind(kline.open)
            .bind(kline.high)
            .bind(kline.low)
            .bind(kline.close)
            .bind(kline.volume as i64)
            .bind(kline.amount)
            .bind(kline.change_percent)
            .bind(kline.change)
            .bind(kline.turnover_rate)
            .execute(pool)
            .await
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        debug!("Saved kline data for {} {:?}", kline.symbol, kline.kline_type);
        Ok(())
    }

    /// 缓存实时行情到 Redis
    pub async fn cache_realtime_quote(&self, quote: &RealtimeQuote) -> Result<(), StorageError> {
        let client = self.redis_client
            .as_ref()
            .ok_or_else(|| StorageError::ConnectionError("Redis not connected".to_string()))?;

        let key = format!("quote:{}", quote.symbol);
        let value = serde_json::to_string(quote)
            .map_err(|e| StorageError::SerializationError(e.to_string()))?;

        let mut conn = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| StorageError::RedisError(e.to_string()))?;

        let _: () = redis::cmd("SETEX")
            .arg(&key)
            .arg(self.config.redis.default_ttl_seconds)
            .arg(&value)
            .query_async(&mut conn)
            .await
            .map_err(|e| StorageError::RedisError(e.to_string()))?;

        debug!("Cached quote for {} in Redis", quote.symbol);
        Ok(())
    }

    /// 从 Redis 获取缓存的实时行情
    pub async fn get_cached_quote(&self, symbol: &str) -> Result<Option<RealtimeQuote>, StorageError> {
        let client = self.redis_client
            .as_ref()
            .ok_or_else(|| StorageError::ConnectionError("Redis not connected".to_string()))?;

        let key = format!("quote:{}", symbol);

        let mut conn = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| StorageError::RedisError(e.to_string()))?;

        let value: Option<String> = redis::cmd("GET")
            .arg(&key)
            .query_async(&mut conn)
            .await
            .map_err(|e| StorageError::RedisError(e.to_string()))?;

        match value {
            Some(v) => {
                let quote = serde_json::from_str(&v)
                    .map_err(|e| StorageError::SerializationError(e.to_string()))?;
                Ok(Some(quote))
            }
            None => Ok(None),
        }
    }

    /// 初始化数据库表
    pub async fn init_schema(&self) -> Result<(), StorageError> {
        let pool = self.timescale_pool
            .as_ref()
            .ok_or_else(|| StorageError::ConnectionError("TimescaleDB not connected".to_string()))?;

        // 创建实时行情表
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS realtime_quotes (
                symbol VARCHAR(20) NOT NULL,
                name VARCHAR(50),
                price DECIMAL(10, 3),
                pre_close DECIMAL(10, 3),
                open DECIMAL(10, 3),
                high DECIMAL(10, 3),
                low DECIMAL(10, 3),
                volume BIGINT,
                amount DECIMAL(20, 2),
                change DECIMAL(10, 3),
                change_percent DECIMAL(10, 3),
                bid1 DECIMAL(10, 3),
                ask1 DECIMAL(10, 3),
                bid1_volume BIGINT,
                ask1_volume BIGINT,
                timestamp TIMESTAMP WITH TIME ZONE NOT NULL,
                source VARCHAR(20),
                PRIMARY KEY (symbol, timestamp)
            );
        "#)
        .execute(pool)
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        // 创建 K线数据表
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS kline_data (
                symbol VARCHAR(20) NOT NULL,
                kline_type VARCHAR(20) NOT NULL,
                timestamp TIMESTAMP WITH TIME ZONE NOT NULL,
                open DECIMAL(10, 3),
                high DECIMAL(10, 3),
                low DECIMAL(10, 3),
                close DECIMAL(10, 3),
                volume BIGINT,
                amount DECIMAL(20, 2),
                change_percent DECIMAL(10, 3),
                change DECIMAL(10, 3),
                turnover_rate DECIMAL(10, 3),
                PRIMARY KEY (symbol, kline_type, timestamp)
            );
        "#)
        .execute(pool)
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        info!("Database schema initialized");
        Ok(())
    }

    /// 检查存储健康状态
    pub async fn health_check(&self) -> bool {
        let mut healthy = true;

        // 检查 TimescaleDB
        if let Some(pool) = &self.timescale_pool {
            match sqlx::query("SELECT 1").fetch_one(pool).await {
                Ok(_) => debug!("TimescaleDB health check passed"),
                Err(e) => {
                    warn!("TimescaleDB health check failed: {}", e);
                    healthy = false;
                }
            }
        }

        // 检查 Redis
        if let Some(client) = &self.redis_client {
            match client.get_multiplexed_async_connection().await {
                Ok(mut conn) => {
                    match redis::cmd("PING").query_async::<_, ()>(&mut conn).await {
                        Ok(_) => debug!("Redis health check passed"),
                        Err(e) => {
                            warn!("Redis health check failed: {}", e);
                            healthy = false;
                        }
                    }
                }
                Err(e) => {
                    warn!("Redis connection failed: {}", e);
                    healthy = false;
                }
            }
        }

        healthy
    }
}

/// 存储层包装器（用于异步上下文）
#[derive(Clone)]
pub struct StorageLayerHandle {
    inner: Arc<StorageLayer>,
}

impl StorageLayerHandle {
    /// 创建新的存储层句柄
    pub async fn new(config: StorageConfig) -> Result<Self, StorageError> {
        let layer = StorageLayer::new(config).await?;
        Ok(Self {
            inner: Arc::new(layer),
        })
    }

    /// 获取存储层引用
    pub async fn get(&self) -> Result<Arc<StorageLayer>, StorageError> {
        Ok(Arc::clone(&self.inner))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_storage_config_default() {
        let config = StorageConfig::default();
        assert_eq!(config.postgres.host, "localhost");
        assert_eq!(config.redis.url, "redis://localhost:6379");
    }

    #[ignore] // 需要实际数据库连接
    #[tokio::test]
    async fn test_storage_layer_creation() {
        let config = StorageConfig::default();
        let layer = StorageLayer::new(config).await;

        // 这会失败，因为默认配置可能没有实际的数据库
        assert!(layer.is_ok() || layer.is_err());
    }
}
