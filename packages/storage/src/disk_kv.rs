//! 本地磁盘 Key-Value 存储

use crate::StorageBackend;
use alpha_core::errors::{AlphaError, AlphaResult};
use std::path::{Path, PathBuf};
use tokio::fs;

/// 将 `file://...` / `disk://...` / 裸路径转换为 `PathBuf`
fn parse_base_path(connection_string: &str) -> AlphaResult<PathBuf> {
    let trimmed = connection_string.trim();
    if trimmed.is_empty() {
        return Err(AlphaError::ConfigurationError(
            "LocalDisk backend requires a non-empty connection_string".to_string(),
        ));
    }

    let path_str = trimmed
        .strip_prefix("file://")
        .or_else(|| trimmed.strip_prefix("disk://"))
        .unwrap_or(trimmed);

    Ok(PathBuf::from(path_str))
}

fn encode_segment(segment: &str) -> AlphaResult<String> {
    if segment.is_empty() || segment == "." || segment == ".." {
        return Err(AlphaError::InvalidInput(format!(
            "invalid storage key segment: {segment:?}"
        )));
    }
    Ok(urlencoding::encode(segment).into_owned())
}

fn decode_segment(segment: &str) -> AlphaResult<String> {
    urlencoding::decode(segment)
        .map(|s| s.into_owned())
        .map_err(|e| AlphaError::InvalidInput(format!("invalid percent-encoding: {e}")))
}

/// 以目录层级保存 key 的本地磁盘存储后端。
#[derive(Debug, Clone)]
pub struct DiskKvStorage {
    base_path: PathBuf,
}

impl DiskKvStorage {
    pub fn new(base_path: impl AsRef<Path>) -> Self {
        Self {
            base_path: base_path.as_ref().to_path_buf(),
        }
    }

    pub fn from_connection_string(connection_string: &str) -> AlphaResult<Self> {
        Ok(Self::new(parse_base_path(connection_string)?))
    }

    fn key_to_path(&self, key: &str) -> AlphaResult<PathBuf> {
        let key = key.trim_matches('/');
        if key.is_empty() {
            return Err(AlphaError::InvalidInput("storage key cannot be empty".to_string()));
        }

        let mut out = self.base_path.clone();
        let segments: Vec<&str> = key.split('/').collect();
        for seg in &segments[..segments.len().saturating_sub(1)] {
            out.push(encode_segment(seg)?);
        }

        let file = format!("{}.bin", encode_segment(segments[segments.len() - 1])?);
        out.push(file);
        Ok(out)
    }

    async fn ensure_parent_dir(path: &Path) -> AlphaResult<()> {
        let parent = path.parent().ok_or_else(|| {
            AlphaError::StorageError("failed to resolve parent directory".to_string())
        })?;
        fs::create_dir_all(parent)
            .await
            .map_err(|e| AlphaError::StorageError(format!("failed to create directory: {e}")))?;
        Ok(())
    }

