//! Alpha Finance API Gateway
//!
//! 统一的 API 入口点：REST 反代 data-engine、WebSocket 反代 real-time-feed、
//! 聚合健康检查（真实探测上游 `/health`，不再返回硬编码假状态）。
//!
//! 上游地址可经 CLI 参数或 `ALPHA_GATEWAY_*` 环境变量配置（env 优先，便于部署
//! 覆写，compose 已按容器网络注入）。

use axum::{
    body::Bytes,
    extract::{
        ws::{CloseFrame as WsCloseFrame, Message as WsMessage, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri},
    middleware,
    response::{IntoResponse, Json, Response},
    routing::{any, get},
    Router,
};
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;

use alpha_storage::{RateDecision, RedisRateLimiter};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tower::ServiceBuilder;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

/// 上游单次 REST 反代的整体超时
const PROXY_TIMEOUT_SECS: u64 = 30;
/// 健康检查对单个上游的探测超时
const HEALTH_PROBE_TIMEOUT_SECS: u64 = 2;

/// 逐跳头（RFC 7230）：反代时不应向下游/上游转发
const HOP_BY_HOP_HEADERS: &[&str] = &[
    "host",
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "content-length",
];

/// API 网关配置
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// 服务器监听地址
    #[arg(short, long, default_value = "0.0.0.0:8080")]
    bind: SocketAddr,

    /// data-engine 上游地址（REST 反代目标；env ALPHA_GATEWAY_DATA_ENGINE_URL 优先）
    #[arg(long, default_value = "http://127.0.0.1:8081")]
    data_engine_url: String,

    /// real-time-feed 上游地址（WS 反代目标；env ALPHA_GATEWAY_REALTIME_URL 优先）
    #[arg(long, default_value = "http://127.0.0.1:8082")]
    realtime_url: String,

    /// collector 上游地址（仅健康探测；env ALPHA_GATEWAY_COLLECTOR_URL 优先）
    #[arg(long, default_value = "http://127.0.0.1:8083")]
    collector_url: String,

    /// Redis 限流地址（env ALPHA_GATEWAY_RATE_LIMIT_URL 优先；空=关闭 /api 限流）
    #[arg(long, default_value = "")]
    rate_limit_url: String,

    /// 每 IP 每分钟 /api 配额（Redis 限流开启时生效）
    #[arg(long, default_value_t = 120)]
    rate_limit_per_minute: u32,

    /// 日志级别
    #[arg(short, long, default_value = "info")]
    log_level: String,
}

/// env 覆写优先于 CLI 默认值（便于部署注入，无需改启动参数）
fn resolve_url(env_key: &str, arg_value: &str) -> String {
    std::env::var(env_key).unwrap_or_else(|_| arg_value.to_string())
}

/// 网关共享状态
#[derive(Clone)]
struct GatewayState {
    client: reqwest::Client,
    data_engine_url: String,
    realtime_url: String,
    collector_url: String,
    /// Redis 限流器（None = 限流关闭）
    rate_limiter: Option<RedisRateLimiter>,
    /// 每 IP 每分钟 /api 配额
    rate_limit_per_minute: u32,
    /// Prometheus 指标渲染句柄（/metrics）
    metrics: PrometheusHandle,
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
    response_time_ms: u128,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // 初始化日志
    tracing_subscriber::fmt()
        .with_max_level(match args.log_level.to_lowercase().as_str() {
            "debug" => tracing::Level::DEBUG,
            "info" => tracing::Level::INFO,
            "warn" => tracing::Level::WARN,
            "error" => tracing::Level::ERROR,
            _ => tracing::Level::INFO,
        })
        .init();

    tracing::info!("Starting Alpha Finance API Gateway");

    // Prometheus 指标：install_recorder 全局接管 metrics 宏；/metrics 暴露
    let metrics = PrometheusBuilder::new()
        .install_recorder()
        .map_err(|e| anyhow::anyhow!("metrics recorder install failed: {e}"))?;

