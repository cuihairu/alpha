//! 分布式缓存（L446）：Redis 上的 JSON cache-aside 面。
//!
//! 与 [`crate::RedisKvStorage`]（通用字节 KV）分层：本模块面向「读多写少的
//! 热点对象」——类型化 get/set、get_or_load 旁路装载、命中/未命中计数。

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use alpha_core::errors::{AlphaError, AlphaResult};
use redis::AsyncCommands;
use serde::de::DeserializeOwned;
use serde::Serialize;

/// 缓存键 = 命名空间前缀 + 业务键（拼接口径纯函数化，单测锁定）
pub fn cache_key(prefix: &str, key: &str) -> String {
    format!("{prefix}{key}")
}

#[derive(Clone)]
pub struct DistributedCache {
    conn: redis::aio::ConnectionManager,
    prefix: String,
    default_ttl: Duration,
    hits: Arc<AtomicU64>,
    misses: Arc<AtomicU64>,
}

impl DistributedCache {
    pub async fn connect(
        connection_string: &str,
        prefix: &str,
        default_ttl: Duration,
    ) -> AlphaResult<Self> {
        let client = redis::Client::open(connection_string)
            .map_err(|e| AlphaError::ConfigurationError(format!("invalid redis URL: {e}")))?;
        let conn = client
            .get_connection_manager()
            .await
            .map_err(|e| AlphaError::StorageError(format!("redis connect failed: {e}")))?;

        Ok(Self {
            conn,
            prefix: prefix.to_string(),
            default_ttl,
            hits: Arc::new(AtomicU64::new(0)),
            misses: Arc::new(AtomicU64::new(0)),
        })
    }

    /// 类型化读取：键不存在 → `Ok(None)`；存在但反序列化失败 → 错误（脏数据不静默吞）。
    pub async fn get_json<T: DeserializeOwned>(&self, key: &str) -> AlphaResult<Option<T>> {
        let mut conn = self.conn.clone();
        let raw: Option<String> = conn
            .get(cache_key(&self.prefix, key))
            .await
            .map_err(|e| AlphaError::StorageError(format!("cache GET failed: {e}")))?;
        match raw {
            Some(s) => {
                self.hits.fetch_add(1, Ordering::Relaxed);
                serde_json::from_str(&s)
                    .map(Some)
                    .map_err(|e| AlphaError::StorageError(format!("cache 值反序列化失败: {e}")))
            }
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                Ok(None)
            }
        }
    }

    /// 类型化写入：ttl 缺省用 connect 时的 default_ttl。
    pub async fn set_json<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
    ) -> AlphaResult<()> {
        let mut conn = self.conn.clone();
        let payload = serde_json::to_string(value)
            .map_err(|e| AlphaError::StorageError(format!("cache 值序列化失败: {e}")))?;
        let ttl = ttl.unwrap_or(self.default_ttl);
        conn.set_ex::<_, _, ()>(cache_key(&self.prefix, key), payload, ttl.as_secs())
            .await
            .map_err(|e| AlphaError::StorageError(format!("cache SETEX failed: {e}")))?;
        Ok(())
    }

    pub async fn delete(&self, key: &str) -> AlphaResult<bool> {
        let mut conn = self.conn.clone();
        let deleted: u64 = conn
            .del(cache_key(&self.prefix, key))
            .await
            .map_err(|e| AlphaError::StorageError(format!("cache DEL failed: {e}")))?;
        Ok(deleted > 0)
    }

    /// cache-aside 装载：命中即返回；未命中执行 `load`、回填（ttl 缺省）后返回。
    /// 装载失败不写缓存（坏值不入湖），错误原样上抛。
    pub async fn get_or_load<T, F, Fut>(
        &self,
        key: &str,
        ttl: Option<Duration>,
        load: F,
    ) -> AlphaResult<T>
    where
        T: DeserializeOwned + Serialize,
        F: FnOnce() -> Fut,
        Fut: Future<Output = AlphaResult<T>>,
    {
        if let Some(cached) = self.get_json::<T>(key).await? {
            return Ok(cached);
        }
        let value = load().await?;
        self.set_json(key, &value, ttl).await?;
        Ok(value)
    }

    pub fn hits(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }

    pub fn misses(&self) -> u64 {
        self.misses.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[test]
    fn cache_key_concatenates_prefix_and_biz_key() {
        assert_eq!(
            cache_key("alpha:cache:", "quote:600519"),
            "alpha:cache:quote:600519"
        );
        assert_eq!(cache_key("", "k"), "k");
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Quote {
        symbol: String,
        price: f64,
    }

    fn redis_url() -> Option<String> {
        std::env::var("REDIS_TEST_URL").ok()
    }

    /// 集成（REDIS_TEST_URL 门控）：roundtrip / get_or_load 单次装载 / 删除 / 计数
    #[tokio::test]
    async fn cache_roundtrip_and_aside_loading() -> AlphaResult<()> {
        let Some(url) = redis_url() else {
            return Ok(());
        };
        let prefix = format!("alpha:test:cache:{}:", uuid::Uuid::new_v4());
        let cache = DistributedCache::connect(&url, &prefix, Duration::from_secs(60)).await?;

        let quote = Quote {
            symbol: "600519".into(),
            price: 90.5,
        };
        // 未命中：键不存在 → None 且 miss 计数 +1
        assert_eq!(cache.get_json::<Quote>("quote:600519").await?, None);
        assert_eq!(cache.misses(), 1);

        cache.set_json("quote:600519", &quote, None).await?;
        assert_eq!(cache.get_json::<Quote>("quote:600519").await?, Some(quote));
        assert_eq!(cache.hits(), 1);

        // cache-aside：loader 只在未命中时执行；回填后第二次读取命中缓存值
        // （loader 产出哨兵值——若被调用，断言即失败，无需外部计数）
        let first = Quote {
            symbol: "000001".into(),
            price: 10.2,
        };
        let v = cache
            .get_or_load("quote:000001", None, || {
                let loaded = first.clone();
                async move { Ok::<_, AlphaError>(loaded) }
            })
            .await?;
        assert_eq!(v.symbol, "000001");
        let v2 = cache
            .get_or_load("quote:000001", None, || async {
                Ok::<_, AlphaError>(Quote {
                    symbol: "SHOULD_NOT_LOAD".into(),
                    price: -1.0,
                })
            })
            .await?;
        assert_eq!(v2, v, "第二次读取应命中缓存而非 loader 哨兵值");

        assert!(cache.delete("quote:600519").await?);
        assert_eq!(cache.get_json::<Quote>("quote:600519").await?, None);

        Ok(())
    }

    #[tokio::test]
    async fn connect_rejects_invalid_url() {
        if redis_url().is_some() {
            return;
        }
        assert!(
            DistributedCache::connect("redis://127.0.0.1:1", "p:", Duration::from_secs(1))
                .await
                .is_err()
        );
    }
}
