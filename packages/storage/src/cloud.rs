//! 对象存储后端

use crate::StorageBackend;
use alpha_core::errors::{AlphaError, AlphaResult};
use reqwest::StatusCode;
use std::collections::BTreeSet;
use url::Url;

const INDEX_KEY: &str = "alpha/_index.json";

/// 云存储配置
#[derive(Debug, Clone)]
pub struct CloudStorageConfig {
    pub provider: CloudProvider,
    pub bucket: String,
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
    pub endpoint: String,
}

/// 云存储提供商
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudProvider {
    AWS,
    Alibaba,
    Tencent,
    MinIO,
    Generic,
}

impl CloudProvider {
    fn from_str(input: &str) -> Self {
        match input.trim().to_ascii_lowercase().as_str() {
            "aws" | "s3" => Self::AWS,
            "alibaba" | "oss" => Self::Alibaba,
            "tencent" | "cos" => Self::Tencent,
            "minio" => Self::MinIO,
            _ => Self::Generic,
        }
    }
}

/// 基于简单 HTTP 对象接口的云存储。
///
/// 约定接口形如 `PUT/GET/HEAD/DELETE {endpoint}/{bucket}/{key}`。
/// `list_keys/clear` 通过维护 `alpha/_index.json` 实现，因此不依赖服务端的目录列举 API。
#[derive(Debug, Clone)]
pub struct CloudStorage {
    config: CloudStorageConfig,
    object_storage: ObjectStorage,
}

impl CloudStorage {
    pub fn new(config: CloudStorageConfig) -> Self {
        let object_storage = ObjectStorage::new(
            config.endpoint.clone(),
            config.access_key.clone(),
            config.secret_key.clone(),
        );

        Self {
            config,
            object_storage,
        }
    }

    pub fn from_connection_string(connection_string: &str) -> AlphaResult<Self> {
        let url = Url::parse(connection_string).map_err(|e| {
            AlphaError::ConfigurationError(format!("invalid cloud storage connection string: {e}"))
        })?;

        let bucket = url.host_str().ok_or_else(|| {
            AlphaError::ConfigurationError(
                "cloud storage connection string must include bucket name in host part".to_string(),
            )
        })?;

        let mut endpoint = None;
        let mut region = None;
        let mut access_key = None;
        let mut secret_key = None;
        let mut provider = CloudProvider::AWS;

        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "endpoint" => endpoint = Some(value.into_owned()),
                "region" => region = Some(value.into_owned()),
                "access_key" => access_key = Some(value.into_owned()),
                "secret_key" => secret_key = Some(value.into_owned()),
                "provider" => provider = CloudProvider::from_str(&value),
                _ => {}
            }
        }

        let endpoint = endpoint.unwrap_or_else(|| "http://127.0.0.1:9000".to_string());
        let region = region.unwrap_or_else(|| "us-east-1".to_string());

        Ok(Self::new(CloudStorageConfig {
            provider,
            bucket: bucket.to_string(),
            region,
            access_key: access_key.unwrap_or_default(),
            secret_key: secret_key.unwrap_or_default(),
            endpoint,
        }))
    }

    fn build_key(&self, key: &str) -> AlphaResult<String> {
        let key = key.trim_matches('/');
        if key.is_empty() {
            return Err(AlphaError::InvalidInput("storage key cannot be empty".to_string()));
        }

        let mut encoded = Vec::new();
        for segment in key.split('/') {
            if segment.is_empty() || segment == "." || segment == ".." {
                return Err(AlphaError::InvalidInput(format!(
                    "invalid storage key segment: {segment:?}"
                )));
            }
            encoded.push(urlencoding::encode(segment).into_owned());
        }

        Ok(format!("alpha/objects/{}.bin", encoded.join("/")))
    }

    #[cfg(test)]
    fn extract_key(&self, object_key: &str) -> Option<String> {
        let suffix = object_key.strip_prefix("alpha/objects/")?;
        let suffix = suffix.strip_suffix(".bin")?;

        let mut decoded = Vec::new();
        for segment in suffix.split('/') {
            let decoded_segment = urlencoding::decode(segment).ok()?;
            decoded.push(decoded_segment.into_owned());
        }
        Some(decoded.join("/"))
    }

    async fn load_index(&self) -> AlphaResult<BTreeSet<String>> {
        let index = match self
            .object_storage
            .download(&self.config.bucket, INDEX_KEY)
            .await?
        {
            Some(bytes) => serde_json::from_slice::<Vec<String>>(&bytes)
                .map_err(|e| AlphaError::SerializationError(format!("invalid cloud index: {e}")))?,
            None => Vec::new(),
        };

        Ok(index.into_iter().collect())
    }

    async fn save_index(&self, index: &BTreeSet<String>) -> AlphaResult<()> {
        let bytes = serde_json::to_vec(&index.iter().cloned().collect::<Vec<_>>())
            .map_err(|e| AlphaError::SerializationError(format!("failed to encode index: {e}")))?;
        self.object_storage
            .upload(&self.config.bucket, INDEX_KEY, bytes)
            .await
    }
}

