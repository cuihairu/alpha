//! 反代层（原 main.rs 998-1296 行）：REST 反代 data-engine、WebSocket
//! 反代 real-time-feed（握手前鉴权）、双向泵、trace-id 与逐跳头处理。

use crate::middleware::unauthorized;
use crate::{auth, GatewayState, HOP_BY_HOP_HEADERS};
use axum::{
    body::Bytes,
    extract::{
        ws::{CloseFrame as WsCloseFrame, Message as WsMessage, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri},
    response::{IntoResponse, Json, Response},
};
use futures_util::{SinkExt, StreamExt};

pub(crate) fn is_hop_by_hop(name: &str) -> bool {
    HOP_BY_HOP_HEADERS
        .iter()
        .any(|h| name.eq_ignore_ascii_case(h))
}

/// Trace-ID 决策（L460，纯函数）：入站带非空 X-Trace-Id → 原样沿用
/// （跨服务传播同链路）；缺失/空 → 生成 `tr-<uuid>`（网关为链路起点）。
pub(crate) fn resolve_trace_id(headers: &HeaderMap) -> String {
    headers
        .get("x-trace-id")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("tr-{}", uuid::Uuid::new_v4()))
}

pub(crate) fn bad_gateway(message: String) -> Response {
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
pub(crate) async fn api_proxy(
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
pub(crate) async fn ws_proxy_root(
    state: State<GatewayState>,
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    ws_proxy_inner(state, "/".to_string(), ws, &headers, &uri).await
}

/// WS 反代（子路径）：/ws/<path> → real-time-feed /<path>
pub(crate) async fn ws_proxy(
    state: State<GatewayState>,
    Path(path): Path<String>,
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    ws_proxy_inner(state, format!("/{path}"), ws, &headers, &uri).await
}

pub(crate) async fn ws_proxy_inner(
    state: State<GatewayState>,
    path: String,
    ws: WebSocketUpgrade,
    headers: &HeaderMap,
    uri: &Uri,
) -> Response {
    // WS 握手鉴权（auth.md §6 遗留项闭合）：jwt 模式下升级请求必须持票，
    // 校验在拨上游之前——未认证流量不触发 real-time-feed 连接。校验分流
    // 与 REST 同核（verify_any：JWKS 在位且票带 kid → OIDC，否则自签）。
    // WS 是只读订阅面，任意已认证角色放行（与 REST 读路径同口径，不走
    // authorize）。off 模式（默认）零行为变化。
    if state.auth.mode != auth::AuthMode::Off {
        let verdict = match ws_auth_token(headers, uri) {
            Some(token) => match state.jwks.as_ref() {
                Some(store) => {
                    let guard = store.read().await;
                    Some(auth::verify_any(&state.auth, Some(&guard), &token))
                }
                None => Some(auth::verify_any(&state.auth, None, &token)),
            },
            None => None,
        };
        match verdict {
            Some(Ok(claims)) => {
                metrics::counter!("alpha_gateway_auth_total", "mode" => "ws_allowed").increment(1);
                tracing::debug!(sub = %claims.sub, "ws handshake authenticated");
            }
            verdict => {
                metrics::counter!("alpha_gateway_auth_total", "mode" => "ws_denied").increment(1);
                if verdict.is_none() {
                    tracing::debug!("ws handshake rejected: no token");
                }
                return unauthorized();
            }
        }
    }

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

/// WS 握手 token 提取：`Authorization: Bearer` 头优先，回落 `?token=` 查询参数
/// （浏览器侧 WebSocket 无法自定义握手头，查询参数是标准回落通道）。JWS compact
/// 序列化（base64url + `.`）全部是未保留字符，查询参数无需 percent 解码。
pub(crate) fn ws_auth_token(headers: &HeaderMap, uri: &Uri) -> Option<String> {
    if let Some(token) = auth::extract_bearer(headers) {
        return Some(token);
    }
    uri.query()?.split('&').find_map(|kv| {
        let (key, value) = kv.split_once('=')?;
        (key == "token" && !value.is_empty()).then(|| value.to_string())
    })
}

/// http(s) 上游地址 → ws(s) 地址
pub(crate) fn to_ws_url(base_url: &str, path: &str) -> String {
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
pub(crate) async fn pump_websocket(
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
