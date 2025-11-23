//! 内存存储实现

use super::StorageBackend;
use alpha_core::errors::AlphaResult;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 内存存储
#[derive(Debug)]
pub struct MemoryStorage {
    data: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl MemoryStorage {
    /// 创建新的内存存储实例
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 创建带容量的内存存储
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::with_capacity(capacity))),
        }
    }
}

#[async_trait::async_trait]
impl StorageBackend for MemoryStorage {
    async fn store(&self, key: &str, value: Vec<u8>) -> AlphaResult<()> {
        let mut data = self.data.write().await;
        data.insert(key.to_string(), value);
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> AlphaResult<Option<Vec<u8>>> {
        let data = self.data.read().await;
        Ok(data.get(key).cloned())
    }

    async fn delete(&self, key: &str) -> AlphaResult<bool> {
        let mut data = self.data.write().await;
        Ok(data.remove(key).is_some())
    }

    async fn exists(&self, key: &str) -> AlphaResult<bool> {
        let data = self.data.read().await;
        Ok(data.contains_key(key))
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
        let mut data = self.data.write().await;
        data.clear();
        Ok(())
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestData {
        name: String,
        value: i32,
    }

    #[tokio::test]
    async fn test_memory_store_retrieve() {
        let storage = MemoryStorage::new();
        let test_data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        // 序列化并存储数据
        let serialized = bincode::serialize(&test_data).unwrap();
        storage.store("test_key", serialized).await.unwrap();

        // 检索数据
        let retrieved_bytes: Option<Vec<u8>> = storage.retrieve("test_key").await.unwrap();
        assert!(retrieved_bytes.is_some());

        let retrieved: TestData = bincode::deserialize(&retrieved_bytes.unwrap()).unwrap();
        assert_eq!(retrieved, test_data);
    }

    #[tokio::test]
    async fn test_memory_delete() {
        let storage = MemoryStorage::new();
        let test_data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        let serialized = bincode::serialize(&test_data).unwrap();
        storage.store("test_key", serialized).await.unwrap();
        assert!(storage.exists("test_key").await.unwrap());

        let deleted = storage.delete("test_key").await.unwrap();
        assert!(deleted);
        assert!(!storage.exists("test_key").await.unwrap());
    }

    #[tokio::test]
    async fn test_memory_list_keys() {
        let storage = MemoryStorage::new();

        storage
            .store("prefix_key1", b"value1".to_vec())
            .await
            .unwrap();
        storage
            .store("prefix_key2", b"value2".to_vec())
            .await
            .unwrap();
        storage
            .store("other_key", b"value3".to_vec())
            .await
            .unwrap();

        let keys = storage.list_keys("prefix_").await.unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"prefix_key1".to_string()));
        assert!(keys.contains(&"prefix_key2".to_string()));
    }
}