    // 限流：配置了 URL 才启用；连接失败直接退出（fail-fast，不带病启动）
    let rate_limit_url = resolve_url("ALPHA_GATEWAY_RATE_LIMIT_URL", &args.rate_limit_url);
    let rate_limiter = if rate_limit_url.is_empty() {
        tracing::info!("rate limit disabled (no --rate-limit-url)");
        None
    } else {
        Some(
            RedisRateLimiter::connect(&rate_limit_url, "alpha:ratelimit:")
                .await
                .map_err(|e| anyhow::anyhow!("rate limiter connect failed: {e}"))?,
        )
    };

    let state = GatewayState {
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(PROXY_TIMEOUT_SECS))
            .build()?,
        data_engine_url: resolve_url("ALPHA_GATEWAY_DATA_ENGINE_URL", &args.data_engine_url),
        realtime_url: resolve_url("ALPHA_GATEWAY_REALTIME_URL", &args.realtime_url),
        collector_url: resolve_url("ALPHA_GATEWAY_COLLECTOR_URL", &args.collector_url),
        rate_limiter,
        rate_limit_per_minute: args.rate_limit_per_minute,
        metrics,
    };

    let app = build_router(state);

    // 启动服务器
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    tracing::info!("API Gateway listening on {}", args.bind);

    axum::serve(listener, app).await?;

    Ok(())
}

fn build_router(state: GatewayState) -> Router {
    // /api 子路由：限流中间件只包 REST 反代面（健康检查与 WS 不占配额）
    let api =
        Router::new()
            .route("/v1/*path", any(api_proxy))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                rate_limit_middleware,
            ));

    Router::new()
        // 健康检查：真实探测三个上游
        .route("/health", get(health_check))
        // Prometheus 抓取端点（scrape 配置见 config/prometheus.yml）
        .route("/metrics", get(metrics_endpoint))
        .nest("/api", api)
        // WebSocket 反代：/ws 与 /ws/<path> → real-time-feed
        .route("/ws", get(ws_proxy_root))
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
                .layer(middleware::from_fn(request_logger)),
        )
        .with_state(state)
}

/// 客户端标识（限流主体）：X-Forwarded-For 首段 → X-Real-IP → "anonymous"。
/// 反代链路下前者由边缘代理注入，最贴近真实来源。
fn client_identity(headers: &HeaderMap) -> String {
    let first_forwarded = |value: &HeaderValue| {
        value
            .to_str()
            .ok()
            .and_then(|s| s.split(',').next())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    headers
        .get("x-forwarded-for")
        .and_then(first_forwarded)
        .or_else(|| headers.get("x-real-ip").and_then(first_forwarded))
        .unwrap_or_else(|| "anonymous".to_string())
}

/// 429 响应：Retry-After（窗口重置秒）+ X-RateLimit-Remaining: 0，JSON 错误体。
fn rate_limit_rejection(decision: &RateDecision) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [
            (
                header::RETRY_AFTER,
                HeaderValue::from(decision.reset_after_secs),
            ),
            (
                header::HeaderName::from_static("x-ratelimit-remaining"),
                HeaderValue::from_static("0"),
            ),
        ],
        Json(serde_json::json!({
            "success": false,
            "error": "rate limit exceeded",
            "retry_after_secs": decision.reset_after_secs,
        })),
    )
        .into_response()
}

/// /api 限流中间件：未启用直接放行；Redis 故障 fail-open（告警放行，
/// 缓存/限流组件不可用不应拖垮网关主链路）。
async fn rate_limit_middleware(
    State(state): State<GatewayState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let Some(limiter) = state.rate_limiter.as_ref() else {
        return next.run(req).await;
    };

    let subject = client_identity(req.headers());
    let decision = match limiter
        .check(
            &subject,
            state.rate_limit_per_minute,
            Duration::from_secs(60),
        )
        .await
    {
        Ok(d) => d,
        Err(err) => {
            tracing::warn!(%subject, %err, "rate limiter unavailable, failing open");
            return next.run(req).await;
        }
    };

    if decision.allowed {
        // L464：放行/拒绝双模计数（GatewayRateLimitExceeded 规则读 mode="denied"）
        metrics::counter!("alpha_gateway_rate_limit_total", "mode" => "allowed").increment(1);
        next.run(req).await
    } else {
        metrics::counter!("alpha_gateway_rate_limit_total", "mode" => "denied").increment(1);
        tracing::warn!(%subject, "rate limit exceeded");
        rate_limit_rejection(&decision)
    }
}

