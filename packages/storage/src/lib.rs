//! Alpha Finance 存储层
//!
//! 提供统一的数据存储抽象层，支持多种存储后端

pub mod memory;
pub mod timeseries;
pub mod timescale;
pub mod clickhouse;

use alpha_core::errors::AlphaResult;

// 重新导出主要类型
pub use memory::*;
pub use timeseries::*;
pub use timescale::*;
pub use clickhouse::*;

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
                // TODO: 实现 PostgreSQL 存储后端
                todo!("PostgreSQL 存储后端未实现")
            }
            StorageBackendType::Redis => {
                // TODO: 实现 Redis 存储后端
                todo!("Redis 存储后端未实现")
            }
            StorageBackendType::S3 => {
                // TODO: 实现 S3 存储后端
                todo!("S3 存储后端未实现")
            }
            StorageBackendType::LocalDisk => {
                // TODO: 实现本地磁盘存储后端
                todo!("本地磁盘存储后端未实现")
            }
        }
    }
}
