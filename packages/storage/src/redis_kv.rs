//! Redis Key-Value 存储

use crate::StorageBackend;
use alpha_core::errors::{AlphaError, AlphaResult};
use redis::AsyncCommands;

#[derive(Clone)]
pub struct RedisKvStorage {
    conn: redis::aio::ConnectionManager,
    default_ttl_seconds: Option<u64>,
}

impl RedisKvStorage {
    pub async fn connect(connection_string: &str, default_ttl_seconds: Option<u64>) -> AlphaResult<Self> {
        let client = redis::Client::open(connection_string)
            .map_err(|e| AlphaError::ConfigurationError(format!("invalid redis URL: {e}")))?;
        let conn = client
            .get_connection_manager()
            .await
            .map_err(|e| AlphaError::StorageError(format!("redis connect failed: {e}")))?;

        Ok(Self {
            conn,
            default_ttl_seconds,
        })
    }

    fn ttl_seconds(&self) -> Option<u64> {
        self.default_ttl_seconds
    }
}

#[async_trait::async_trait]
impl StorageBackend for RedisKvStorage {
    async fn store(&self, key: &str, value: Vec<u8>) -> AlphaResult<()> {
        let mut conn = self.conn.clone();
        if let Some(ttl) = self.ttl_seconds() {
            conn.set_ex::<_, _, ()>(key, value, ttl)
                .await
                .map_err(|e| AlphaError::StorageError(format!("redis SETEX failed: {e}")))?;
        } else {
            conn.set::<_, _, ()>(key, value)
                .await
                .map_err(|e| AlphaError::StorageError(format!("redis SET failed: {e}")))?;
        }
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> AlphaResult<Option<Vec<u8>>> {
        let mut conn = self.conn.clone();
        let val: Option<Vec<u8>> = conn
            .get(key)
            .await
            .map_err(|e| AlphaError::StorageError(format!("redis GET failed: {e}")))?;
        Ok(val)
    }

    async fn delete(&self, key: &str) -> AlphaResult<bool> {
        let mut conn = self.conn.clone();
        let deleted: u64 = conn
            .del(key)
            .await
            .map_err(|e| AlphaError::StorageError(format!("redis DEL failed: {e}")))?;
        Ok(deleted > 0)
    }

    async fn exists(&self, key: &str) -> AlphaResult<bool> {
        let mut conn = self.conn.clone();
        let exists: bool = conn
            .exists(key)
            .await
            .map_err(|e| AlphaError::StorageError(format!("redis EXISTS failed: {e}")))?;
        Ok(exists)
    }

    async fn list_keys(&self, prefix: &str) -> AlphaResult<Vec<String>> {
        let mut conn = self.conn.clone();
        let pattern = format!("{prefix}*");

        let mut cursor: u64 = 0;
        let mut keys: Vec<String> = Vec::new();

        loop {
            let (next_cursor, batch): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(1000u64)
                .query_async(&mut conn)
                .await
                .map_err(|e| AlphaError::StorageError(format!("redis SCAN failed: {e}")))?;

            keys.extend(batch);
            cursor = next_cursor;
            if cursor == 0 {
                break;
            }
        }

        keys.sort();
        keys.dedup();
        Ok(keys)
    }

    async fn clear(&self) -> AlphaResult<()> {
        let mut conn = self.conn.clone();
        redis::cmd("FLUSHDB")
            .query_async::<_, ()>(&mut conn)
            .await
            .map_err(|e| AlphaError::StorageError(format!("redis FLUSHDB failed: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redis_url() -> Option<String> {
        std::env::var("REDIS_TEST_URL").ok()
    }

    #[tokio::test]
    async fn connect_returns_error_without_redis() {
        if redis_url().is_some() {
            return;
        }

        let result = RedisKvStorage::connect("redis://127.0.0.1:1", None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn redis_kv_roundtrip_with_optional_ttl() -> AlphaResult<()> {
        let Some(url) = redis_url() else {
            return Ok(());
        };

        let prefix = format!("alpha:test:{}:", uuid::Uuid::new_v4());
        let storage = RedisKvStorage::connect(&url, Some(60)).await?;
        let key = format!("{prefix}quote");

        storage.store(&key, b"hello".to_vec()).await?;
        assert!(storage.exists(&key).await?);
        assert_eq!(storage.retrieve(&key).await?, Some(b"hello".to_vec()));

        let keys = storage.list_keys(&prefix).await?;
        assert_eq!(keys, vec![key.clone()]);

        assert!(storage.delete(&key).await?);
        assert!(!storage.exists(&key).await?);

        Ok(())
    }
}
