//! 缓存层实现

use super::StorageBackend;
use alpha_core::errors::{AlphaError, AlphaResult};
use serde::{de::DeserializeOwned, Serialize};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// 缓存条目
#[derive(Debug)]
struct CacheEntry<T> {
    value: T,
    expires_at: Option<Instant>,
}

impl<T> CacheEntry<T> {
    fn new(value: T, ttl: Option<Duration>) -> Self {
        Self {
            value,
            expires_at: ttl.map(|duration| Instant::now() + duration),
        }
    }

    fn is_expired(&self) -> bool {
        self.expires_at
            .map(|expires_at| Instant::now() > expires_at)
            .unwrap_or(false)
    }
}

/// 内存缓存
#[derive(Debug)]
pub struct MemoryCache<T> {
    data: RwLock<std::collections::HashMap<String, CacheEntry<T>>>,
    default_ttl: Option<Duration>,
    max_size: usize,
}

impl<T> MemoryCache<T>
where
    T: Clone + Send + Sync + 'static,
{
    /// 创建新的缓存实例
    pub fn new() -> Self {
        Self {
            data: RwLock::new(std::collections::HashMap::new()),
            default_ttl: None,
            max_size: 1000,
        }
    }

    /// 创建带 TTL 的缓存实例
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            data: RwLock::new(std::collections::HashMap::new()),
            default_ttl: Some(ttl),
            max_size: 1000,
        }
    }

    /// 创建带 TTL 和最大大小的缓存实例
    pub fn with_config(ttl: Option<Duration>, max_size: usize) -> Self {
        Self {
            data: RwLock::new(std::collections::HashMap::new()),
            default_ttl: ttl,
            max_size,
        }
    }

    /// 设置缓存值
    pub async fn set(&self, key: &str, value: T, ttl: Option<Duration>) -> AlphaResult<()> {
        let ttl = ttl.or(self.default_ttl);
        let entry = CacheEntry::new(value, ttl);

        let mut data = self.data.write().await;

        // 如果缓存已满，删除最旧的条目
        if data.len() >= self.max_size {
            // 简单策略：移除第一个条目
            // 在实际应用中，可以使用 LRU 等更智能的策略
            if let Some(first_key) = data.keys().next().cloned() {
                data.remove(&first_key);
            }
        }

        data.insert(key.to_string(), entry);
        Ok(())
    }

    /// 获取缓存值
    pub async fn get(&self, key: &str) -> AlphaResult<Option<T>> {
        let mut data = self.data.write().await;

        match data.get(key) {
            Some(entry) if !entry.is_expired() => {
                Ok(Some(entry.value.clone()))
            }
            Some(_) => {
                // 过期了，删除条目
                data.remove(key);
                Ok(None)
            }
            None => Ok(None),
        }
    }

    /// 删除缓存值
    pub async fn delete(&self, key: &str) -> AlphaResult<bool> {
        let mut data = self.data.write().await;
        Ok(data.remove(key).is_some())
    }

    /// 检查键是否存在且未过期
    pub async fn exists(&self, key: &str) -> AlphaResult<bool> {
        let mut data = self.data.write().await;

        match data.get(key) {
            Some(entry) => {
                if entry.is_expired() {
                    data.remove(key);
                    Ok(false)
                } else {
                    Ok(true)
                }
            }
            None => Ok(false),
        }
    }

    /// 清理过期的条目
    pub async fn cleanup_expired(&self) -> AlphaResult<usize> {
        let mut data = self.data.write().await;
        let initial_count = data.len();

        data.retain(|_, entry| !entry.is_expired());

        Ok(initial_count - data.len())
    }

    /// 清空所有缓存
    pub async fn clear(&self) -> AlphaResult<()> {
        let mut data = self.data.write().await;
        data.clear();
        Ok(())
    }

    /// 获取缓存大小
    pub async fn size(&self) -> AlphaResult<usize> {
        let data = self.data.read().await;
        Ok(data.len())
    }
}

impl<T> Default for MemoryCache<T>
where
    T: Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl StorageBackend for MemoryCache<String> {
    type Error = Box<dyn std::error::Error + Send + Sync>;

    async fn store<U>(&self, key: &str, value: &U) -> AlphaResult<()>
    where
        U: serde::Serialize + Send + Sync,
    {
        let serialized = serde_json::to_string(value)
            .map_err(|e| AlphaError::SerializationError(e.to_string()))?;

        self.set(key, serialized, None).await
    }

    async fn retrieve<U>(&self, key: &str) -> AlphaResult<Option<U>>
    where
        U: for<'de> serde::Deserialize<'de> + Send + Sync,
    {
        match self.get(key).await? {
            Some(serialized) => {
                let value: U = serde_json::from_str(&serialized)
                    .map_err(|e| AlphaError::SerializationError(e.to_string()))?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    async fn delete(&self, key: &str) -> AlphaResult<bool> {
        MemoryCache::delete(self, key).await
    }

    async fn exists(&self, key: &str) -> AlphaResult<bool> {
        MemoryCache::exists(self, key).await
    }

    async fn list_keys(&self, prefix: &str) -> AlphaResult<Vec<String>> {
        let data = self.data.read().await;
        let keys: Vec<String> = data
            .keys()
            .filter(|key| key.starts_with(prefix))
            .cloned()
            .collect();

        Ok(keys)
    }

    async fn clear(&self) -> AlphaResult<()> {
        MemoryCache::clear(self).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_cache_basic_operations() {
        let cache: MemoryCache<String> = MemoryCache::new();

        cache.set("key1", "value1".to_string(), None).await.unwrap();
        assert_eq!(cache.get("key1").await.unwrap(), Some("value1".to_string()));

        cache.delete("key1").await.unwrap();
        assert_eq!(cache.get("key1").await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_cache_ttl() {
        let cache: MemoryCache<String> = MemoryCache::with_ttl(Duration::from_millis(100));

        cache.set("key1", "value1".to_string(), None).await.unwrap();
        assert_eq!(cache.get("key1").await.unwrap(), Some("value1".to_string()));

        // 等待过期
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(cache.get("key1").await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_cache_custom_ttl() {
        let cache: MemoryCache<String> = MemoryCache::with_ttl(Duration::from_millis(100));

        // 使用自定义 TTL
        cache.set("key1", "value1".to_string(), Some(Duration::from_millis(200))).await.unwrap();

        // 默认 TTL 应该过期
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(cache.get("key1").await.unwrap(), Some("value1".to_string()));

        // 自定义 TTL 应该过期
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(cache.get("key1").await.unwrap(), None);
    }
}