    async fn collect_keys_under(
        &self,
        dir: &Path,
        prefix_filter: &str,
        out: &mut Vec<String>,
    ) -> AlphaResult<()> {
        let mut pending = vec![dir.to_path_buf()];

        while let Some(current_dir) = pending.pop() {
            let mut rd = match fs::read_dir(&current_dir).await {
                Ok(rd) => rd,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    return Err(AlphaError::StorageError(format!(
                        "failed to read directory {current_dir:?}: {e}"
                    )))
                }
            };

            while let Some(entry) = rd
                .next_entry()
                .await
                .map_err(|e| AlphaError::StorageError(format!("failed to read dir entry: {e}")))?
            {
                let path = entry.path();
                let file_type = entry
                    .file_type()
                    .await
                    .map_err(|e| AlphaError::StorageError(format!("failed to stat path: {e}")))?;

                if file_type.is_dir() {
                    pending.push(path);
                    continue;
                }

                if !file_type.is_file() {
                    continue;
                }

                let rel = match path.strip_prefix(&self.base_path) {
                    Ok(rel) => rel,
                    Err(_) => continue,
                };

                let mut parts = Vec::new();
                for component in rel.components() {
                    let s = component.as_os_str().to_string_lossy();
                    parts.push(s.to_string());
                }

                if parts.is_empty() {
                    continue;
                }

                let last = parts.pop().unwrap();
                let last = match last.strip_suffix(".bin") {
                    Some(v) => v,
                    None => continue,
                };
                parts.push(last.to_string());

                let mut decoded = Vec::with_capacity(parts.len());
                for p in parts {
                    decoded.push(decode_segment(&p)?);
                }

                let key = decoded.join("/");
                if key.starts_with(prefix_filter) {
                    out.push(key);
                }
            }
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl StorageBackend for DiskKvStorage {
    async fn store(&self, key: &str, value: Vec<u8>) -> AlphaResult<()> {
        let path = self.key_to_path(key)?;
        Self::ensure_parent_dir(&path).await?;

        fs::write(&path, value)
            .await
            .map_err(|e| AlphaError::StorageError(format!("failed to write file {path:?}: {e}")))?;
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> AlphaResult<Option<Vec<u8>>> {
        let path = self.key_to_path(key)?;
        match fs::read(&path).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(AlphaError::StorageError(format!(
                "failed to read file {path:?}: {e}"
            ))),
        }
    }

    async fn delete(&self, key: &str) -> AlphaResult<bool> {
        let path = self.key_to_path(key)?;
        match fs::remove_file(&path).await {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(AlphaError::StorageError(format!(
                "failed to delete file {path:?}: {e}"
            ))),
        }
    }

    async fn exists(&self, key: &str) -> AlphaResult<bool> {
        let path = self.key_to_path(key)?;
        Ok(fs::try_exists(path).await.unwrap_or(false))
    }

    async fn list_keys(&self, prefix: &str) -> AlphaResult<Vec<String>> {
        let prefix = prefix.trim_matches('/');
        let mut keys = Vec::new();
        self.collect_keys_under(&self.base_path, prefix, &mut keys)
            .await?;
        keys.sort();
        keys.dedup();
        Ok(keys)
    }

    async fn clear(&self) -> AlphaResult<()> {
        match fs::remove_dir_all(&self.base_path).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(AlphaError::StorageError(format!(
                    "failed to remove directory {base:?}: {e}",
                    base = self.base_path
                )))
            }
        }
        fs::create_dir_all(&self.base_path)
            .await
            .map_err(|e| AlphaError::StorageError(format!("failed to recreate base dir: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn disk_kv_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let storage = DiskKvStorage::new(tmp.path());

        storage
            .store("a/b/c", b"hello".to_vec())
            .await
            .unwrap();
        assert!(storage.exists("a/b/c").await.unwrap());

        let val = storage.retrieve("a/b/c").await.unwrap().unwrap();
        assert_eq!(val, b"hello".to_vec());

        let keys = storage.list_keys("a/").await.unwrap();
        assert!(keys.contains(&"a/b/c".to_string()));

        assert!(storage.delete("a/b/c").await.unwrap());
        assert!(!storage.exists("a/b/c").await.unwrap());
    }

    #[tokio::test]
    async fn disk_kv_encodes_special_segments_and_clear_removes_all_data() {
        let tmp = TempDir::new().unwrap();
        let storage = DiskKvStorage::new(tmp.path());

        storage
            .store("quotes/600519.SH space/%value", b"payload".to_vec())
            .await
            .unwrap();

        let encoded_path = tmp
            .path()
            .join("quotes")
            .join("600519.SH%20space")
            .join("%25value.bin");
        assert!(fs::try_exists(&encoded_path).await.unwrap());

        let keys = storage.list_keys("quotes/").await.unwrap();
        assert_eq!(keys, vec!["quotes/600519.SH space/%value".to_string()]);

        storage.clear().await.unwrap();
        assert_eq!(storage.list_keys("").await.unwrap(), Vec::<String>::new());
        assert!(fs::try_exists(tmp.path()).await.unwrap());
    }

    #[test]
    fn disk_kv_rejects_invalid_keys_and_connection_strings() {
        assert!(DiskKvStorage::from_connection_string("").is_err());

        let tmp = TempDir::new().unwrap();
        let storage = DiskKvStorage::new(tmp.path());

        assert!(storage.key_to_path("").is_err());
        assert!(storage.key_to_path("/").is_err());
        assert!(storage.key_to_path("../escape").is_err());
        assert!(storage.key_to_path("safe/../escape").is_err());
    }
}
