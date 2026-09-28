//! 平台适配层抽象接口（跨平台架构 L0，见 docs/cross-platform-architecture.md §4）
//!
//! 平台差异（键值存储、本地文件、用户通知）收敛为本模块的 trait 面：Rust 业务代码
//! 面向 trait，不直接摸平台 API。实现放各交付面 crate 并依赖注入：
//!
//! * Desktop（Tauri）: `KeyValueStore` → 文件/SQLite、`LocalPersistence` → 原生 fs、
//!   `UserNotification` → Tauri notification 插件
//! * Web（wasm）: `KeyValueStore` → IndexedDB（经 wasm-bindgen 桥）、
//!   `LocalPersistence` → 下载导出、`UserNotification` → Notification API
//! * Android/iOS（规划）: 应用沙箱目录 + 系统推送（UniFFI/JNI 桥）
//! * 服务端参考实现：[`InMemoryKeyValueStore`]（亦用于测试与 wasm 单测回退）
//!
//! 约束：trait 方法均为 async 且不带平台类型；本模块零平台依赖，
//! wasm32 必须保持可编译（scripts/check-cross-platform.sh 门禁）。

use crate::errors::{AlphaError, AlphaResult};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;

/// 键值存储抽象：桌面文件/SQLite、Web IndexedDB、服务端 Redis 等的统一面。
#[async_trait]
pub trait KeyValueStore: Send + Sync {
    async fn get(&self, key: &str) -> AlphaResult<Option<Vec<u8>>>;
    async fn set(&self, key: &str, value: &[u8]) -> AlphaResult<()>;
    /// 删除键；键不存在时返回 Ok(false)，存在并删除返回 Ok(true)。
    async fn delete(&self, key: &str) -> AlphaResult<bool>;
}

/// 本地持久化抽象：桌面原生 fs、移动端应用沙箱目录、Web 侧下载导出。
#[async_trait]
pub trait LocalPersistence: Send + Sync {
    /// 导出文件到平台约定的用户可见位置（如桌面「另存为」、移动端文档目录）。
    async fn export_file(&self, name: &str, data: &[u8]) -> AlphaResult<()>;
}

/// 用户通知抽象：桌面系统托盘通知、移动端系统推送、Web Notification API。
#[async_trait]
pub trait UserNotification: Send + Sync {
    /// 发送一条用户通知；失败不致命，返回错误供调用方降级（如退化为应用内提示）。
    async fn notify(&self, title: &str, body: &str) -> AlphaResult<()>;
}

/// [`KeyValueStore`] 的进程内参考实现：HashMap + 互斥锁，键值后写覆盖。
/// 用于测试、wasm 单测回退与不需要持久化的场景；非持久化，重启即失。
#[derive(Default)]
pub struct InMemoryKeyValueStore {
    entries: Mutex<HashMap<String, Vec<u8>>>,
}

impl InMemoryKeyValueStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl KeyValueStore for InMemoryKeyValueStore {
    async fn get(&self, key: &str) -> AlphaResult<Option<Vec<u8>>> {
        let entries = self
            .entries
            .lock()
            .map_err(|_| AlphaError::ConfigurationError("key-value store poisoned".to_string()))?;
        Ok(entries.get(key).cloned())
    }

    async fn set(&self, key: &str, value: &[u8]) -> AlphaResult<()> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| AlphaError::ConfigurationError("key-value store poisoned".to_string()))?;
        entries.insert(key.to_string(), value.to_vec());
        Ok(())
    }

    async fn delete(&self, key: &str) -> AlphaResult<bool> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| AlphaError::ConfigurationError("key-value store poisoned".to_string()))?;
        Ok(entries.remove(key).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约测试：所有 KeyValueStore 实现都应满足 get/set/delete 后写覆盖语义。
    async fn assert_key_value_store_contract(store: impl KeyValueStore) {
        assert_eq!(store.get("k").await.unwrap(), None);
        assert!(
            !store.delete("k").await.unwrap(),
            "删除不存在的键应返回 false"
        );

        store.set("k", b"v1").await.unwrap();
        assert_eq!(store.get("k").await.unwrap(), Some(b"v1".to_vec()));

        store.set("k", b"v2").await.unwrap();
        assert_eq!(
            store.get("k").await.unwrap(),
            Some(b"v2".to_vec()),
            "后写覆盖"
        );

        assert!(store.delete("k").await.unwrap());
        assert_eq!(store.get("k").await.unwrap(), None);
    }

    #[tokio::test]
    async fn in_memory_store_satisfies_key_value_contract() {
        assert_key_value_store_contract(InMemoryKeyValueStore::new()).await;
    }

    /// 动态分发可用（业务代码经 Arc<dyn KeyValueStore> 注入平台实现）。
    #[tokio::test]
    async fn store_is_object_safe_and_thread_safe() {
        let store: std::sync::Arc<dyn KeyValueStore> =
            std::sync::Arc::new(InMemoryKeyValueStore::new());
        store.set("shared", b"1").await.unwrap();
        let clone = store.clone();
        let handle = tokio::spawn(async move { clone.get("shared").await.unwrap() });
        assert_eq!(handle.await.unwrap(), Some(b"1".to_vec()));
    }
}
