use axum::extract::State;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::sync::Arc;

use crate::AppState;

/// Prometheus 抓取端点（L459）：进程级 recorder 快照渲染
pub(crate) async fn metrics_endpoint() -> String {
    global_metrics_handle().render()
}

/// 进程级唯一 Prometheus 句柄（install_recorder 每进程一次——首个调用者
/// install 全局接管 metrics 宏，后续复用渲染）
pub(crate) fn global_metrics_handle() -> &'static PrometheusHandle {
    static HANDLE: std::sync::OnceLock<PrometheusHandle> = std::sync::OnceLock::new();
    HANDLE.get_or_init(|| {
        PrometheusBuilder::new()
            .install_recorder()
            .unwrap_or_else(|_| PrometheusBuilder::new().build_recorder().handle())
    })
}

/// 健康检查
pub(crate) async fn health_check() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "healthy",
        "service": "real-time-feed",
        "timestamp": chrono::Utc::now(),
    }))
}

/// 获取服务统计信息
pub(crate) async fn get_stats(
    State(app_state): State<Arc<AppState>>,
) -> axum::Json<serde_json::Value> {
    let connection_count = app_state.connection_manager.get_connection_count();

    axum::Json(serde_json::json!({
        "active_connections": connection_count,
        "service": "real-time-feed",
        "timestamp": chrono::Utc::now(),
    }))
}