#[async_trait::async_trait]
impl StorageBackend for CloudStorage {
    async fn store(&self, key: &str, value: Vec<u8>) -> AlphaResult<()> {
        let object_key = self.build_key(key)?;
        self.object_storage
            .upload(&self.config.bucket, &object_key, value)
            .await?;

        let mut index = self.load_index().await?;
        index.insert(key.trim_matches('/').to_string());
        self.save_index(&index).await?;

        Ok(())
    }

    async fn retrieve(&self, key: &str) -> AlphaResult<Option<Vec<u8>>> {
        let object_key = self.build_key(key)?;
        self.object_storage
            .download(&self.config.bucket, &object_key)
            .await
    }

    async fn delete(&self, key: &str) -> AlphaResult<bool> {
        let object_key = self.build_key(key)?;
        let deleted = self
            .object_storage
            .delete(&self.config.bucket, &object_key)
            .await?;

        if deleted {
            let mut index = self.load_index().await?;
            index.remove(key.trim_matches('/'));
            self.save_index(&index).await?;
        }

        Ok(deleted)
    }

    async fn exists(&self, key: &str) -> AlphaResult<bool> {
        let object_key = self.build_key(key)?;
        self.object_storage
            .exists(&self.config.bucket, &object_key)
            .await
    }

    async fn list_keys(&self, prefix: &str) -> AlphaResult<Vec<String>> {
        let prefix = prefix.trim_matches('/');
        let mut keys = self
            .load_index()
            .await?
            .into_iter()
            .filter(|key| key.starts_with(prefix))
            .collect::<Vec<_>>();
        keys.sort();
        Ok(keys)
    }

    async fn clear(&self) -> AlphaResult<()> {
        let keys = self.load_index().await?;
        for key in &keys {
            let object_key = self.build_key(key)?;
            self.object_storage
                .delete(&self.config.bucket, &object_key)
                .await?;
        }
        self.object_storage
            .delete(&self.config.bucket, INDEX_KEY)
            .await?;
        Ok(())
    }
}

/// 简单对象存储客户端。
#[derive(Debug, Clone)]
pub struct ObjectStorage {
    client: reqwest::Client,
    endpoint: String,
    access_key: String,
    secret_key: String,
}

