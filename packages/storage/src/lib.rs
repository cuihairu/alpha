//! Alpha Finance 存储层
//!
//! 提供统一的数据存储抽象层，支持多种存储后端

pub mod memory;
pub mod timeseries;
pub mod timescale;
pub mod clickhouse;
pub mod cloud;
pub mod dal;
pub mod disk_kv;
pub mod postgres_kv;
pub mod redis_kv;

use alpha_core::errors::AlphaResult;

// 重新导出主要类型
pub use memory::*;
pub use timeseries::*;
pub use timescale::*;
pub use clickhouse::*;
pub use cloud::*;
pub use dal::*;
pub use disk_kv::*;
pub use postgres_kv::*;
pub use redis_kv::*;

/// 存储后端特征（对象安全版本）
#[async_trait::async_trait]
pub trait StorageBackend: Send + Sync {
    async fn store(&self, key: &str, value: Vec<u8>) -> AlphaResult<()>;
    async fn retrieve(&self, key: &str) -> AlphaResult<Option<Vec<u8>>>;
    async fn delete(&self, key: &str) -> AlphaResult<bool>;
    async fn exists(&self, key: &str) -> AlphaResult<bool>;
    async fn list_keys(&self, prefix: &str) -> AlphaResult<Vec<String>>;
    async fn clear(&self) -> AlphaResult<()>;
}

/// 存储配置
#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub backend: StorageBackendType,
    pub connection_string: String,
    pub ttl_seconds: Option<u64>,
    pub max_connections: Option<u32>,
}

/// 存储后端类型
#[derive(Debug, Clone)]
pub enum StorageBackendType {
    Memory,
    Postgres,
    Redis,
    S3,
    LocalDisk,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            backend: StorageBackendType::Memory,
            connection_string: "memory://".to_string(),
            ttl_seconds: None,
            max_connections: None,
        }
    }
}

/// 存储工厂
pub struct StorageFactory;

impl StorageFactory {
    /// 根据配置创建存储后端
    pub async fn create(config: StorageConfig) -> AlphaResult<Box<dyn StorageBackend>> {
        match config.backend {
            StorageBackendType::Memory => Ok(Box::new(MemoryStorage::new())),
            StorageBackendType::Postgres => {
                let backend = PostgresKvStorage::connect(
                    &config.connection_string,
                    "alpha_kv",
                    config.max_connections,
                    config.ttl_seconds,
                )
                .await?;
                Ok(Box::new(backend))
            }
            StorageBackendType::Redis => {
                let backend = RedisKvStorage::connect(&config.connection_string, config.ttl_seconds).await?;
                Ok(Box::new(backend))
            }
            StorageBackendType::S3 => {
                let backend = CloudStorage::from_connection_string(&config.connection_string)?;
                Ok(Box::new(backend))
            }
            StorageBackendType::LocalDisk => {
                let backend = DiskKvStorage::from_connection_string(&config.connection_string)?;
                Ok(Box::new(backend))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn storage_factory_creates_local_disk_backend_from_connection_string() {
        let tmp = TempDir::new().unwrap();
        let config = StorageConfig {
            backend: StorageBackendType::LocalDisk,
            connection_string: format!("file://{}", tmp.path().display()),
            ttl_seconds: None,
            max_connections: None,
        };

        let storage = StorageFactory::create(config).await.unwrap();
        storage
            .store("factory/test", b"value".to_vec())
            .await
            .unwrap();

        assert_eq!(
            storage.retrieve("factory/test").await.unwrap(),
            Some(b"value".to_vec())
        );
    }

    #[tokio::test]
    async fn storage_factory_creates_s3_backend_from_connection_string() {
        let config = StorageConfig {
            backend: StorageBackendType::S3,
            connection_string:
                "s3://alpha?provider=minio&endpoint=http%3A%2F%2F127.0.0.1%3A9000".to_string(),
            ttl_seconds: None,
            max_connections: None,
        };

        let _storage = StorageFactory::create(config).await.unwrap();
    }
}
