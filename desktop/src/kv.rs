//! 文件键值存储：`alpha_core::platform::KeyValueStore` 的桌面实现
//!
//! TODO L38（跨平台适配层）只给了 trait 面与说明（"Desktop（Tauri）: KeyValueStore
//! → 文件/SQLite"），本模块把该说明落成可测实现：每个键一个文件，值为原始字节。
//!
//! 安全口径：文件名由键的**十六进制编码**派生（[`key_filename`]），不使用键原文，
//! 因此 `../../etc/passwd` 这类键只会落到目录内的长十六进制文件名，天然阻断路径穿越。

use alpha_core::errors::{AlphaError, AlphaResult};
use alpha_core::platform::KeyValueStore;
use async_trait::async_trait;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// 键 → 文件名：逐字节十六进制，空格/分隔符/多字节均无歧义，长度 2×字节数
pub fn key_filename(key: &str) -> String {
    let mut name = String::with_capacity(key.len() * 2);
    for byte in key.as_bytes() {
        // 十六进制数字表写入不会失败
        let _ = write!(name, "{byte:02x}");
    }
    name
}

/// 基于目录的文件键值存储（`&self` 方法，线程安全要求由 `Mutex` 无关——每次调用独立打开文件）
#[derive(Debug, Clone)]
pub struct FileKeyValueStore {
    dir: PathBuf,
}

impl FileKeyValueStore {
    /// 指向已存在的目录（调用方负责建目录，见 [`crate::paths::AppPaths::ensure`]）
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// 存储根目录
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// 键对应的文件路径（空键直接判非法，不返回路径）
    fn path_for(&self, key: &str) -> AlphaResult<PathBuf> {
        if key.is_empty() {
            return Err(AlphaError::invalid_input("键不能为空"));
        }
        Ok(self.dir.join(key_filename(key)))
    }
}

