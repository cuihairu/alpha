//! Alpha Finance 存储层
//!
//! 提供统一的数据存储抽象层，支持多种存储后端

// 公共导出面（architecture-review §4.1 白名单化：只列实际消费面，
// 避免新代码顺手引用次级后端）：
//   cache / clickhouse / cloud / columnar / dal / dataset_registry / diagnosis /
//   encryption / memory / partition / prefetch / rate_limit / redis_kv(限流键) /
//   redis_streams / timescale(可选) / timeseries
// legacy 次级后端不进白名单：disk_kv（仓内零外部消费）、postgres_kv
// （单点豁免 PostgresKvStorage——api-gateway 账户持久层的既有消费面）
pub mod cache;
pub mod clickhouse;
pub mod cloud;
pub mod columnar;
pub mod dal;
pub mod dataset_registry;
pub mod diagnosis;
mod disk_kv;
pub mod encryption;
pub mod lake;
pub mod memory;
pub mod partition;
mod postgres_kv;
pub mod prefetch;
pub mod rate_limit;
pub mod redis_kv;
pub mod redis_streams;
pub mod timescale;
pub mod timeseries;

use alpha_core::errors::AlphaResult;

// 重新导出主要类型（白名单）
pub use cache::*;
pub use clickhouse::*;
pub use cloud::*;
pub use columnar::*;
pub use dal::*;
pub use dataset_registry::*;
pub use diagnosis::*;
pub use encryption::{decrypt_sensitive_field, encrypt_sensitive_field, EncryptedStorage};
pub use lake::*;
pub use memory::*;
pub use partition::*;
pub use postgres_kv::PostgresKvStorage;
pub use prefetch::*;
pub use rate_limit::*;
pub use redis_kv::*;
pub use redis_streams::*;
pub use timescale::*;
pub use timeseries::*;

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
                let backend =
                    RedisKvStorage::connect(&config.connection_string, config.ttl_seconds).await?;
                Ok(Box::new(backend))
            }
            StorageBackendType::S3 => {
                let backend = CloudStorage::from_connection_string(&config.connection_string)?;
                Ok(Box::new(backend))
            }
            StorageBackendType::LocalDisk => {
                let backend =
                    disk_kv::DiskKvStorage::from_connection_string(&config.connection_string)?;
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
            connection_string: "s3://alpha?provider=minio&endpoint=http%3A%2F%2F127.0.0.1%3A9000"
                .to_string(),
            ttl_seconds: None,
            max_connections: None,
        };

        let _storage = StorageFactory::create(config).await.unwrap();
    }
}