/// Prometheus 抓取端点：渲染全局 metrics recorder 的文本快照
async fn metrics_endpoint(State(state): State<GatewayState>) -> String {
    state.metrics.render()
}

/// 健康检查端点：真实探测上游 /health（data-engine 与 real-time-feed 任一不可达
/// 则整体 degraded），返回实测时延。
async fn health_check(State(state): State<GatewayState>) -> Json<HealthResponse> {
    let (data_engine, realtime, collector) = tokio::join!(
        probe_service(&state.client, "data-engine", &state.data_engine_url),
        probe_service(&state.client, "real-time-feed", &state.realtime_url),
        probe_service(&state.client, "collector", &state.collector_url),
    );

    // collector 探测失败不影响整体可用性判定（网关只反代前两个服务）
    let core_healthy = data_engine.status == "healthy" && realtime.status == "healthy";

    Json(HealthResponse {
        status: if core_healthy {
            "ok".to_string()
        } else {
            "degraded".to_string()
        },
        version: env!("CARGO_PKG_VERSION").to_string(),
        timestamp: chrono::Utc::now(),
        services: vec![data_engine, realtime, collector],
    })
}

/// 探测单个上游的 /health：成功记 healthy 与实测时延，失败记不可达/异常状态。
async fn probe_service(client: &reqwest::Client, name: &str, base_url: &str) -> ServiceStatus {
    let start = Instant::now();
    let url = format!("{}/health", base_url.trim_end_matches('/'));
    let result = client
        .get(&url)
        .timeout(Duration::from_secs(HEALTH_PROBE_TIMEOUT_SECS))
        .send()
        .await;
    let response_time_ms = start.elapsed().as_millis();

    let status = match result {
        Ok(resp) if resp.status().is_success() => ServiceStatus {
            name: name.to_string(),
            status: "healthy".to_string(),
            response_time_ms,
        },
        Ok(resp) => ServiceStatus {
            name: name.to_string(),
            status: format!("unhealthy: upstream {}", resp.status().as_u16()),
            response_time_ms,
        },
        Err(err) => ServiceStatus {
            name: name.to_string(),
            status: format!("unreachable: {err}"),
            response_time_ms,
        },
    };

    // L464 上游健康 gauge（GatewayUpstreamUnhealthy 数据源）。label 名用
    // `upstream` 而非 `job`：抓取时 Prometheus 会用 target 的 job 标签
    // 覆盖指标自报的同名标签（honor_labels 默认 false），job 口径会失真。
    metrics::gauge!(
        "alpha_gateway_service_health",
        "upstream" => name.to_string()
    )
    .set(if status.status == "healthy" { 1.0 } else { 0.0 });
    status
}

fn is_hop_by_hop(name: &str) -> bool {
    HOP_BY_HOP_HEADERS
        .iter()
        .any(|h| name.eq_ignore_ascii_case(h))
}

/// Trace-ID 决策（L460，纯函数）：入站带非空 X-Trace-Id → 原样沿用
/// （跨服务传播同链路）；缺失/空 → 生成 `tr-<uuid>`（网关为链路起点）。
fn resolve_trace_id(headers: &HeaderMap) -> String {
    headers
        .get("x-trace-id")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("tr-{}", uuid::Uuid::new_v4()))
}

fn bad_gateway(message: String) -> Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(serde_json::json!({
            "success": false,
            "error": message,
        })),
    )
        .into_response()
}

