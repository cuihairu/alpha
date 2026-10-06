//! Alpha Finance 通信协议
//!
//! 定义所有服务间的通信协议和数据格式

pub mod dataset;
pub mod events;
pub mod grpc;
pub mod instrument;
pub mod rest;
pub mod websocket;
// proto 代码生成依赖 tonic transport（服务端专属）；wasm 侧 --no-default-features 时不存在
#[cfg(feature = "grpc")]
#[allow(clippy::result_large_err)] // tonic 生成代码返回大 Err 变体，代码生成产物不做人工改造
pub mod proto {
    pub mod data_engine {
        tonic::include_proto!("alpha.dataengine");
    }
}

// 重新导出主要类型
pub use dataset::*;
pub use events::*;
pub use grpc::*;
pub use instrument::*;
pub use rest::*;
pub use websocket::*;

/// 公共的 API 错误类型
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApiError {
    pub code: i32,
    pub message: String,
    pub details: Option<String>,
}

impl ApiError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(code: i32, message: impl Into<String>, details: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: Some(details.into()),
        }
    }
}

/// 常用的 API 错误代码
pub mod error_codes {
    pub const SUCCESS: i32 = 0;
    pub const INVALID_REQUEST: i32 = 1001;
    pub const UNAUTHORIZED: i32 = 1002;
    pub const FORBIDDEN: i32 = 1003;
    pub const NOT_FOUND: i32 = 1004;
    pub const INTERNAL_ERROR: i32 = 1005;
    pub const RATE_LIMITED: i32 = 1006;
    pub const SERVICE_UNAVAILABLE: i32 = 1007;
}