impl ObjectStorage {
    pub fn new(endpoint: String, access_key: String, secret_key: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            endpoint: endpoint.trim_end_matches('/').to_string(),
            access_key,
            secret_key,
        }
    }

    fn object_url(&self, bucket: &str, key: &str) -> String {
        let encoded_key = key
            .split('/')
            .map(|segment| urlencoding::encode(segment).into_owned())
            .collect::<Vec<_>>()
            .join("/");
        format!("{}/{}/{}", self.endpoint, bucket, encoded_key)
    }

    fn with_auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let request = if self.access_key.is_empty() {
            request
        } else {
            request.header("Authorization", format!("Bearer {}", self.access_key))
        };

        if self.secret_key.is_empty() {
            request
        } else {
            request.header("X-Secret-Key", &self.secret_key)
        }
    }

    pub async fn upload(&self, bucket: &str, key: &str, data: Vec<u8>) -> AlphaResult<()> {
        let url = self.object_url(bucket, key);
        let response = self
            .with_auth(self.client.put(&url))
            .body(data)
            .send()
            .await
            .map_err(|e| AlphaError::NetworkError(format!("upload failed: {e}")))?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(AlphaError::NetworkError(format!(
                "upload failed with status: {}",
                response.status()
            )))
        }
    }

    pub async fn download(&self, bucket: &str, key: &str) -> AlphaResult<Option<Vec<u8>>> {
        let url = self.object_url(bucket, key);
        let response = self
            .with_auth(self.client.get(&url))
            .send()
            .await
            .map_err(|e| AlphaError::NetworkError(format!("download failed: {e}")))?;

        match response.status() {
            StatusCode::OK => {
                let bytes = response
                    .bytes()
                    .await
                    .map_err(|e| AlphaError::NetworkError(format!("read body failed: {e}")))?;
                Ok(Some(bytes.to_vec()))
            }
            StatusCode::NOT_FOUND => Ok(None),
            status => Err(AlphaError::NetworkError(format!(
                "download failed with status: {status}"
            ))),
        }
    }

    pub async fn exists(&self, bucket: &str, key: &str) -> AlphaResult<bool> {
        let url = self.object_url(bucket, key);
        let response = self
            .with_auth(self.client.head(&url))
            .send()
            .await
            .map_err(|e| AlphaError::NetworkError(format!("head failed: {e}")))?;

        match response.status() {
            StatusCode::OK => Ok(true),
            StatusCode::NOT_FOUND => Ok(false),
            status => Err(AlphaError::NetworkError(format!(
                "head failed with status: {status}"
            ))),
        }
    }

    pub async fn delete(&self, bucket: &str, key: &str) -> AlphaResult<bool> {
        let url = self.object_url(bucket, key);
        let response = self
            .with_auth(self.client.delete(&url))
            .send()
            .await
            .map_err(|e| AlphaError::NetworkError(format!("delete failed: {e}")))?;

        match response.status() {
            StatusCode::OK | StatusCode::NO_CONTENT => Ok(true),
            StatusCode::NOT_FOUND => Ok(false),
            status => Err(AlphaError::NetworkError(format!(
                "delete failed with status: {status}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Bytes,
        extract::{Path, State},
        http::StatusCode as AxumStatusCode,
        routing::put,
        Router,
    };
    use std::{collections::HashMap, sync::Arc};
    use tokio::{net::TcpListener, sync::Mutex};

    #[derive(Clone, Default)]
    struct MockState {
        objects: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    }

    async fn put_object(
        State(state): State<MockState>,
        Path((bucket, key)): Path<(String, String)>,
        body: Bytes,
    ) -> AxumStatusCode {
        state
            .objects
            .lock()
            .await
            .insert(format!("{bucket}/{key}"), body.to_vec());
        AxumStatusCode::OK
    }

    async fn get_object(
        State(state): State<MockState>,
        Path((bucket, key)): Path<(String, String)>,
    ) -> (AxumStatusCode, Vec<u8>) {
        match state.objects.lock().await.get(&format!("{bucket}/{key}")).cloned() {
            Some(bytes) => (AxumStatusCode::OK, bytes),
            None => (AxumStatusCode::NOT_FOUND, Vec::new()),
        }
    }

    async fn head_object(
        State(state): State<MockState>,
        Path((bucket, key)): Path<(String, String)>,
    ) -> AxumStatusCode {
        if state.objects.lock().await.contains_key(&format!("{bucket}/{key}")) {
            AxumStatusCode::OK
        } else {
            AxumStatusCode::NOT_FOUND
        }
    }

    async fn delete_object(
        State(state): State<MockState>,
        Path((bucket, key)): Path<(String, String)>,
    ) -> AxumStatusCode {
        if state
            .objects
            .lock()
            .await
            .remove(&format!("{bucket}/{key}"))
            .is_some()
        {
            AxumStatusCode::NO_CONTENT
        } else {
            AxumStatusCode::NOT_FOUND
        }
    }

    async fn spawn_mock_object_storage() -> String {
        let app = Router::new()
            .route(
                "/:bucket/*key",
                put(put_object)
                    .get(get_object)
                    .head(head_object)
                    .delete(delete_object),
            )
            .with_state(MockState::default());

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        format!("http://{}", addr)
    }

    #[test]
    fn cloud_storage_key_building_and_parsing() {
        let config = CloudStorageConfig {
            provider: CloudProvider::MinIO,
            bucket: "test-bucket".to_string(),
            region: "us-east-1".to_string(),
            access_key: "test-key".to_string(),
            secret_key: "test-secret".to_string(),
            endpoint: "http://127.0.0.1:9000".to_string(),
        };

        let storage = CloudStorage::new(config);
        let object_key = storage.build_key("test/data").unwrap();
        assert_eq!(object_key, "alpha/objects/test/data.bin");
        assert_eq!(storage.extract_key(&object_key), Some("test/data".to_string()));
    }

    #[test]
    fn cloud_storage_parses_connection_string() {
        let storage = CloudStorage::from_connection_string(
            "s3://alpha-bucket?provider=minio&endpoint=http%3A%2F%2F127.0.0.1%3A9000&region=ap-southeast-1&access_key=ak&secret_key=sk",
        )
        .unwrap();

        assert_eq!(storage.config.bucket, "alpha-bucket");
        assert_eq!(storage.config.provider, CloudProvider::MinIO);
        assert_eq!(storage.config.endpoint, "http://127.0.0.1:9000");
        assert_eq!(storage.config.region, "ap-southeast-1");
    }

    #[tokio::test]
    async fn cloud_storage_roundtrip_list_and_clear() {
        let endpoint = spawn_mock_object_storage().await;
        let storage = CloudStorage::new(CloudStorageConfig {
            provider: CloudProvider::Generic,
            bucket: "alpha-bucket".to_string(),
            region: "local".to_string(),
            access_key: String::new(),
            secret_key: String::new(),
            endpoint,
        });

        storage
            .store("quotes/AAPL", b"100".to_vec())
            .await
            .unwrap();
        storage
            .store("quotes/MSFT", b"200".to_vec())
            .await
            .unwrap();

        assert!(storage.exists("quotes/AAPL").await.unwrap());
        assert_eq!(
            storage.retrieve("quotes/MSFT").await.unwrap(),
            Some(b"200".to_vec())
        );
        assert_eq!(
            storage.list_keys("quotes/").await.unwrap(),
            vec!["quotes/AAPL".to_string(), "quotes/MSFT".to_string()]
        );

        assert!(storage.delete("quotes/AAPL").await.unwrap());
        assert!(!storage.exists("quotes/AAPL").await.unwrap());

        storage.clear().await.unwrap();
        assert_eq!(storage.list_keys("").await.unwrap(), Vec::<String>::new());
    }
}
