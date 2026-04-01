//! PostgreSQL Key-Value 存储

use crate::StorageBackend;
use alpha_core::errors::{AlphaError, AlphaResult};
use sqlx::{postgres::PgPoolOptions, PgPool};

fn escape_like_prefix(prefix: &str) -> String {
    let mut out = String::with_capacity(prefix.len() + 8);
    for ch in prefix.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '%' => out.push_str("\\%"),
            '_' => out.push_str("\\_"),
            _ => out.push(ch),
        }
    }
    out
}

#[derive(Clone)]
pub struct PostgresKvStorage {
    pool: PgPool,
    table: String,
    default_ttl_seconds: Option<u64>,
}

impl PostgresKvStorage {
    pub async fn connect(
        database_url: &str,
        table: &str,
        max_connections: Option<u32>,
        default_ttl_seconds: Option<u64>,
    ) -> AlphaResult<Self> {
        let mut opts = PgPoolOptions::new();
        if let Some(max) = max_connections {
            opts = opts.max_connections(max);
        }

        let pool = opts
            .connect(database_url)
            .await
            .map_err(|e| AlphaError::StorageError(format!("Postgres connect failed: {e}")))?;

        let storage = Self {
            pool,
            table: table.to_string(),
            default_ttl_seconds,
        };
        storage.ensure_schema().await?;
        Ok(storage)
    }

    async fn ensure_schema(&self) -> AlphaResult<()> {
        let ddl = format!(
            r#"
            CREATE TABLE IF NOT EXISTS {table} (
                key TEXT PRIMARY KEY,
                value BYTEA NOT NULL,
                expires_at TIMESTAMPTZ NULL
            );
            CREATE INDEX IF NOT EXISTS {table}_expires_at_idx ON {table}(expires_at);
            "#,
            table = self.table
        );

        sqlx::query(&ddl)
            .execute(&self.pool)
            .await
            .map_err(|e| AlphaError::StorageError(format!("Postgres schema init failed: {e}")))?;
        Ok(())
    }

    fn ttl_i64(&self) -> Option<i64> {
        self.default_ttl_seconds
            .and_then(|v| i64::try_from(v).ok())
    }
}

#[async_trait::async_trait]
impl StorageBackend for PostgresKvStorage {
    async fn store(&self, key: &str, value: Vec<u8>) -> AlphaResult<()> {
        let ttl = self.ttl_i64();
        let sql = format!(
            r#"
            INSERT INTO {table} (key, value, expires_at)
            VALUES ($1, $2, CASE WHEN $3 IS NULL THEN NULL ELSE now() + ($3 * interval '1 second') END)
            ON CONFLICT (key)
            DO UPDATE SET value = EXCLUDED.value, expires_at = EXCLUDED.expires_at
            "#,
            table = self.table
        );

        sqlx::query(&sql)
            .bind(key)
            .bind(value)
            .bind(ttl)
            .execute(&self.pool)
            .await
            .map_err(|e| AlphaError::StorageError(format!("Postgres store failed: {e}")))?;
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> AlphaResult<Option<Vec<u8>>> {
        let sql = format!(
            r#"
            SELECT value
            FROM {table}
            WHERE key = $1 AND (expires_at IS NULL OR expires_at > now())
            "#,
            table = self.table
        );

        let row = sqlx::query_scalar::<_, Vec<u8>>(&sql)
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AlphaError::StorageError(format!("Postgres retrieve failed: {e}")))?;

        Ok(row)
    }

    async fn delete(&self, key: &str) -> AlphaResult<bool> {
        let sql = format!("DELETE FROM {table} WHERE key = $1", table = self.table);
        let res = sqlx::query(&sql)
            .bind(key)
            .execute(&self.pool)
            .await
            .map_err(|e| AlphaError::StorageError(format!("Postgres delete failed: {e}")))?;
        Ok(res.rows_affected() > 0)
    }

    async fn exists(&self, key: &str) -> AlphaResult<bool> {
        let sql = format!(
            r#"
            SELECT EXISTS(
              SELECT 1 FROM {table}
              WHERE key = $1 AND (expires_at IS NULL OR expires_at > now())
            )
            "#,
            table = self.table
        );
        let exists = sqlx::query_scalar::<_, bool>(&sql)
            .bind(key)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AlphaError::StorageError(format!("Postgres exists failed: {e}")))?;
        Ok(exists)
    }

    async fn list_keys(&self, prefix: &str) -> AlphaResult<Vec<String>> {
        let like = format!("{}%", escape_like_prefix(prefix));
        let sql = format!(
            r#"
            SELECT key
            FROM {table}
            WHERE key LIKE $1 ESCAPE '\' AND (expires_at IS NULL OR expires_at > now())
            ORDER BY key ASC
            "#,
            table = self.table
        );
        let keys = sqlx::query_scalar::<_, String>(&sql)
            .bind(like)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AlphaError::StorageError(format!("Postgres list_keys failed: {e}")))?;
        Ok(keys)
    }

    async fn clear(&self) -> AlphaResult<()> {
        let sql = format!("TRUNCATE TABLE {table}", table = self.table);
        sqlx::query(&sql)
            .execute(&self.pool)
            .await
            .map_err(|e| AlphaError::StorageError(format!("Postgres clear failed: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database_url() -> Option<String> {
        std::env::var("POSTGRES_TEST_URL")
            .ok()
            .or_else(|| std::env::var("TIMESCALE_TEST_URL").ok())
    }

    #[tokio::test]
    async fn connect_returns_error_without_postgres() {
        if database_url().is_some() {
            return;
        }

        let result = PostgresKvStorage::connect("postgres://invalid", "alpha_kv_test", Some(1), None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn postgres_kv_roundtrip_and_prefix_listing() -> AlphaResult<()> {
        let Some(url) = database_url() else {
            return Ok(());
        };

        let table = format!("alpha_kv_test_{}", uuid::Uuid::new_v4().simple());
        let storage = PostgresKvStorage::connect(&url, &table, Some(2), Some(60)).await?;

        storage.store("quotes/AAPL", b"100".to_vec()).await?;
        storage.store("quotes/MSFT", b"200".to_vec()).await?;
        storage.store("trades/AAPL", b"300".to_vec()).await?;

        assert_eq!(storage.retrieve("quotes/AAPL").await?, Some(b"100".to_vec()));
        assert!(storage.exists("quotes/MSFT").await?);

        let keys = storage.list_keys("quotes/").await?;
        assert_eq!(
            keys,
            vec!["quotes/AAPL".to_string(), "quotes/MSFT".to_string()]
        );

        assert!(storage.delete("quotes/MSFT").await?);
        assert!(!storage.exists("quotes/MSFT").await?);

        storage.clear().await?;
        assert_eq!(storage.list_keys("").await?, Vec::<String>::new());

        Ok(())
    }
}
