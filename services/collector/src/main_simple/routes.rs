//! HTTP API 面：任务/行情/健康路由、处理器与 API DTO

use axum::{
    body::Body,
    extract::State,
    http::StatusCode,
    middleware::Next,
    response::{
        sse::{Event as SseEvent, KeepAlive, Sse},
        IntoResponse, Json, Response,
    },
    routing::{get, post},
    Router,
};
use chrono::{DateTime, Utc};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::{
    wrappers::{errors::BroadcastStreamRecvError, BroadcastStream},
    StreamExt,
};
use tracing::debug;

use super::SimpleCollector;
use crate::sources::CrawlerError;

/// 新建任务请求
#[derive(Debug, Deserialize)]
pub struct NewTaskRequest {
    /// 任务ID
    pub id: Option<String>,
    /// 任务名称
    pub name: String,
    /// 任务类型
    pub source_type: String,
    /// 数据源URL
    pub url: String,
    /// HTTP方法
    pub method: Option<String>,
    /// 请求头
    pub headers: Option<HashMap<String, String>>,
    /// 请求体
    pub body: Option<String>,
    /// 优先级
    pub priority: Option<String>,
    /// 调度表达式
    pub schedule: Option<String>,
    /// 超时时间（秒）
    pub timeout: Option<u64>,
    /// 最大重试次数
    pub max_retries: Option<u32>,
    /// 语言选择
    pub language: Option<String>,
    /// 标签
    pub tags: Option<Vec<String>>,
}

/// 任务响应
#[derive(Debug, Serialize)]
pub struct TaskResponse {
    /// 任务ID
    pub task_id: String,
    /// 状态
    pub status: String,
    /// 消息
    pub message: Option<String>,
    /// 创建时间
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct PublishQuotesRequest {
    pub symbols: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct PublishQuotesResponse {
    pub stream: String,
    pub requested: usize,
    pub published: usize,
}

/// 健康检查响应
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// 服务状态
    pub status: String,
    /// 版本
    pub version: String,
    /// 运行时间
    pub uptime_seconds: u64,
    /// 任务统计
    pub task_stats: TaskStats,
}

/// 任务统计
#[derive(Debug, Serialize)]
pub struct TaskStats {
    /// 总任务数
    pub total: usize,
    /// 运行中
    pub running: usize,
    /// 已完成
    pub completed: usize,
    /// 失败
    pub failed: usize,
}

/// 进程级唯一 Prometheus 句柄（install_recorder 每进程一次——首个调用者
/// install 全局接管 metrics 宏，后续复用渲染；collector 指标面是单一宏
/// 世界：`alpha_collector_*` 全部经宏进入 /metrics，prometheus crate 自
/// 带 Registry 的第二世界已随 CollectorMetrics 死码移除，勿再引入）
fn global_metrics_handle() -> &'static PrometheusHandle {
    static HANDLE: std::sync::OnceLock<PrometheusHandle> = std::sync::OnceLock::new();
    HANDLE.get_or_init(|| {
        PrometheusBuilder::new()
            .install_recorder()
            .unwrap_or_else(|_| PrometheusBuilder::new().build_recorder().handle())
    })
}

/// 构建路由
pub fn build_router(collector: Arc<SimpleCollector>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/metrics", get(metrics_endpoint))
        .route("/tasks", post(submit_task))
        .route("/tasks/:id", get(get_task_status).delete(delete_task))
        .route("/tasks", get(list_tasks))
        .route("/sources/health", get(get_sources_health))
        .route("/tasks/:id/cancel", post(cancel_task))
        .route("/tasks/:id/execute", post(execute_task))
        .route("/streams/quotes/publish", post(publish_quotes))
        .route("/stats", get(get_stats))
        .route("/events", get(sse_events))
        .with_state(collector)
        .layer(axum::middleware::from_fn(request_log_middleware))
}

async fn request_log_middleware(request: axum::http::Request<Body>, next: Next) -> Response {
    let method = request.method().to_string();
    let uri = request.uri().to_string();
    debug!("{} {}", method, uri);
    next.run(request).await
}

/// Prometheus 抓取端点（L459）：全局 recorder 快照渲染
async fn metrics_endpoint() -> String {
    global_metrics_handle().render()
}

/// 健康检查端点
async fn health_check(State(collector): State<Arc<SimpleCollector>>) -> impl IntoResponse {
    let stats = collector.get_task_stats().await;

    let response = HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: collector.started_at.elapsed().as_secs(),
        task_stats: stats,
    };

    (StatusCode::OK, Json(response))
}

/// 提交任务端点
async fn submit_task(
    State(collector): State<Arc<SimpleCollector>>,
    Json(request): Json<NewTaskRequest>,
) -> impl IntoResponse {
    match collector.submit_task(request).await {
        Ok(response) => {
            let value = serde_json::to_value(response)
                .unwrap_or_else(|_| serde_json::json!({"error": "failed to serialize response"}));
            (StatusCode::CREATED, Json(value))
        }
        Err(error) => {
            let response = serde_json::json!({
                "error": error
            });
            (StatusCode::BAD_REQUEST, Json(response))
        }
    }
}