/// REST 反代：/api/v1/<path> → data-engine /<path>。
/// 透传请求方法、查询串、请求头（滤除逐跳头）与请求体；上游不可达返回 502。
async fn api_proxy(
    State(state): State<GatewayState>,
    method: Method,
    Path(path): Path<String>,
    original_uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let query = original_uri
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    let upstream_url = format!(
        "{}/{}{}",
        state.data_engine_url.trim_end_matches('/'),
        path,
        query
    );

    // axum(http 1.x) 与 reqwest 0.11(http 0.2) 的 Method 类型不同，按字节转换
    let upstream_method = match reqwest::Method::from_bytes(method.as_str().as_bytes()) {
        Ok(m) => m,
        Err(_) => return bad_gateway(format!("unsupported method: {method}")),
    };

    let mut request = state.client.request(upstream_method, &upstream_url);
    // trace-id 统一在循环后注入（避免入站已有头时 reqwest 追加成双值）
    let trace_id = resolve_trace_id(&headers);
    for (name, value) in headers.iter() {
        if is_hop_by_hop(name.as_str()) || name == "x-trace-id" {
            continue;
        }
        if let Ok(value_str) = value.to_str() {
            request = request.header(name.as_str(), value_str);
        }
    }
    request = request.header("x-trace-id", &trace_id);
    if !body.is_empty() {
        request = request.body(body.to_vec());
    }

    tracing::info!(trace_id = %trace_id, "Proxying {} {} → {}", method, original_uri, upstream_url);

    match request.send().await {
        Ok(upstream) => {
            let status =
                StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            // 响应回填 trace-id：客户端/前端可关联日志与后续请求
            let mut builder = Response::builder()
                .status(status)
                .header("x-trace-id", &trace_id);
            for (name, value) in upstream.headers() {
                if is_hop_by_hop(name.as_str()) || name == "x-trace-id" {
                    continue;
                }
                if let Ok(header_value) = HeaderValue::from_bytes(value.as_bytes()) {
                    builder = builder.header(name.as_str(), header_value);
                }
            }
            match upstream.bytes().await {
                Ok(bytes) => builder
                    .body(axum::body::Body::from(bytes))
                    .unwrap_or_else(|err| bad_gateway(format!("proxy body build failed: {err}"))),
                Err(err) => bad_gateway(format!("upstream body read failed: {err}")),
            }
        }
        Err(err) => {
            tracing::warn!("data-engine unreachable at {}: {}", upstream_url, err);
            bad_gateway(format!("data-engine unreachable: {err}"))
        }
    }
}

/// WS 反代（根路径）：/ws → real-time-feed /ws
async fn ws_proxy_root(state: State<GatewayState>, ws: WebSocketUpgrade) -> Response {
    ws_proxy_inner(state, "/".to_string(), ws).await
}

/// WS 反代（子路径）：/ws/<path> → real-time-feed /<path>
async fn ws_proxy(
    state: State<GatewayState>,
    Path(path): Path<String>,
    ws: WebSocketUpgrade,
) -> Response {
    ws_proxy_inner(state, format!("/{path}"), ws).await
}

async fn ws_proxy_inner(
    state: State<GatewayState>,
    path: String,
    ws: WebSocketUpgrade,
) -> Response {
    let upstream_url = to_ws_url(&state.realtime_url, &path);
    tracing::info!("Proxying WebSocket connection → {}", upstream_url);

    // 先建立上游连接，失败时在升级握手前直接拒绝（返回 502 而非空升级）
    match tokio_tungstenite::connect_async(&upstream_url).await {
        Ok((upstream, _)) => ws.on_upgrade(move |downstream| pump_websocket(downstream, upstream)),
        Err(err) => {
            tracing::warn!("real-time-feed unreachable at {}: {}", upstream_url, err);
            (
                StatusCode::BAD_GATEWAY,
                format!("real-time-feed unreachable: {err}"),
            )
                .into_response()
        }
    }
}

