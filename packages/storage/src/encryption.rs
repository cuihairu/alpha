//! 存储加密封装（L485 存储侧静态加密）
//!
//! 提供 `EncryptedStorage` 透明加密包装器，可包装任意实现 `StorageBackend` 的后端。
//! - 使用 alpha-core::crypto 的 AES-256-GCM（随机 IV，认证加密）
//! - 密钥通过环境变量 `ALPHA_STORAGE_ENC_KEY`（32 字节 Base64）注入
//! - 仅在 `crypto` feature 启用时编译（零成本抽象，未启用时编译报错引导开启 feature）
//! - 适用场景：Redis KV 缓存 API Key/会话/用户隐私、Timescale/ClickHouse 敏感列（文档演示）

use crate::StorageBackend;
use alpha_core::crypto::{decrypt_storage, encrypt_storage, StorageEncryptionError};
use alpha_core::errors::{AlphaError, AlphaResult};
use async_trait::async_trait;

/// 透明加密存储包装器
///
/// ```ignore
/// let base = RedisKvStorage::connect(url, None).await?;
/// let encrypted = EncryptedStorage::new(base);
/// encrypted.store("api_key", b"sk-...").await?;  // 自动加密
/// let plain = encrypted.retrieve("api_key").await?;  // 自动解密
/// ```
#[derive(Clone)]
pub struct EncryptedStorage<B: StorageBackend> {
    inner: B,
}

impl<B: StorageBackend> EncryptedStorage<B> {
    /// 包装任意存储后端
    pub fn new(inner: B) -> Self {
        Self { inner }
    }

    /// 访问内层后端（不加密的直接操作，慎用）
    pub fn inner(&self) -> &B {
        &self.inner
    }
}

#[async_trait]
impl<B: StorageBackend> StorageBackend for EncryptedStorage<B> {
    async fn store(&self, key: &str, value: Vec<u8>) -> AlphaResult<()> {
        let encrypted = encrypt_storage(&value)
            .map_err(|e| AlphaError::StorageError(format!("encryption failed: {e}")))?;
        self.inner.store(key, encrypted.into_bytes()).await
    }

    async fn retrieve(&self, key: &str) -> AlphaResult<Option<Vec<u8>>> {
        let encrypted_opt = self.inner.retrieve(key).await?;
        match encrypted_opt {
            Some(encrypted_bytes) => {
                let encrypted_str = String::from_utf8(encrypted_bytes).map_err(|_| {
                    AlphaError::StorageError("stored value not valid utf-8".to_string())
                })?;
                let decrypted = decrypt_storage(&encrypted_str)
                    .map_err(|e| AlphaError::StorageError(format!("decryption failed: {e}")))?;
                Ok(Some(decrypted))
            }
            None => Ok(None),
        }
    }

    async fn delete(&self, key: &str) -> AlphaResult<bool> {
        self.inner.delete(key).await
    }

    async fn exists(&self, key: &str) -> AlphaResult<bool> {
        self.inner.exists(key).await
    }

    async fn list_keys(&self, prefix: &str) -> AlphaResult<Vec<String>> {
        self.inner.list_keys(prefix).await
    }

    async fn clear(&self) -> AlphaResult<()> {
        self.inner.clear().await
    }
}

/// 列级加密辅助（用于 Timescale/ClickHouse 敏感字段）
///
/// 示例：
/// ```ignore
/// // 写入前加密敏感字段
/// let encrypted_price = encrypt_sensitive_field(&data.price.to_string())?;
/// // 存入数据库（price 列改为 TEXT 存密文）
/// // 读取后解密
/// let price: f64 = decrypt_sensitive_field(&row.encrypted_price)?.parse()?;
/// ```
pub fn encrypt_sensitive_field<T: AsRef<[u8]>>(value: T) -> Result<String, StorageEncryptionError> {
    encrypt_storage(value.as_ref())
}

pub fn decrypt_sensitive_field(encrypted_b64: &str) -> Result<Vec<u8>, StorageEncryptionError> {
    decrypt_storage(encrypted_b64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;
    use base64::Engine;

    fn set_test_enc_key() {
        std::env::set_var(
            "ALPHA_STORAGE_ENC_KEY",
            base64::engine::general_purpose::STANDARD.encode([0x42u8; 32]),
        );
    }

    #[tokio::test]
    async fn encrypted_storage_roundtrip() {
        set_test_enc_key();
        let base = MemoryStorage::new();
        let encrypted = EncryptedStorage::new(base);

        let key = "test_key";
        let value = b"sensitive api key: sk-1234567890abcdef";

        // 存储（自动加密）
        encrypted.store(key, value.to_vec()).await.unwrap();

        // 内层存储应为密文
        let raw = encrypted.inner().retrieve(key).await.unwrap().unwrap();
        let raw_str = String::from_utf8(raw).unwrap();
        assert!(raw_str.len() > value.len()); // 密文含 IV+tag
        assert_ne!(raw_str, std::str::from_utf8(value).unwrap());

        // 读取（自动解密）
        let retrieved = encrypted.retrieve(key).await.unwrap().unwrap();
        assert_eq!(retrieved, value);
    }

    #[tokio::test]
    async fn encrypted_storage_not_found() {
        set_test_enc_key();
        let base = MemoryStorage::new();
        let encrypted = EncryptedStorage::new(base);
        assert!(encrypted.retrieve("nonexistent").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn encrypted_storage_delete() {
        set_test_enc_key();
        let base = MemoryStorage::new();
        let encrypted = EncryptedStorage::new(base);

        encrypted.store("k", b"v".to_vec()).await.unwrap();
        assert!(encrypted.exists("k").await.unwrap());
        assert!(encrypted.delete("k").await.unwrap());
        assert!(!encrypted.exists("k").await.unwrap());
    }

    #[tokio::test]
    async fn field_level_encrypt_decrypt() {
        std::env::set_var(
            "ALPHA_STORAGE_ENC_KEY",
            base64::engine::general_purpose::STANDARD.encode([0x42u8; 32]),
        );

        let plaintext = "credit_card_4242_4242_4242_4242";
        let encrypted = encrypt_sensitive_field(plaintext).unwrap();
        let decrypted = decrypt_sensitive_field(&encrypted).unwrap();
        assert_eq!(String::from_utf8(decrypted).unwrap(), plaintext);

        // 篡改失败
        let mut tampered = base64::engine::general_purpose::STANDARD
            .decode(&encrypted)
            .unwrap();
        let last_idx = tampered.len() - 1;
        tampered[last_idx] ^= 0x01;
        let tampered_b64 = base64::engine::general_purpose::STANDARD.encode(&tampered);
        assert!(decrypt_sensitive_field(&tampered_b64).is_err());
    }
}
