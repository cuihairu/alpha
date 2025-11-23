//! TimescaleDB 时间序列存储
//!
//! 提供基于 TimescaleDB/SQLx 的市场数据落盘能力

use alpha_core::{
    errors::{AlphaError, AlphaResult},
    models::MarketData,
};
use chrono::{DateTime, Utc};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::convert::TryFrom;
use tracing::{debug, instrument};

/// TimescaleDB 时间序列存储
#[derive(Clone)]
pub struct TimescaleTimeSeriesStorage {
    pool: PgPool,
}

impl TimescaleTimeSeriesStorage {
    /// 连接 TimescaleDB 并确保基础表结构存在
    pub async fn connect(database_url: &str) -> AlphaResult<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await
            .map_err(map_db_error)?;

        let storage = Self { pool };
        storage.ensure_schema().await?;

        Ok(storage)
    }

    /// 访问底层连接池
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// 写入单条市场数据
    #[instrument(skip_all, fields(symbol = %data.symbol))]
    pub async fn insert_market_data(&self, data: &MarketData) -> AlphaResult<()> {
        sqlx::query(
            r#"
            INSERT INTO market_timeseries
            (symbol, ts, price, volume, bid, ask, open, high, low)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
            ON CONFLICT (symbol, ts) DO UPDATE SET
                price = EXCLUDED.price,
                volume = EXCLUDED.volume,
                bid = EXCLUDED.bid,
                ask = EXCLUDED.ask,
                open = EXCLUDED.open,
                high = EXCLUDED.high,
                low = EXCLUDED.low
            "#,
        )
        .bind(&data.symbol)
        .bind(data.timestamp)
        .bind(data.price)
        .bind(as_i64(data.volume))
        .bind(data.bid)
        .bind(data.ask)
        .bind(data.open)
        .bind(data.high)
        .bind(data.low)
        .execute(&self.pool)
        .await
        .map_err(map_db_error)?;

        Ok(())
    }

    /// 批量写入市场数据（单事务）
    #[instrument(skip_all, fields(batch = data_list.len()))]
    pub async fn insert_market_data_batch(&self, data_list: &[MarketData]) -> AlphaResult<()> {
        if data_list.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await.map_err(map_db_error)?;

        for data in data_list {
            sqlx::query(
                r#"
                INSERT INTO market_timeseries
                (symbol, ts, price, volume, bid, ask, open, high, low)
                VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
                ON CONFLICT (symbol, ts) DO UPDATE SET
                    price = EXCLUDED.price,
                    volume = EXCLUDED.volume,
                    bid = EXCLUDED.bid,
                    ask = EXCLUDED.ask,
                    open = EXCLUDED.open,
                    high = EXCLUDED.high,
                    low = EXCLUDED.low
                "#,
            )
            .bind(&data.symbol)
            .bind(data.timestamp)
            .bind(data.price)
            .bind(as_i64(data.volume))
            .bind(data.bid)
            .bind(data.ask)
            .bind(data.open)
            .bind(data.high)
            .bind(data.low)
            .execute(&mut *tx)
            .await
            .map_err(map_db_error)?;
        }

        tx.commit().await.map_err(map_db_error)?;
        Ok(())
    }

    /// 查询指定区间的市场数据
    pub async fn fetch_range(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> AlphaResult<Vec<MarketData>> {
        let rows = sqlx::query_as::<_, TimescaleRow>(
            r#"
            SELECT symbol, ts, price, volume, bid, ask, open, high, low
            FROM market_timeseries
            WHERE symbol = $1 AND ts BETWEEN $2 AND $3
            ORDER BY ts ASC
            "#,
        )
        .bind(symbol)
        .bind(start)
        .bind(end)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)?;

        Ok(rows.into_iter().map(MarketData::from).collect())
    }

    /// 获取最新一条市场数据
    pub async fn latest(&self, symbol: &str) -> AlphaResult<Option<MarketData>> {
        let row = sqlx::query_as::<_, TimescaleRow>(
            r#"
            SELECT symbol, ts, price, volume, bid, ask, open, high, low
            FROM market_timeseries
            WHERE symbol = $1
            ORDER BY ts DESC
            LIMIT 1
            "#,
        )
        .bind(symbol)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;

        Ok(row.map(MarketData::from))
    }

    /// 统计指定符号的点位数量
    pub async fn count(&self, symbol: &str) -> AlphaResult<i64> {
        let count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*) FROM market_timeseries WHERE symbol = $1
            "#,
        )
        .bind(symbol)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db_error)?;

        Ok(count)
    }

    async fn ensure_schema(&self) -> AlphaResult<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS market_timeseries (
                symbol TEXT NOT NULL,
                ts TIMESTAMPTZ NOT NULL,
                price DOUBLE PRECISION NOT NULL,
                volume BIGINT NOT NULL,
                bid DOUBLE PRECISION,
                ask DOUBLE PRECISION,
                open DOUBLE PRECISION,
                high DOUBLE PRECISION,
                low DOUBLE PRECISION,
                PRIMARY KEY (symbol, ts)
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(map_db_error)?;

        if let Err(err) = sqlx::query(
            "SELECT create_hypertable('market_timeseries', 'ts', if_not_exists => TRUE);",
        )
        .execute(&self.pool)
        .await
        {
            debug!(
                "Timescale create_hypertable skipped (extension not available?): {}",
                err
            );
        }

        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct TimescaleRow {
    symbol: String,
    ts: DateTime<Utc>,
    price: f64,
    volume: i64,
    bid: Option<f64>,
    ask: Option<f64>,
    open: Option<f64>,
    high: Option<f64>,
    low: Option<f64>,
}

impl From<TimescaleRow> for MarketData {
    fn from(row: TimescaleRow) -> Self {
        Self {
            symbol: row.symbol,
            timestamp: row.ts,
            price: row.price,
            volume: if row.volume < 0 {
                0
            } else {
                u64::try_from(row.volume).unwrap_or(u64::MAX)
            },
            bid: row.bid,
            ask: row.ask,
            open: row.open,
            high: row.high,
            low: row.low,
        }
    }
}

fn map_db_error(err: sqlx::Error) -> AlphaError {
    AlphaError::StorageError(format!("TimescaleDB error: {}", err))
}

fn as_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database_url() -> Option<String> {
        std::env::var("TIMESCALE_TEST_URL").ok()
    }

    #[tokio::test]
    async fn connect_returns_error_without_db() {
        if database_url().is_some() {
            return;
        }

        let result = TimescaleTimeSeriesStorage::connect("postgres://invalid").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn insert_and_fetch_cycle() -> AlphaResult<()> {
        let Some(url) = database_url() else {
            // 未配置测试数据库时跳过
            return Ok(());
        };

        let storage = TimescaleTimeSeriesStorage::connect(&url).await?;
        let symbol = format!("TEST-{}", uuid::Uuid::new_v4());
        let data = MarketData {
            symbol: symbol.clone(),
            timestamp: Utc::now(),
            price: 123.45,
            volume: 100,
            bid: Some(123.4),
            ask: Some(123.5),
            open: Some(120.0),
            high: Some(124.0),
            low: Some(119.5),
        };
        storage.insert_market_data(&data).await?;

        let fetched = storage.latest(&symbol).await?;
        assert!(fetched.is_some());

        let count = storage.count(&symbol).await?;
        assert_eq!(count, 1);

        let range = storage
            .fetch_range(
                &symbol,
                data.timestamp - chrono::Duration::minutes(1),
                data.timestamp + chrono::Duration::minutes(1),
            )
            .await?;
        assert_eq!(range.len(), 1);

        Ok(())
    }
}
