//! 磁盘存储实现

use super::StorageBackend;
use alpha_core::errors::{AlphaError, AlphaResult};
use serde::{de::DeserializeOwned, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;

/// 本地磁盘存储
#[derive(Debug)]
pub struct DiskStorage {
    base_path: PathBuf,
}

impl DiskStorage {
    /// 创建新的磁盘存储实例
    pub fn new<P: AsRef<Path>>(base_path: P) -> Self {
        Self {
            base_path: base_path.as_ref().to_path_buf(),
        }
    }

    /// 确保目录存在
    async fn ensure_dir(&self) -> AlphaResult<()> {
        if !self.base_path.exists() {
            fs::create_dir_all(&self.base_path).await
                .map_err(|e| AlphaError::StorageError(format!("Failed to create directory: {}", e)))?;
        }
        Ok(())
    }

    /// 获取文件的完整路径
    fn get_file_path(&self, key: &str) -> PathBuf {
        // 简单地将键转换为文件路径，实际应用中可能需要更复杂的策略
        let file_name = format!("{}.bin", key.replace('/', "_"));
        self.base_path.join(file_name)
    }
}

#[async_trait::async_trait]
impl StorageBackend for DiskStorage {
    type Error = Box<dyn std::error::Error + Send + Sync>;

    async fn store<T>(&self, key: &str, value: &T) -> AlphaResult<()>
    where
        T: serde::Serialize + Send + Sync,
    {
        self.ensure_dir().await?;

        let serialized = bincode::serialize(value)
            .map_err(|e| AlphaError::SerializationError(e.to_string()))?;

        let file_path = self.get_file_path(key);

        let mut file = fs::File::create(&file_path).await
            .map_err(|e| AlphaError::StorageError(format!("Failed to create file: {}", e)))?;

        file.write_all(&serialized).await
            .map_err(|e| AlphaError::StorageError(format!("Failed to write file: {}", e)))?;

        Ok(())
    }

    async fn retrieve<T>(&self, key: &str) -> AlphaResult<Option<T>>
    where
        T: for<'de> serde::Deserialize<'de> + Send + Sync,
    {
        let file_path = self.get_file_path(key);

        if !file_path.exists() {
            return Ok(None);
        }

        let contents = fs::read(&file_path).await
            .map_err(|e| AlphaError::StorageError(format!("Failed to read file: {}", e)))?;

        let value: T = bincode::deserialize(&contents)
            .map_err(|e| AlphaError::SerializationError(e.to_string()))?;

        Ok(Some(value))
    }

    async fn delete(&self, key: &str) -> AlphaResult<bool> {
        let file_path = self.get_file_path(key);

        if file_path.exists() {
            fs::remove_file(&file_path).await
                .map_err(|e| AlphaError::StorageError(format!("Failed to remove file: {}", e)))?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn exists(&self, key: &str) -> AlphaResult<bool> {
        let file_path = self.get_file_path(key);
        Ok(file_path.exists())
    }

    async fn list_keys(&self, prefix: &str) -> AlphaResult<Vec<String>> {
        self.ensure_dir().await?;

        let mut entries = fs::read_dir(&self.base_path).await
            .map_err(|e| AlphaError::StorageError(format!("Failed to read directory: {}", e)))?;

        let mut keys = Vec::new();
        let search_prefix = format!("{}.bin", prefix.replace('/', "_"));

        while let Some(entry) = entries.next_entry().await
            .map_err(|e| AlphaError::StorageError(format!("Failed to read directory entry: {}", e)))?
        {
            let file_name = entry.file_name();
            if let Some(name_str) = file_name.to_str() {
                if name_str.starts_with(&search_prefix) {
                    // 移除 .bin 后缀并替换回来
                    let key = name_str
                        .strip_suffix(".bin")
                        .unwrap()
                        .replace('_', "/");
                    keys.push(key);
                }
            }
        }

        Ok(keys)
    }

    async fn clear(&self) -> AlphaResult<()> {
        if self.base_path.exists() {
            fs::remove_dir_all(&self.base_path).await
                .map_err(|e| AlphaError::StorageError(format!("Failed to remove directory: {}", e)))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestData {
        name: String,
        value: i32,
    }

    #[tokio::test]
    async fn test_disk_storage() {
        let temp_dir = TempDir::new().unwrap();
        let storage = DiskStorage::new(temp_dir.path());

        let test_data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        // 存储数据
        storage.store("test_key", &test_data).await.unwrap();

        // 检索数据
        let retrieved: Option<TestData> = storage.retrieve("test_key").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), test_data);
    }

    #[tokio::test]
    async fn test_disk_storage_nested_keys() {
        let temp_dir = TempDir::new().unwrap();
        let storage = DiskStorage::new(temp_dir.path());

        let test_data = TestData {
            name: "nested".to_string(),
            value: 100,
        };

        // 存储带嵌套键的数据
        storage.store("folder/subfolder/key", &test_data).await.unwrap();

        // 检索数据
        let retrieved: Option<TestData> = storage.retrieve("folder/subfolder/key").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap(), test_data);
    }
}