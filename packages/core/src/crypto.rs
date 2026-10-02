//! 存储侧静态加密（L485）：AES-256-GCM 应用层加密
//!
//! 威胁模型：数据在 Timescale/ClickHouse/Redis 等外部存储系统中静止时的机密性。
//! - 非目标：传输加密（TLS 归 L485 传输面）、密钥管理体系（KMS/密钥轮换归后续）。
//! - 口径：每写一次生成新随机 IV（12 字节），AES-256-GCM 认证加密，密文 = IV || ciphertext || tag。
//!   解密时验证 tag，篡改/错钥直接返回 Err（不 panic、不泄漏明文长度侧信道）。
//! - 密钥来源：32 字节 Base64 编码环境变量 `ALPHA_STORAGE_ENC_KEY`（生产由 KMS 注入，
//!   本地开发可用 `openssl rand -base64 32` 生成）。缺失/非法 = 编译期 feature `crypto` 下运行期报错。

#[cfg(feature = "crypto")]
use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
#[cfg(feature = "crypto")]
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use thiserror::Error;

/// 存储加密错误
#[derive(Debug, Error)]
pub enum StorageEncryptionError {
    #[error("加密密钥未配置：环境变量 ALPHA_STORAGE_ENC_KEY 必须为 32 字节 Base64")]
    KeyMissing,
    #[error("加密密钥格式非法：必须为 32 字节 Base64 解码后")]
    KeyInvalid,
    #[error("加密失败：{0}")]
    EncryptFailed(String),
    #[error("解密失败：认证标签验证不通过（篡改或密钥错误）")]
    DecryptFailed,
}

/// 从环境变量加载 32 字节加密密钥（仅 `crypto` feature 下可用）
#[cfg(feature = "crypto")]
fn load_key() -> Result<Key<Aes256Gcm>, StorageEncryptionError> {
    let key_b64 =
        std::env::var("ALPHA_STORAGE_ENC_KEY").map_err(|_| StorageEncryptionError::KeyMissing)?;
    let key_bytes = BASE64
        .decode(key_b64)
        .map_err(|_| StorageEncryptionError::KeyInvalid)?;
    if key_bytes.len() != 32 {
        return Err(StorageEncryptionError::KeyInvalid);
    }
    Ok(*Key::<Aes256Gcm>::from_slice(&key_bytes))
}

/// 加密任意字节载荷（返回 IV || ciphertext || tag，Base64 编码便于存储为文本列）
#[cfg(feature = "crypto")]
pub fn encrypt_storage(payload: &[u8]) -> Result<String, StorageEncryptionError> {
    let key = load_key()?;
    let cipher = Aes256Gcm::new(&key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng); // 12 字节随机 IV
    let mut ciphertext = cipher
        .encrypt(&nonce, payload)
        .map_err(|e| StorageEncryptionError::EncryptFailed(e.to_string()))?;
    // 拼接：nonce(12) + ciphertext + tag(16, 已在 ciphertext 尾部)
    let mut out = Vec::with_capacity(12 + ciphertext.len());
    out.extend_from_slice(&nonce);
    out.append(&mut ciphertext);
    Ok(BASE64.encode(out))
}

/// 解密存储载荷（Base64 编码的 IV || ciphertext || tag）
#[cfg(feature = "crypto")]
pub fn decrypt_storage(encrypted_b64: &str) -> Result<Vec<u8>, StorageEncryptionError> {
    let key = load_key()?;
    let cipher = Aes256Gcm::new(&key);
    let data = BASE64
        .decode(encrypted_b64)
        .map_err(|_| StorageEncryptionError::DecryptFailed)?;
    if data.len() < 12 + 16 {
        return Err(StorageEncryptionError::DecryptFailed);
    }
    let nonce = Nonce::from_slice(&data[..12]);
    let ciphertext = &data[12..];
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| StorageEncryptionError::DecryptFailed)
}

/// 非 `crypto` feature 下的存根（编译通过、运行期明确报错）
#[cfg(not(feature = "crypto"))]
pub fn encrypt_storage(_payload: &[u8]) -> Result<String, StorageEncryptionError> {
    Err(StorageEncryptionError::KeyMissing)
}

#[cfg(not(feature = "crypto"))]
pub fn decrypt_storage(_encrypted_b64: &str) -> Result<Vec<u8>, StorageEncryptionError> {
    Err(StorageEncryptionError::KeyMissing)
}

#[cfg(test)]
#[cfg(feature = "crypto")]
mod tests {
    use super::*;
    use std::env;
    use std::sync::Mutex;

    // 生产实现按调用读进程级 env（ALPHA_STORAGE_ENC_KEY），五条测试并发 set_var
    // 会互相污染——登记过的偶发红：empty_payload_works 撞 decrypt_wrong_key_fails
    // 的换键窗口（encrypt 用对键、decrypt 读到错键）。全部串行拿这把进程内锁；
    // 锁中毒取内值继续，避免一条测试失败连锁污染其余四条。
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn set_test_key() {
        // 固定测试密钥（仅测试用，生产严禁硬编码）
        let key = [0x42u8; 32];
        env::set_var("ALPHA_STORAGE_ENC_KEY", BASE64.encode(key));
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_test_key();
        let plaintext = b"sensitive market data: symbol=600519,price=1800.50,volume=12345";
        let encrypted = encrypt_storage(plaintext).expect("encrypt ok");
        let decrypted = decrypt_storage(&encrypted).expect("decrypt ok");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn encrypt_produces_different_ciphertext_each_time() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_test_key();
        let plaintext = b"same plaintext";
        let c1 = encrypt_storage(plaintext).expect("encrypt 1");
        let c2 = encrypt_storage(plaintext).expect("encrypt 2");
        // 随机 IV 保证每次密文不同
        assert_ne!(c1, c2);
        // 但都能解回原文
        assert_eq!(decrypt_storage(&c1).expect("decrypt 1"), plaintext);
        assert_eq!(decrypt_storage(&c2).expect("decrypt 2"), plaintext);
    }

    #[test]
    fn decrypt_tampered_fails() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_test_key();
        let plaintext = b"data";
        let mut encrypted = encrypt_storage(plaintext).expect("encrypt");
        // 翻转最后一位（破坏 tag）
        let mut bytes = BASE64.decode(&encrypted).expect("b64 decode");
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        encrypted = BASE64.encode(bytes);
        assert!(decrypt_storage(&encrypted).is_err());
    }

    #[test]
    fn decrypt_wrong_key_fails() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_test_key();
        let plaintext = b"data";
        let encrypted = encrypt_storage(plaintext).expect("encrypt");
        // 临时换密钥
        let wrong_key = [0x43u8; 32];
        env::set_var("ALPHA_STORAGE_ENC_KEY", BASE64.encode(wrong_key));
        assert!(decrypt_storage(&encrypted).is_err());
    }

    #[test]
    fn empty_payload_works() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_test_key();
        let encrypted = encrypt_storage(b"").expect("encrypt empty");
        let decrypted = decrypt_storage(&encrypted).expect("decrypt empty");
        assert_eq!(decrypted, b"");
    }
}
