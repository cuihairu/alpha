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
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri},
    middleware,
    response::{IntoResponse, Json, Response},
    routing::{any, get},
    Router,
};
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
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

    let state = GatewayState {
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(PROXY_TIMEOUT_SECS))
            .build()?,
        data_engine_url: resolve_url("ALPHA_GATEWAY_DATA_ENGINE_URL", &args.data_engine_url),
        realtime_url: resolve_url("ALPHA_GATEWAY_REALTIME_URL", &args.realtime_url),
        collector_url: resolve_url("ALPHA_GATEWAY_COLLECTOR_URL", &args.collector_url),
    };

    let app = build_router(state);

    // 启动服务器
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    tracing::info!("API Gateway listening on {}", args.bind);

    axum::serve(listener, app).await?;

    Ok(())
}

fn build_router(state: GatewayState) -> Router {
    Router::new()
        // 健康检查：真实探测三个上游
        .route("/health", get(health_check))
        // REST 反代：/api/v1/<path> → data-engine /<path>（透传方法/查询/头/体）
        .route("/api/v1/*path", any(api_proxy))
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

    match result {
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
    }
}

fn is_hop_by_hop(name: &str) -> bool {
    HOP_BY_HOP_HEADERS
        .iter()
        .any(|h| name.eq_ignore_ascii_case(h))
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
    for (name, value) in headers.iter() {
        if is_hop_by_hop(name.as_str()) {
            continue;
        }
        if let Ok(value_str) = value.to_str() {
            request = request.header(name.as_str(), value_str);
        }
    }
    if !body.is_empty() {
        request = request.body(body.to_vec());
    }

    tracing::info!("Proxying {} {} → {}", method, original_uri, upstream_url);

    match request.send().await {
        Ok(upstream) => {
            let status = StatusCode::from_u16(upstream.status().as_u16())
                .unwrap_or(StatusCode::BAD_GATEWAY);
            let mut builder = Response::builder().status(status);
            for (name, value) in upstream.headers() {
                if is_hop_by_hop(name.as_str()) {
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
async fn ws_proxy_root(
    state: State<GatewayState>,
    ws: WebSocketUpgrade,
) -> Response {
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

async fn ws_proxy_inner(state: State<GatewayState>, path: String, ws: WebSocketUpgrade) -> Response {
    let upstream_url = to_ws_url(&state.realtime_url, &path);
    tracing::info!("Proxying WebSocket connection → {}", upstream_url);

    // 先建立上游连接，失败时在升级握手前直接拒绝（返回 502 而非空升级）
    match tokio_tungstenite::connect_async(&upstream_url).await {
        Ok((upstream, _)) => ws.on_upgrade(move |downstream| {
            pump_websocket(downstream, upstream)
        }),
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
    let suffix = if path == "/" { String::new() } else { path.to_string() };
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
                WsMessage::Close(frame) => Some(tokio_tungstenite::tungstenite::Message::Close(
                    frame.map(|f| tokio_tungstenite::tungstenite::protocol::frame::CloseFrame {
                        code: f.code.into(),
                        reason: f.reason.to_string().into(),
                    }),
                )),
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
                tokio_tungstenite::tungstenite::Message::Ping(bytes) => Some(WsMessage::Ping(bytes)),
                tokio_tungstenite::tungstenite::Message::Pong(bytes) => Some(WsMessage::Pong(bytes)),
                tokio_tungstenite::tungstenite::Message::Close(frame) => Some(WsMessage::Close(
                    frame.map(|f| WsCloseFrame {
                        code: f.code.into(),
                        reason: f.reason.to_string().into(),
                    }),
                )),
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

    fn test_state(data_engine_url: String, realtime_url: String) -> GatewayState {
        GatewayState {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(PROXY_TIMEOUT_SECS))
                .build()
                .unwrap(),
            data_engine_url,
            realtime_url,
            collector_url: "http://127.0.0.1:1".to_string(),
        }
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
                                if tx.send(WsMessage::Text(format!("echo:{text}"))).await.is_err()
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

        let (mut ws_stream, _) =
            tokio_tungstenite::connect_async(format!("ws://{addr}/ws")).await.unwrap();
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
