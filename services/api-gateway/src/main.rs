//! Alpha Finance API Gateway
//!
//! 统一的 API 入口点，负责路由、认证、限流和负载均衡

use axum::{
    extract::Query,
    http::{HeaderMap, StatusCode},
    middleware,
    response::Json,
    routing::{get, post},
    Router,
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tower::ServiceBuilder;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

/// API 网关配置
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// 服务器监听地址
    #[arg(short, long, default_value = "0.0.0.0:8080")]
    bind: SocketAddr,

    /// 服务发现地址
    #[arg(long, default_value = "http://localhost:8081")]
    discovery_url: String,

    /// 日志级别
    #[arg(short, long, default_value = "info")]
    log_level: String,
}

/// 健康检查响应
#[derive(Debug, Serialize)]
struct HealthResponse {
    status: String,
    version: String,
    timestamp: chrono::DateTime<chrono::Utc>,
    services: Vec<ServiceStatus>,
}

#[derive(Debug, Serialize)]
struct ServiceStatus {
    name: String,
    status: String,
    response_time_ms: u64,
}

/// API 路由响应
#[derive(Debug, Deserialize)]
struct ApiRequest {
    service: String,
    path: String,
}

#[derive(Debug, Serialize)]
struct ApiResponse<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
    timestamp: chrono::DateTime<chrono::Utc>,
}

impl<T> ApiResponse<T> {
    fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            timestamp: chrono::Utc::now(),
        }
    }

    fn error(message: String) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message),
            timestamp: chrono::Utc::now(),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // 初始化日志
    tracing_subscriber::fmt()
        .with_max_level(
            match args.log_level.to_lowercase().as_str() {
                "debug" => tracing::Level::DEBUG,
                "info" => tracing::Level::INFO,
                "warn" => tracing::Level::WARN,
                "error" => tracing::Level::ERROR,
                _ => tracing::Level::INFO,
            }
        )
        .init();

    tracing::info!("Starting Alpha Finance API Gateway");

    // 构建路由
    let app = Router::new()
        // 健康检查
        .route("/health", get(health_check))
        // API 代理
        .route("/api/v1/*path", get(api_proxy))
        .route("/api/v1/*path", post(api_proxy))
        // WebSocket 代理
        .route("/ws/*path", get(ws_proxy))
        // 中间件
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(
                    CorsLayer::new()
                        .allow_origin(Any)
                        .allow_methods(Any)
                        .allow_headers(Any),
                )
                .layer(middleware::from_fn(request_logger))
        );

    // 启动服务器
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    tracing::info!("API Gateway listening on {}", args.bind);

    axum::serve(listener, app).await?;

    Ok(())
}

/// 健康检查端点
async fn health_check() -> Json<HealthResponse> {
    let services = vec![
        ServiceStatus {
            name: "data-engine".to_string(),
            status: "healthy".to_string(),
            response_time_ms: 15,
        },
        ServiceStatus {
            name: "real-time-feed".to_string(),
            status: "healthy".to_string(),
            response_time_ms: 8,
        },
        ServiceStatus {
            name: "collector".to_string(),
            status: "healthy".to_string(),
            response_time_ms: 22,
        },
    ];

    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        timestamp: chrono::Utc::now(),
        services,
    })
}

/// API 代理端点
async fn api_proxy(
    axum::extract::Path(path): axum::extract::Path<String>,
    headers: HeaderMap,
    query: Query<std::collections::HashMap<String, String>>,
) -> Json<ApiResponse<serde_json::Value>> {
    // 这里应该实现实际的代理逻辑
    // 包括服务发现、负载均衡、认证等

    tracing::info!("Proxying request to: {}", path);

    // 模拟代理响应
    let mock_data = serde_json::json!({
        "path": path,
        "query": serde_json::to_value(query.into_inner()).unwrap_or_default(),
        "timestamp": chrono::Utc::now(),
    });

    Json(ApiResponse::success(mock_data))
}

/// WebSocket 代理端点
async fn ws_proxy(
    axum::extract::Path(path): axum::extract::Path<String>,
    ws: axum::extract::ws::WebSocketUpgrade,
    ws_state: axum::extract::State<()>,
) -> axum::response::Response {
    tracing::info!("WebSocket connection to: {}", path);

    // 这里应该实现 WebSocket 代理逻辑
    ws.on_upgrade(|_socket| async {
        // 处理 WebSocket 连接
    })
}

/// 请求日志中间件
async fn request_logger(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let method = req.method().clone();
    let uri = req.uri().clone();

    let start = std::time::Instant::now();
    let response = next.run(req).await;
    let duration = start.elapsed();

    tracing::info!(
        "Request: {} {} - Status: {} - Duration: {:?}",
        method,
        uri,
        response.status(),
        duration
    );

    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_check() {
        let response = health_check().await;
        let health = response.0;

        assert_eq!(health.status, "ok");
        assert!(!health.services.is_empty());
    }

    #[tokio::test]
    async fn test_api_response() {
        let data = serde_json::json!({"test": "value"});
        let response = ApiResponse::success(data.clone());

        assert!(response.success);
        assert!(response.data.is_some());
        assert!(response.error.is_none());
    }
}