#[async_trait]
impl KeyValueStore for FileKeyValueStore {
    async fn get(&self, key: &str) -> AlphaResult<Option<Vec<u8>>> {
        let path = self.path_for(key)?;
        match std::fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            // 键不存在 = 正常未命中，不算错误
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(AlphaError::StorageError(format!("读取键 {key} 失败: {e}"))),
        }
    }

    async fn set(&self, key: &str, value: &[u8]) -> AlphaResult<()> {
        let path = self.path_for(key)?;
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| AlphaError::StorageError(format!("准备键 {key} 的存储目录失败: {e}")))?;
        std::fs::write(&path, value)
            .map_err(|e| AlphaError::StorageError(format!("写入键 {key} 失败: {e}")))?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> AlphaResult<bool> {
        let path = self.path_for(key)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(AlphaError::StorageError(format!("删除键 {key} 失败: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(dir: &Path) -> FileKeyValueStore {
        std::fs::create_dir_all(dir).expect("建存储目录");
        FileKeyValueStore::new(dir)
    }

    #[tokio::test]
    async fn set_then_get_roundtrip() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let kv = store(tmp.path());
        assert!(kv.get("theme").await.expect("未命中应 Ok(None)").is_none());

        kv.set("theme", b"dark").await.expect("写入");
        assert_eq!(
            kv.get("theme").await.expect("读取").as_deref(),
            Some(&b"dark"[..])
        );
    }

    #[tokio::test]
    async fn set_overwrites_value() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let kv = store(tmp.path());
        kv.set("k", b"first").await.expect("首次写入");
        kv.set("k", b"second").await.expect("覆盖写入");
        assert_eq!(
            kv.get("k").await.expect("读取").as_deref(),
            Some(&b"second"[..])
        );
    }

    #[tokio::test]
    async fn delete_reports_whether_key_existed() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let kv = store(tmp.path());
        assert!(
            !kv.delete("ghost").await.expect("删不存在键不报错"),
            "删除不存在的键返回 false"
        );
        kv.set("live", b"x").await.expect("写入");
        assert!(kv.delete("live").await.expect("删除应成功"));
        assert!(kv.get("live").await.expect("读取").is_none());
    }

    #[tokio::test]
    async fn survives_new_store_instance() {
        let tmp = tempfile::tempdir().expect("临时目录");
        store(tmp.path()).set("persist", b"1").await.expect("写入");
        // 新实例 = 模拟应用重启后仍能读回
        let reopened = store(tmp.path());
        assert_eq!(
            reopened
                .get("persist")
                .await
                .expect("重启后读取")
                .as_deref(),
            Some(&b"1"[..])
        );
    }

    #[tokio::test]
    async fn empty_key_rejected() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let kv = store(tmp.path());
        assert!(kv.get("").await.is_err(), "空键应判非法");
        assert!(kv.set("", b"x").await.is_err());
        assert!(kv.delete("").await.is_err());
    }

    #[tokio::test]
    async fn traversal_key_stays_inside_dir() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let dir = tmp.path().join("kv");
        let kv = store(&dir);
        let evil = "../../etc/passwd";
        kv.set(evil, b"pwned").await.expect("穿越键也应可写");

        let written = dir.join(key_filename(evil));
        assert!(written.is_file(), "应落在存储目录内的十六进制文件");
        assert!(
            !Path::new("/etc/passwd").exists()
                || std::fs::read("/etc/passwd").ok() != Some(b"pwned".to_vec())
        );
        assert_eq!(
            kv.get(evil).await.expect("读取").as_deref(),
            Some(&b"pwned"[..])
        );
    }

    #[test]
    fn key_filename_is_hex_and_unique() {
        assert_eq!(key_filename("a"), "61");
        assert_eq!(key_filename("A"), "41");
        assert_eq!(key_filename("ab"), "6162");
        // 多字节键按 UTF-8 字节编码，长度 2×字节数
        assert_eq!(key_filename("中").len(), 6);
        assert_ne!(key_filename("a/b"), key_filename("a_b"));
    }

    #[tokio::test]
    async fn unicode_and_separator_keys_roundtrip() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let kv = store(tmp.path());
        for key in ["a/b", "a\\b", "键:值", "with space"] {
            kv.set(key, key.as_bytes()).await.expect("写入");
            assert_eq!(
                kv.get(key).await.expect("读取").as_deref(),
                Some(key.as_bytes()),
                "键 {key} 应往返一致"
            );
        }
    }

    #[tokio::test]
    async fn binary_values_preserved() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let kv = store(tmp.path());
        let payload = vec![0u8, 255, 10, 13, 0];
        kv.set("bin", &payload).await.expect("写入");
        assert_eq!(kv.get("bin").await.expect("读取"), Some(payload));
    }

    #[tokio::test]
    async fn large_value_roundtrip() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let kv = store(tmp.path());
        let payload = vec![7u8; 128 * 1024];
        kv.set("big", &payload).await.expect("写入");
        assert_eq!(
            kv.get("big").await.expect("读取").map(|v| v.len()),
            Some(131072)
        );
    }

    #[tokio::test]
    async fn object_safe_and_usable_as_trait_object() {
        let tmp = tempfile::tempdir().expect("临时目录");
        std::fs::create_dir_all(tmp.path()).expect("建目录");
        let store: Box<dyn KeyValueStore> = Box::new(FileKeyValueStore::new(tmp.path()));
        store.set("boxed", b"ok").await.expect("装箱后写入");
        assert_eq!(
            store.get("boxed").await.expect("读取").as_deref(),
            Some(&b"ok"[..])
        );
    }

    #[tokio::test]
    async fn set_creates_missing_dir() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let dir = tmp.path().join("not/yet/created");
        let kv = FileKeyValueStore::new(&dir);
        kv.set("k", b"v").await.expect("写入应自建目录");
        assert!(dir.is_dir());
    }
}