/// 获取任务状态端点
async fn get_task_status(
    State(collector): State<Arc<SimpleCollector>>,
    axum::extract::Path(task_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    match collector.get_task_status(&task_id).await {
        Some(status) => {
            let response = serde_json::json!({
                "task_id": task_id,
                "status": format!("{:?}", status)
            });
            (StatusCode::OK, Json(response))
        }
        None => {
            let response = serde_json::json!({
                "error": "Task not found"
            });
            (StatusCode::NOT_FOUND, Json(response))
        }
    }
}

/// 列出所有任务端点
async fn list_tasks(State(collector): State<Arc<SimpleCollector>>) -> impl IntoResponse {
    let tasks = collector.tasks.read().await;
    let task_list: Vec<_> = tasks
        .values()
        .map(|task| {
            serde_json::json!({
                "id": task.id,
                "name": task.name,
                "source": format!("{:?}", task.source),
                "priority": format!("{:?}", task.priority),
                "created_at": task.created_at,
                "status": format!("{:?}", task.status),
            })
        })
        .collect();

    (StatusCode::OK, Json(task_list))
}

/// 数据源健康端点（L504）：unknown=从未执行 / healthy / degraded（连败
/// ≤阈值）/ down（连败 >阈值）；gauge `alpha_collector_source_health`
/// 同步维护，alert 规则按阈值比对（≥2 预警、≥3 告警）
async fn get_sources_health(State(collector): State<Arc<SimpleCollector>>) -> impl IntoResponse {
    (StatusCode::OK, Json(collector.get_sources_health().await))
}

/// 执行任务端点
async fn execute_task(
    State(collector): State<Arc<SimpleCollector>>,
    axum::extract::Path(task_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    match collector.execute_task(&task_id).await {
        Ok(message) => {
            let response = serde_json::json!({
                "task_id": task_id,
                "message": message
            });
            (StatusCode::OK, Json(response))
        }
        Err(error) => {
            let response = serde_json::json!({
                "task_id": task_id,
                "error": error
            });
            (StatusCode::INTERNAL_SERVER_ERROR, Json(response))
        }
    }
}

/// 取消任务端点（重试语义归既有 POST /tasks/:id/execute 承担——
/// 对 Failed/Cancelled 任务重跑即 retry，不再造重复端点）
async fn cancel_task(
    State(collector): State<Arc<SimpleCollector>>,
    axum::extract::Path(task_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    match collector.cancel_task(&task_id).await {
        Ok(message) => (
            StatusCode::OK,
            Json(serde_json::json!({ "task_id": task_id, "message": message })),
        ),
        Err(error) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": error })),
        ),
    }
}

/// 删除任务端点：204 幂等删除；409 运行中；404 不会出现（幂等语义）
async fn delete_task(
    State(collector): State<Arc<SimpleCollector>>,
    axum::extract::Path(task_id): axum::extract::Path<String>,
) -> StatusCode {
    match collector.delete_task(&task_id).await {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::CONFLICT,
    }
}

async fn publish_quotes(
    State(collector): State<Arc<SimpleCollector>>,
    Json(request): Json<PublishQuotesRequest>,
) -> impl IntoResponse {
    match collector.publish_realtime_quotes(&request.symbols).await {
        Ok(response) => (
            StatusCode::OK,
            Json(serde_json::to_value(response).unwrap_or_default()),
        ),
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": error })),
        ),
    }
}

/// 获取统计信息端点
async fn get_stats(State(collector): State<Arc<SimpleCollector>>) -> impl IntoResponse {
    let stats = collector.get_task_stats().await;
    (StatusCode::OK, Json(stats))
}

/// SSE事件流端点
async fn sse_events(State(collector): State<Arc<SimpleCollector>>) -> impl IntoResponse {
    let rx = collector.subscribe_events();

    let stream = BroadcastStream::new(rx).filter_map(|msg| match msg {
        Ok(event) => match SseEvent::default().json_data(&event) {
            Ok(evt) => Some(Ok::<SseEvent, Infallible>(evt)),
            Err(err) => {
                tracing::warn!("failed to serialize event: {}", err);
                None
            }
        },
        Err(BroadcastStreamRecvError::Lagged(skipped)) => {
            tracing::warn!("events subscriber lagged by {}", skipped);
            None
        }
    });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

pub(crate) fn map_crawler_error(err: CrawlerError) -> String {
    match err {
        CrawlerError::RequestError(msg) => format!("request error: {}", msg),
        CrawlerError::ParseError(msg) => format!("parse error: {}", msg),
        CrawlerError::SourceError(msg) => format!("source error: {}", msg),
        CrawlerError::RateLimited => "rate limited".to_string(),
        CrawlerError::Timeout => "crawler timeout".to_string(),
        CrawlerError::InvalidData(msg) => format!("invalid data: {}", msg),
    }
}