/// http(s) 上游地址 → ws(s) 地址
fn to_ws_url(base_url: &str, path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    let base = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        base.to_string()
    };
    let suffix = if path == "/" {
        String::new()
    } else {
        path.to_string()
    };
    format!("{base}/ws{suffix}")
}

/// 双向泵：下游(axum WS) ⇄ 上游(tokio-tungstenite)，任一侧断开即结束。
async fn pump_websocket(
    downstream: WebSocket,
    upstream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) {
    let (mut down_tx, mut down_rx) = downstream.split();
    let (mut up_tx, mut up_rx) = upstream.split();

    let downstream_to_upstream = async move {
        while let Some(msg) = down_rx.next().await {
            let forward = match msg {
                Ok(msg) => msg,
                Err(_) => break,
            };
            let mapped = match forward {
                WsMessage::Text(text) => Some(tokio_tungstenite::tungstenite::Message::Text(text)),
                WsMessage::Binary(bytes) => {
                    Some(tokio_tungstenite::tungstenite::Message::Binary(bytes))
                }
                WsMessage::Ping(bytes) => {
                    Some(tokio_tungstenite::tungstenite::Message::Ping(bytes))
                }
                WsMessage::Pong(bytes) => {
                    Some(tokio_tungstenite::tungstenite::Message::Pong(bytes))
                }
                WsMessage::Close(frame) => {
                    Some(tokio_tungstenite::tungstenite::Message::Close(frame.map(
                        |f| tokio_tungstenite::tungstenite::protocol::frame::CloseFrame {
                            code: f.code.into(),
                            reason: f.reason.to_string().into(),
                        },
                    )))
                }
            };
            match mapped {
                Some(msg) => {
                    if up_tx.send(msg).await.is_err() {
                        break;
                    }
                }
                None => break,
            }
        }
        let _ = up_tx.close().await;
    };

    let upstream_to_downstream = async move {
        while let Some(msg) = up_rx.next().await {
            let forward = match msg {
                Ok(msg) => msg,
                Err(_) => break,
            };
            let mapped = match forward {
                tokio_tungstenite::tungstenite::Message::Text(text) => Some(WsMessage::Text(text)),
                tokio_tungstenite::tungstenite::Message::Binary(bytes) => {
                    Some(WsMessage::Binary(bytes))
                }
                tokio_tungstenite::tungstenite::Message::Ping(bytes) => {
                    Some(WsMessage::Ping(bytes))
                }
                tokio_tungstenite::tungstenite::Message::Pong(bytes) => {
                    Some(WsMessage::Pong(bytes))
                }
                tokio_tungstenite::tungstenite::Message::Close(frame) => {
                    Some(WsMessage::Close(frame.map(|f| WsCloseFrame {
                        code: f.code.into(),
                        reason: f.reason.to_string().into(),
                    })))
                }
                tokio_tungstenite::tungstenite::Message::Frame(_) => None,
            };
            match mapped {
                Some(msg) => {
                    if down_tx.send(msg).await.is_err() {
                        break;
                    }
                }
                None => break,
            }
        }
        let _ = down_tx.close().await;
    };

    tokio::join!(downstream_to_upstream, upstream_to_downstream);
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

    // L464 业务指标：请求数按方法/状态分桶（alpha-alerts 的 5xx 错误率规则数据源）。
    // metrics 0.22 链式语法（0.21 的内联 value 形态与 exporter 0.13 依赖的
    // 0.22 全局 slot 错位，见根 Cargo.toml 版本统一说明）
    metrics::counter!(
        "alpha_gateway_requests_total",
        "method" => method.as_str().to_string(),
        "status" => response.status().as_u16().to_string()
    )
    .increment(1);

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
    use axum::{
        extract::ws::{WebSocket, WebSocketUpgrade as TestWsUpgrade},
        routing::get as test_get,
    };

    /// 进程级唯一 Prometheus 句柄（install_recorder 每进程一次——首个调用者
    /// install 全局接管 metrics 宏，后续并行测试复用同一句柄渲染；
    /// 与 data-engine/collector 的 global_metrics_handle 同款惯例）
    fn global_metrics_handle() -> PrometheusHandle {
        static HANDLE: std::sync::OnceLock<PrometheusHandle> = std::sync::OnceLock::new();
        HANDLE
            .get_or_init(|| {
                PrometheusBuilder::new()
                    .install_recorder()
                    .unwrap_or_else(|_| PrometheusBuilder::new().build_recorder().handle())
            })
            .clone()
    }

    fn test_state(data_engine_url: String, realtime_url: String) -> GatewayState {
        GatewayState {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(PROXY_TIMEOUT_SECS))
                .build()
                .unwrap(),
            data_engine_url,
            realtime_url,
            collector_url: "http://127.0.0.1:1".to_string(),
            rate_limiter: None,
            rate_limit_per_minute: 120,
            // build_recorder 不占全局 install（install_recorder 每进程一次，
            // 并行测试会冲突）；render 走本 recorder 快照
            metrics: global_metrics_handle(),
        }
    }

    #[test]
    fn resolve_trace_id_propagates_or_generates() {
        let mut headers = HeaderMap::new();
        let generated = resolve_trace_id(&headers);
        assert!(
            generated.starts_with("tr-"),
            "缺失时生成 tr- 前缀: {generated}"
        );

        headers.insert("x-trace-id", "t-123".parse().unwrap());
        assert_eq!(resolve_trace_id(&headers), "t-123", "入站非空原样沿用");

        headers.insert("x-trace-id", "   ".parse().unwrap());
        assert!(
            resolve_trace_id(&headers).starts_with("tr-"),
            "空白值视同缺失"
        );
    }

    /// trace-id 贯穿（L460）：入站带 → 响应原样回填；不带 → 生成并回填
    #[tokio::test]
    async fn api_proxy_propagates_trace_id_in_response() {
        let upstream = spawn_upstream().await;
        let app = build_router(test_state(upstream, "http://127.0.0.1:1".to_string()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let url = format!("http://{addr}/api/v1/stocks/600519/history");
        let body_fn = || {
            reqwest::Client::new()
                .post(&url)
                .header("content-type", "application/json")
                .body("{}")
        };

        let with_id = client
            .post(&url)
            .header("x-trace-id", "t-123")
            .header("content-type", "application/json")
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(
            with_id.headers().get("x-trace-id").unwrap(),
            "t-123",
            "入站 trace-id 应原样回填"
        );

        let without_id = body_fn().send().await.unwrap();
        let generated = without_id
            .headers()
            .get("x-trace-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        assert!(
            generated.starts_with("tr-"),
            "缺失时应生成并回填: {generated}"
        );
    }

    #[test]
    fn client_identity_prefers_forwarded_then_real_ip_then_anonymous() {
        let mut headers = HeaderMap::new();
        assert_eq!(client_identity(&headers), "anonymous");

        headers.insert("x-real-ip", "203.0.113.7".parse().unwrap());
        assert_eq!(client_identity(&headers), "203.0.113.7");

        headers.insert("x-forwarded-for", "198.51.100.9, 10.0.0.1".parse().unwrap());
        assert_eq!(client_identity(&headers), "198.51.100.9", "取首段");
    }

    /// /metrics 端点（L459）：Prometheus 文本格式就绪，Prometheus 抓取契约
    #[tokio::test]
    async fn metrics_endpoint_serves_prometheus_text() {
        let app = build_router(test_state(
            "http://127.0.0.1:1".to_string(),
            "http://127.0.0.1:1".to_string(),
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        // 触发请求计数（request_logger）与上游探测 gauge（probe_service）注册
        let health = client
            .get(format!("http://{addr}/health"))
            .send()
            .await
            .unwrap();
        assert_eq!(health.status().as_u16(), 200);

        let response = client
            .get(format!("http://{addr}/metrics"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 200);
        assert!(
            response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .starts_with("text/plain"),
            "Prometheus 文本协议 content-type 应为 text/plain"
        );
        let body = response.text().await.unwrap();
        // L464 契约：业务指标随 handler 执行注册进全局 recorder 并对外暴露
        assert!(
            body.contains("alpha_gateway_requests_total"),
            "应暴露请求计数指标（5xx 错误率规则数据源）:\n{body}"
        );
        assert!(
            body.contains("alpha_gateway_service_health"),
            "应暴露上游健康 gauge（GatewayUpstreamUnhealthy 数据源）:\n{body}"
        );
        assert!(
            body.contains("upstream=\"data-engine\""),
            "健康 gauge 应按 upstream 标签分桶（job 标签会被抓取覆盖）:\n{body}"
        );
    }

    #[test]
    fn rate_limit_rejection_sets_429_with_retry_after() {
        let response = rate_limit_rejection(&RateDecision {
            allowed: false,
            remaining: 0,
            reset_after_secs: 60,
        });
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let headers = response.headers();
        assert_eq!(headers.get("retry-after").unwrap(), "60");
        assert_eq!(headers.get("x-ratelimit-remaining").unwrap(), "0");
    }

    /// 集成（REDIS_TEST_URL 门控）：配额内放行、超限 429 且带 Retry-After
    #[tokio::test]
    async fn api_rate_limit_enforced_when_limiter_configured() {
        let Some(url) = std::env::var("REDIS_TEST_URL").ok() else {
            return;
        };
        // 独立前缀避免与其他运行互相污染
        let limiter =
            RedisRateLimiter::connect(&url, &format!("alpha:test:gw:{}:", uuid::Uuid::new_v4()))
                .await
                .unwrap();

        let upstream = spawn_upstream().await;
        let mut state = test_state(upstream, "http://127.0.0.1:1".to_string());
        state.rate_limiter = Some(limiter);
        state.rate_limit_per_minute = 2;
        let app = build_router(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let url = format!("http://{addr}/api/v1/stocks/600519/history");
        for i in 1..=2 {
            let resp = client
                .post(&url)
                .header("content-type", "application/json")
                .body("{}")
                .send()
                .await
                .unwrap();
            assert!(
                resp.status().is_success(),
                "第 {i} 个请求在配额内应放行（上游可达）"
            );
        }

        let third = client
            .post(&url)
            .header("content-type", "application/json")
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(third.status().as_u16(), 429, "超限应拒绝");
        assert_eq!(third.headers().get("retry-after").unwrap(), "60");

        // 限流面之外不受影响：健康检查不占配额（走到网关处理器而非 429）
        let health = client
            .get(format!("http://{addr}/health"))
            .send()
            .await
            .unwrap();
        assert_eq!(
            health.status().as_u16(),
            200,
            "健康检查不应被 /api 限流拦截"
        );

        // L464 指标契约：放行/拒绝计数已发生，/metrics 应暴露 rate_limit_total
        // （GatewayRateLimitExceeded 规则的 mode="denied" 数据源）
        let metrics_body = client
            .get(format!("http://{addr}/metrics"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(
            metrics_body.contains("alpha_gateway_rate_limit_total"),
            "限流计数指标应暴露:\n{metrics_body}"
        );
        assert!(
            metrics_body.contains("mode=\"denied\""),
            "拒绝计数应带 mode=denied 标签:\n{metrics_body}"
        );
    }

    /// 在临时端口起一个带 /health、/echo 路由的上游 axum 服务，返回其基地址。
    async fn spawn_upstream() -> String {
        let app = Router::new()
            .route("/health", test_get(|| async { "ok" }))
            .route(
                "/stocks/:symbol/history",
                axum::routing::post(upstream_history_echo),
            )
            .route(
                "/ws",
                test_get(|ws: TestWsUpgrade| async move {
                    ws.on_upgrade(|socket: WebSocket| async move {
                        let (mut tx, mut rx) = socket.split();
                        while let Some(Ok(msg)) = rx.next().await {
                            if let WsMessage::Text(text) = msg {
                                if tx
                                    .send(WsMessage::Text(format!("echo:{text}")))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                    })
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    /// 上游桩：回显路径参数与请求体（供反代透传断言）。
    async fn upstream_history_echo(
        axum::extract::Path(symbol): axum::extract::Path<String>,
        axum::Json(body): axum::Json<serde_json::Value>,
    ) -> axum::Json<serde_json::Value> {
        axum::Json(serde_json::json!({
            "symbol": symbol,
            "echo_body": body,
        }))
    }

    #[tokio::test]
    async fn health_check_probes_real_upstreams_and_flags_degraded() {
        let upstream = spawn_upstream().await;
        let state = test_state(upstream.clone(), "http://127.0.0.1:1".to_string());
        let response = health_check(State(state)).await;
        let health = response.0;

        assert_eq!(health.status, "degraded", "real-time-feed 不可达应降级");
        let data_engine = health
            .services
            .iter()
            .find(|s| s.name == "data-engine")
            .unwrap();
        assert_eq!(data_engine.status, "healthy");
        let realtime = health
            .services
            .iter()
            .find(|s| s.name == "real-time-feed")
            .unwrap();
        assert!(realtime.status.starts_with("unreachable"));
    }

    #[tokio::test]
    async fn api_proxy_forwards_method_path_query_and_body() {
        let upstream = spawn_upstream().await;
        let app = build_router(test_state(upstream, "http://127.0.0.1:1".to_string()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/api/v1/stocks/600519/history"))
            .header("x-trace-id", "t-123")
            .query(&[("days", "5")])
            .header("content-type", "application/json")
            .body(r#"{"range":"1m"}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 200, "上游 200 应原样透传");
        let payload: serde_json::Value = response.json().await.unwrap();
        assert_eq!(payload["symbol"], "600519", "路径参数应正确映射到上游");
        assert_eq!(payload["echo_body"]["range"], "1m", "请求体应透传");
    }

    #[tokio::test]
    async fn api_proxy_returns_502_when_upstream_unreachable() {
        let app = build_router(test_state(
            "http://127.0.0.1:1".to_string(),
            "http://127.0.0.1:1".to_string(),
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let response = reqwest::Client::new()
            .get(format!("http://{addr}/api/v1/stocks/600519/history"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 502);
        let payload: serde_json::Value = response.json().await.unwrap();
        assert_eq!(payload["success"], false);
    }

    #[tokio::test]
    async fn ws_proxy_round_trips_messages_to_upstream() {
        let upstream = spawn_upstream().await;
        // WS 反代目标是第二个参数（realtime_url），data-engine 侧给死端口即可
        let app = build_router(test_state("http://127.0.0.1:1".to_string(), upstream));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let (mut ws_stream, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
            .await
            .unwrap();
        use futures_util::SinkExt as TestSink;
        use futures_util::StreamExt as TestStream;
        ws_stream
            .send(tokio_tungstenite::tungstenite::Message::Text("ping".into()))
            .await
            .unwrap();
        let echoed = tokio::time::timeout(Duration::from_secs(5), ws_stream.next())
            .await
            .expect("echo within timeout")
            .expect("message received")
            .unwrap();
        assert_eq!(
            echoed,
            tokio_tungstenite::tungstenite::Message::Text("echo:ping".into())
        );
    }

    #[test]
    fn to_ws_url_converts_schemes_and_appends_path() {
        assert_eq!(to_ws_url("http://h:8082", "/"), "ws://h:8082/ws");
        assert_eq!(to_ws_url("http://h:8082/", "/"), "ws://h:8082/ws");
        assert_eq!(to_ws_url("https://h", "/sub"), "wss://h/ws/sub");
    }

    #[test]
    fn health_probe_marks_unreachable_upstream() {
        // probe_service 对拒绝连接的端口应报 unreachable 而非 panic
        let client = reqwest::Client::new();
        let status = {
            let rt = tokio_runtime();
            rt.block_on(probe_service(&client, "dead", "http://127.0.0.1:1"))
        };
        assert!(status.status.starts_with("unreachable"));
    }

    fn tokio_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }
}
