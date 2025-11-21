//! 统一错误定义

use thiserror::Error;

/// Alpha Finance 统一错误类型
#[derive(Debug, Clone, Error)]
pub enum AlphaError {
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Data not found: {0}")]
    DataNotFound(String),

    #[error("Calculation error: {0}")]
    CalculationError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    #[error("Authentication error: {0}")]
    AuthenticationError(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Rate limited: {0}")]
    RateLimited(String),

    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("Internal error: {0}")]
    InternalError(String),

    #[error("Platform error: {0}")]
    PlatformError(String),

    #[error("WASM error: {0}")]
    WasmError(String),

    #[error("JNI error: {0}")]
    JniError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),
}

/// 统一结果类型
pub type AlphaResult<T> = Result<T, AlphaError>;

impl AlphaError {
    /// 创建网络错误
    pub fn network(msg: impl Into<String>) -> Self {
        Self::NetworkError(msg.into())
    }

    /// 创建数据未找到错误
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::DataNotFound(msg.into())
    }

    /// 创建无效输入错误
    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self::InvalidInput(msg.into())
    }

    /// 创建内部错误
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::InternalError(msg.into())
    }
}

// 为常见外部错误类型实现转换
impl From<serde_json::Error> for AlphaError {
    fn from(err: serde_json::Error) -> Self {
        Self::SerializationError(err.to_string())
    }
}

impl From<chrono::ParseError> for AlphaError {
    fn from(err: chrono::ParseError) -> Self {
        Self::InvalidInput(format!("Date parsing error: {}", err))
    }
}

#[cfg(target_arch = "wasm32")]
impl From<js_sys::Error> for AlphaError {
    fn from(err: js_sys::Error) -> Self {
        Self::WasmError(format!("JavaScript error: {}", err.as_string().unwrap_or_default()))
    }
}

#[cfg(target_os = "android")]
impl From<jni::errors::Error> for AlphaError {
    fn from(err: jni::errors::Error) -> Self {
        Self::JniError(format!("JNI error: {}", err))
    }
}