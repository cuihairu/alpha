
use super::*;
use crate::middleware::*;
use crate::proxy::*;
use crate::routes::*;
use alpha_storage::RateDecision;
use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade as TestWsUpgrade},
        State,
    },
    http::{HeaderMap, HeaderValue, StatusCode, Uri},
    routing::get as test_get,
};
use futures_util::{SinkExt, StreamExt};
use std::collections::BTreeMap;

/// 存储 trait 方法（账户写穿测试用；`as _` = 只要方法不要名字）
use alpha_storage::StorageBackend as _;

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
        shield: shield::ShieldState::from_config(shield::ShieldConfig::default()),
        audit: audit::AuditState::new(60_000, 20),
        account_store: account::AccountStore::in_memory(),
        auth: auth::AuthConfig::disabled(),
        jwks: None,
        // build_recorder 不占全局 install（install_recorder 每进程一次，
        // 并行测试会冲突）；render 走本 recorder 快照
        metrics: global_metrics_handle(),
    }
}

/// 护栏面（L486）测试装配：上游不可达（透传 502），断言区分
/// 403/429 与透传状态
async fn spawn_shield_app(config: shield::ShieldConfig) -> String {
    let mut state = test_state(
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    );
    state.shield = shield::ShieldState::from_config(config);
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// burst 护栏：桶深耗尽 → 同身份 429；不同身份独立桶不受牵连
#[tokio::test]
async fn shield_burst_guard_rejects_flood_within_bucket_depth() {
    // refill 取近零值：默认 50/s（20ms 回一枚）会让隔调度间隙的两发
    // 请求被补出的令牌放行——本测只锁「桶深耗尽即拒」，refill 行为
    // 由 BurstGuard 单测（burst_guard_absorbs_burst_then_refills）覆盖
    let base = spawn_shield_app(shield::ShieldConfig {
        burst_capacity: 1,
        burst_refill_per_sec: 0.001,
        ..Default::default()
    })
    .await;
    let client = reqwest::Client::new();
    let url = format!("{base}/api/v1/stocks/X/history");
    let first = client
        .get(&url)
        .header("x-forwarded-for", "10.1.1.1")
        .send()
        .await
        .unwrap();
    assert_ne!(first.status().as_u16(), 429, "桶深 1：首发放行");
    let second = client
        .get(&url)
        .header("x-forwarded-for", "10.1.1.1")
        .send()
        .await
        .unwrap();
    assert_eq!(second.status().as_u16(), 429, "同身份秒级 burst 超桶拒绝");
    let other = client
        .get(&url)
        .header("x-forwarded-for", "10.1.1.2")
        .send()
        .await
        .unwrap();
    assert_ne!(other.status().as_u16(), 429, "不同身份独立桶");
}

/// 扫描检测：同身份窗口内离散路径超阈 → 403；重复热点路径不误伤
#[tokio::test]
async fn shield_scan_detector_rejects_broad_crawling() {
    let base = spawn_shield_app(shield::ShieldConfig {
        scan_max_distinct: 2,
        ..Default::default()
    })
    .await;
    let client = reqwest::Client::new();
    for path in ["/api/v1/a", "/api/v1/b"] {
        let res = client
            .get(format!("{base}{path}"))
            .header("x-forwarded-for", "10.2.2.2")
            .send()
            .await
            .unwrap();
        assert_ne!(res.status().as_u16(), 403, "{path} 阈内放行");
    }
    let third = client
        .get(format!("{base}/api/v1/c"))
        .header("x-forwarded-for", "10.2.2.2")
        .send()
        .await
        .unwrap();
    assert_eq!(third.status().as_u16(), 403, "第 3 条离散路径越阈");
}

/// bot 拒绝：默认关（脚本 UA 透传不破坏既有调用方），开启后命中 403
#[tokio::test]
async fn shield_bot_deny_off_by_default_on_when_enabled() {
    let client = reqwest::Client::new();
    let base = spawn_shield_app(shield::ShieldConfig::default()).await;
    let passthrough = client
        .get(format!("{base}/api/v1/stocks/X/history"))
        .header("user-agent", "curl/8.5.0")
        .header("x-forwarded-for", "10.3.3.3")
        .send()
        .await
        .unwrap();
    assert_ne!(
        passthrough.status().as_u16(),
        403,
        "默认 bot_deny 关，脚本 UA 不拒绝"
    );

    let base = spawn_shield_app(shield::ShieldConfig {
        bot_deny: true,
        ..Default::default()
    })
    .await;
    let blocked = client
        .get(format!("{base}/api/v1/stocks/X/history"))
        .header("user-agent", "curl/8.5.0")
        .header("x-forwarded-for", "10.3.3.4")
        .send()
        .await
        .unwrap();
    assert_eq!(blocked.status().as_u16(), 403);
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

/// 审计面（L487）：错票据风暴 → AuthFailure 计数与风暴异常位在
/// /metrics 可见；rbac 拒绝 → AccessDenied 计数在位
#[tokio::test]
async fn audit_emits_on_auth_failures_and_storms() {
    let mut state = test_state(
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    );
    state.auth = auth::AuthConfig {
        mode: auth::AuthMode::JwtRequired,
        secret: "test-secret".to_string(),
        expected_issuer: String::new(),
        expected_audience: String::new(),
        provision_key: String::new(),
    };
    // 阈值 3：3 次错票据即风暴
    state.audit = audit::AuditState::new(60_000, 3);
    let metrics_handle = state.metrics.clone();
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let url = format!("http://{addr}/api/v1/stocks/X/history");
    for _ in 0..3 {
        let res = client
            .get(&url)
            .header("authorization", "Bearer not-a-token")
            .header("x-forwarded-for", "10.9.9.9")
            .send()
            .await
            .unwrap();
        assert_eq!(res.status().as_u16(), 401);
    }
    let rendered = metrics_handle.render();
    assert!(
        rendered.contains("alpha_gateway_audit_total"),
        "AuthFailure 审计计数应在 /metrics 在位"
    );
    assert!(
        rendered.contains("alpha_gateway_audit_anomaly_total"),
        "风暴异常位应在 /metrics 在位：{rendered}"
    );
}

/// 认证中间件（L483）：jwt 模式无 token → 401；合法 token → 放行；
/// /health 与 /auth/token 不鉴权
#[tokio::test]
async fn api_auth_enforced_in_jwt_mode() {
    let upstream = spawn_upstream().await;
    let mut state = test_state(upstream, "http://127.0.0.1:1".to_string());
    state.auth = auth::AuthConfig {
        mode: auth::AuthMode::JwtRequired,
        secret: "test-secret".to_string(),
        expected_issuer: String::new(),
        expected_audience: String::new(),
        provision_key: "provision-test-key".to_string(),
    };
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let api_url = format!("http://{addr}/api/v1/stocks/600519/history");

    // 无 token → 401 + WWW-Authenticate
    let denied = client
        .post(&api_url)
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status().as_u16(), 401);
    assert!(
        denied
            .headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .starts_with("Bearer"),
        "401 应带 WWW-Authenticate: Bearer"
    );

    // 错 secret 签的 → 401（不区分原因）
    let bad = client
        .post(&api_url)
        .bearer_auth(
            auth::create_token("wrong-secret", "mallory", "", Duration::from_secs(60)).unwrap(),
        )
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status().as_u16(), 401);

    // /auth/token 签发（operator 角色）→ 持票放行 200
    let provisioned: serde_json::Value = client
        .post(format!("http://{addr}/auth/token"))
        .header("x-provision-key", "provision-test-key")
        .header("content-type", "application/json")
        .body(r#"{"sub":"alice","scope":"read:quotes","roles":["operator"]}"#)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(provisioned["success"], true);
    let token = provisioned["access_token"].as_str().unwrap();

    let allowed = client
        .post(&api_url)
        .bearer_auth(token)
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(allowed.status().as_u16(), 200, "合法 token 应放行");

    // L484：无角色票据 POST → 403（viewer 只读）
    let viewer_token =
        auth::create_token("test-secret", "mallory", "", Duration::from_secs(60)).unwrap();
    let forbidden_resp = client
        .post(&api_url)
        .bearer_auth(viewer_token)
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(forbidden_resp.status().as_u16(), 403);
    let payload: serde_json::Value = forbidden_resp.json().await.unwrap();
    assert_eq!(payload["success"], false);

    // /health 在 jwt 模式下仍公开（探活不能要求先登录）
    let health = client
        .get(format!("http://{addr}/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(health.status().as_u16(), 200);
}

/// provision_key 为空 → /auth/token 503 关闭（默认不扩大攻击面）
#[tokio::test]
async fn provision_token_disabled_without_key() {
    let app = build_router(test_state(
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let resp = reqwest::Client::new()
        .post(format!("http://{addr}/auth/token"))
        .header("content-type", "application/json")
        .body(r#"{"sub":"alice"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 503);
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

/// 账户面装配（L476）：认证关闭 → 请求落到本机缺省账户（单账户本地形态）
async fn spawn_account_app(state: GatewayState) -> String {
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

fn account_test_state() -> GatewayState {
    test_state(
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    )
}

/// TLS 装配解析（L485）：都缺省反代终结、齐备启用、只给其一拒绝启动
#[test]
fn resolve_tls_requires_both_paths() {
    assert_eq!(resolve_tls("", "").unwrap(), None);
    assert_eq!(
        resolve_tls("/etc/alpha/tls.crt", "/etc/alpha/tls.key").unwrap(),
        Some((
            "/etc/alpha/tls.crt".to_string(),
            "/etc/alpha/tls.key".to_string()
        ))
    );
    assert!(resolve_tls("/c.crt", "").is_err());
    assert!(resolve_tls("", "/k.key").is_err());
}

/// TLS 端到端（L485）：服务内 rustls 起服后 https 握手可达。rcgen 进程内
/// 自签证书落临时文件，走与生产完全一致的 from_pem_file 装配路径
#[tokio::test]
async fn tls_serving_serves_https_end_to_end() {
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let dir = std::env::temp_dir().join(format!("alpha-gw-tls-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    std::fs::write(&cert_path, certified.cert.pem()).unwrap();
    std::fs::write(&key_path, certified.key_pair.serialize_pem()).unwrap();

    // bind_rustls 不暴露 local_addr：占位取空闲端口后让位再绑
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);

    let app = build_router(account_test_state());
    let config = axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert_path, &key_path)
        .await
        .unwrap();
    let handle = tokio::spawn(async move {
        axum_server::bind_rustls(addr, config)
            .serve(app.into_make_service())
            .await
            .unwrap();
    });

    // 自签证书接受无效校验——只为验证「服务内 TLS 握手 + 请求可达」这一装配本身
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();
    // 监听在 spawn 首次 poll 时才绑定，短轮询等就绪
    let mut status = 0u16;
    for _ in 0..50 {
        match client.get(format!("https://{addr}/health")).send().await {
            Ok(resp) => {
                status = resp.status().as_u16();
                break;
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(50)).await,
        }
    }
    // /health 上游不可达时返回 degraded 但仍是 200（装配正确性只看状态码）
    assert_eq!(status, 200, "https 请求应达 200");

    handle.abort();
    let _ = std::fs::remove_dir_all(&dir);
}

async fn post_json(client: &reqwest::Client, url: &str, body: &str) -> serde_json::Value {
    client
        .post(url)
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

/// GET 建初始档案、PUT 字段级更新（缺省字段不改，非法值 400 且不推进 rev）
#[tokio::test]
async fn account_profile_get_creates_and_put_patches_fields() {
    let base = spawn_account_app(account_test_state()).await;
    let client = reqwest::Client::new();
    let url = format!("{base}/api/v1/account/profile");

    let created: serde_json::Value = client.get(&url).send().await.unwrap().json().await.unwrap();
    assert_eq!(created["account_id"], alpha_core::account::LOCAL_ACCOUNT_ID);
    assert_eq!(
        created["display_name"], "本机用户",
        "缺省展示名不编造用户身份"
    );
    assert_eq!(created["rev"], 1);
    assert!(
        created.get("email").is_none(),
        "缺省档案不带空邮箱字段（省流量的编码惯例）: {created}"
    );

    let patched: serde_json::Value = client
        .put(&url)
        .header("content-type", "application/json")
        .body(r#"{"display_name":"Cui","email":" cui@example.com ","locale":"zh-CN"}"#)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(patched["display_name"], "Cui");
    assert_eq!(patched["email"], "cui@example.com", "邮箱去空白");
    assert_eq!(patched["locale"], "zh-CN");
    assert_eq!(patched["rev"], 2);

    // 字段级更新：只给 locale 时其余字段保持（不是整体替换）
    let locale_only: serde_json::Value = client
        .put(&url)
        .header("content-type", "application/json")
        .body(r#"{"locale":"en-US"}"#)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(locale_only["display_name"], "Cui");
    assert_eq!(locale_only["email"], "cui@example.com");
    assert_eq!(locale_only["rev"], 3);

    // 非法字段 → 400 且不推进 rev（失败的写不得改变服务端状态）
    let rejected = client
        .put(&url)
        .header("content-type", "application/json")
        .body(r#"{"display_name":"   "}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status().as_u16(), 400);
    let after: serde_json::Value = client.get(&url).send().await.unwrap().json().await.unwrap();
    assert_eq!(after["rev"], 3, "被拒的更新不得推进版本");
}

/// 同步往返：接受 → 旧基线冲突回权威副本 → 带权威 rev 重推 → 墓碑留痕
#[tokio::test]
async fn account_sync_round_trips_over_http() {
    let base = spawn_account_app(account_test_state()).await;
    let client = reqwest::Client::new();
    let url = format!("{base}/api/v1/account/sync");

    let pushed = post_json(
            &client,
            &url,
            r#"{"cursor":0,"base":{},"pushes":[{"key":"workspace:a","base_rev":0,"payload":{"name":"盯盘"}}]}"#,
        )
        .await;
    assert_eq!(pushed["accepted"][0]["rev"], 1, "服务端分配权威 rev");
    assert!(
        pushed["accepted"][0]["updated_at_ms"].as_i64().unwrap() > 0,
        "时间戳以服务端时钟为准（客户端时钟不可信）: {pushed}"
    );
    assert_eq!(pushed["cursor"], 1);
    assert_eq!(pushed["changes"].as_array().unwrap().len(), 1);

    // 旧 base_rev → 冲突 + 权威副本（乐观并发，非最后写入者胜）
    let stale = post_json(
            &client,
            &url,
            r#"{"cursor":1,"base":{"workspace:a":0},"pushes":[{"key":"workspace:a","base_rev":0,"payload":{"name":"打板"}}]}"#,
        )
        .await;
    assert!(stale["accepted"].as_array().unwrap().is_empty());
    assert_eq!(stale["rejected"][0]["reason"], "conflict");
    assert_eq!(stale["rejected"][0]["server"]["payload"]["name"], "盯盘");
    assert!(
        stale["changes"].as_array().unwrap().is_empty(),
        "水位之后无变更 = 空增量: {stale}"
    );
    assert_eq!(stale["cursor"], 1, "被拒的推送不消耗水位");

    // 带权威 rev 重推 → rev 推进
    let retried = post_json(
            &client,
            &url,
            r#"{"cursor":1,"base":{"workspace:a":1},"pushes":[{"key":"workspace:a","base_rev":1,"payload":{"name":"打板"}}]}"#,
        )
        .await;
    assert_eq!(retried["accepted"][0]["rev"], 2);
    assert_eq!(retried["accepted"][0]["payload"]["name"], "打板");

    // 墓碑走同一路径（删除要留痕，否则传不到其他端）
    let deleted = post_json(
            &client,
            &url,
            r#"{"cursor":2,"pushes":[{"key":"workspace:a","base_rev":2,"deleted":true,"payload":null}]}"#,
        )
        .await;
    assert_eq!(deleted["accepted"][0]["deleted"], true);
    assert_eq!(deleted["accepted"][0]["rev"], 3);
    assert!(deleted["accepted"][0]["payload"].is_null());

    // 键形非法在服务端兜一层（客户端已拦，这里锁服务端不依赖客户端自律）
    let bad_key = post_json(
        &client,
        &url,
        r#"{"cursor":3,"pushes":[{"key":"nope","base_rev":0,"payload":null}]}"#,
    )
    .await;
    assert_eq!(bad_key["rejected"][0]["reason"], "invalid_key");
    assert!(bad_key["rejected"][0].get("server").is_none());
}

/// 账户隔离：认证开启后按 token 的 sub 分区，一个账户推的数据另一个拉不到
#[tokio::test]
async fn account_endpoints_partition_by_token_sub() {
    let mut state = account_test_state();
    state.auth = auth::AuthConfig {
        mode: auth::AuthMode::JwtRequired,
        secret: "test-secret".to_string(),
        expected_issuer: String::new(),
        expected_audience: String::new(),
        provision_key: String::new(),
    };
    let base = spawn_account_app(state).await;
    let client = reqwest::Client::new();
    let profile_url = format!("{base}/api/v1/account/profile");
    let sync_url = format!("{base}/api/v1/account/sync");

    // 未认证不得读写任何账户
    assert_eq!(
        client
            .get(&profile_url)
            .send()
            .await
            .unwrap()
            .status()
            .as_u16(),
        401
    );
    assert_eq!(
        client
            .post(&sync_url)
            .header("content-type", "application/json")
            .body(r#"{"cursor":0,"base":{},"pushes":[]}"#)
            .send()
            .await
            .unwrap()
            .status()
            .as_u16(),
        401
    );

    // viewer 票据：认证过了，但同步是写方法 → 403（账户面同样受 RBAC 约束）
    let viewer = auth::create_token("test-secret", "carol", "", Duration::from_secs(60)).unwrap();
    assert_eq!(
        client
            .post(&sync_url)
            .bearer_auth(&viewer)
            .header("content-type", "application/json")
            .body(r#"{"cursor":0,"base":{},"pushes":[]}"#)
            .send()
            .await
            .unwrap()
            .status()
            .as_u16(),
        403
    );

    // 同步端点是写方法 → 需 operator+ 角色（viewer 只读，L484）
    let alice = auth::create_token_with_roles(
        "test-secret",
        "alice",
        "",
        &["operator".to_string()],
        Duration::from_secs(60),
    )
    .unwrap();
    let bob = auth::create_token_with_roles(
        "test-secret",
        "bob",
        "",
        &["operator".to_string()],
        Duration::from_secs(60),
    )
    .unwrap();

    let alice_profile: serde_json::Value = client
        .get(&profile_url)
        .bearer_auth(&alice)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(alice_profile["account_id"], "alice", "档案按 sub 分区");

    post_json_typed(
            &client,
            &sync_url,
            &alice,
            r#"{"cursor":0,"base":{},"pushes":[{"key":"workspace:a","base_rev":0,"payload":{"secret":"A"}}]}"#,
        )
        .await;

    // bob 从 0 水位拉不到 alice 的记录
    let bob_view = post_json_typed(
        &client,
        &sync_url,
        &bob,
        r#"{"cursor":0,"base":{},"pushes":[]}"#,
    )
    .await;
    assert!(
        bob_view["changes"].as_array().unwrap().is_empty(),
        "账户间记录不可见: {bob_view}"
    );
    let bob_profile: serde_json::Value = client
        .get(&profile_url)
        .bearer_auth(&bob)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(bob_profile["account_id"], "bob");
    assert_eq!(bob_profile["display_name"], "bob");

    // alice 自己拉得到（确认上一条不是「谁都没数据」）
    let alice_view = post_json_typed(
        &client,
        &sync_url,
        &alice,
        r#"{"cursor":0,"base":{},"pushes":[]}"#,
    )
    .await;
    assert_eq!(alice_view["changes"].as_array().unwrap().len(), 1);
}

async fn post_json_typed(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    body: &str,
) -> serde_json::Value {
    client
        .post(url)
        .bearer_auth(token)
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

/// 写穿持久化（L476）：改动写穿后端；换一份空内存存储（模拟重启）能恢复
#[tokio::test]
async fn account_store_write_through_survives_fresh_memory() {
    // 同一后端实例挂两份存储 = 进程重启后内存空、后端还在
    let backend = Arc::new(alpha_storage::MemoryStorage::new());
    let store = account::AccountStore::with_persistence(backend.clone());
    store.profile("alice", 100).await;
    let response = store
        .sync(
            "alice",
            &alpha_core::account::SyncRequest {
                cursor: 0,
                base: BTreeMap::new(),
                pushes: vec![alpha_core::account::SyncPush {
                    key: "workspace:a".into(),
                    base_rev: 0,
                    deleted: false,
                    payload: serde_json::json!({"n": 1}),
                    updated_at_ms: 0,
                }],
            },
            200,
        )
        .await;
    assert_eq!(response.accepted.len(), 1);
    let keys = backend.list_keys("alpha:account:").await.unwrap();
    assert_eq!(keys.len(), 1, "账户状态整体落一个键");
    assert!(
        keys[0].starts_with("alpha:account:"),
        "键带账户前缀（slug 化 sub）: {}",
        keys[0]
    );

    // 新存储 = 空内存 + 同一后端（等价于进程重启）
    let fresh = account::AccountStore::with_persistence(backend);
    let view = fresh
        .sync(
            "alice",
            &alpha_core::account::SyncRequest {
                cursor: 0,
                base: BTreeMap::new(),
                pushes: vec![],
            },
            300,
        )
        .await;
    assert_eq!(view.changes.len(), 1, "记录应从快照恢复");
    assert_eq!(view.changes[0].rev, 1);
    assert_eq!(view.changes[0].payload, serde_json::json!({"n": 1}));
    assert_eq!(fresh.profile("alice", 400).await.rev, 1, "档案随快照恢复");
    assert_eq!(fresh.profile("alice", 500).await.display_name, "alice");
}

/// 写穿失败只告警不拒绝请求（内存仍是权威 serving 层——多副本下由
/// 下次写穿收敛；把 5xx 抛给客户端会让一次后端抖动变成用户可见的数据丢失）
#[tokio::test]
async fn account_store_write_through_failure_still_serves() {
    let store = account::AccountStore::with_persistence(Arc::new(FailingWriteBackend));
    let response = store
        .sync(
            "alice",
            &alpha_core::account::SyncRequest {
                cursor: 0,
                base: BTreeMap::new(),
                pushes: vec![alpha_core::account::SyncPush {
                    key: "workspace:a".into(),
                    base_rev: 0,
                    deleted: false,
                    payload: serde_json::json!({"n": 1}),
                    updated_at_ms: 0,
                }],
            },
            100,
        )
        .await;
    assert_eq!(response.accepted.len(), 1, "后端写失败不应拒绝同步请求");
}

/// DELETE 账户面（data-privacy §4）：档案与同步记录整体清除，幂等；
/// 删后 GET 复建的是缺省档案，改名不复活
#[tokio::test]
async fn account_delete_is_idempotent_and_resets_profile() {
    let base = spawn_account_app(account_test_state()).await;
    let client = reqwest::Client::new();
    let profile_url = format!("{base}/api/v1/account/profile");
    let sync_url = format!("{base}/api/v1/account/sync");
    let delete_url = format!("{base}/api/v1/account");

    // 建档案 + 改名 + 留一条同步记录，让删除前后可分辨
    let created: serde_json::Value = client
        .get(&profile_url)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(created["rev"], 1);
    let patched: serde_json::Value = client
        .put(&profile_url)
        .header("content-type", "application/json")
        .body(r#"{"display_name":"待删用户"}"#)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(patched["rev"], 2);
    let pushed = post_json(
            &client,
            &sync_url,
            r#"{"cursor":0,"base":{},"pushes":[{"key":"workspace:a","base_rev":0,"payload":{"name":"盯盘"}}]}"#,
        )
        .await;
    assert_eq!(
        pushed["cursor"], 2,
        "档案改名占一个增量位 + 推送占一个（update_profile 触碰水位）"
    );

    let deleted = client.delete(&delete_url).send().await.unwrap();
    assert_eq!(deleted.status().as_u16(), 204, "删除成功无响应体");

    // 复建的档案 = 缺省（rev 归 1、展示名回「本机用户」），同步水位归零
    let fresh: serde_json::Value = client
        .get(&profile_url)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(fresh["rev"], 1, "删除后档案应重建为缺省，不复用旧 rev");
    assert_eq!(fresh["display_name"], "本机用户", "改名不复活");
    let cleared = post_json(&client, &sync_url, r#"{"cursor":0,"base":{},"pushes":[]}"#).await;
    assert!(
        cleared["changes"].as_array().unwrap().is_empty(),
        "同步记录应一并清除: {cleared}"
    );
    assert_eq!(cleared["cursor"], 0, "同步水位随删除归零");

    // 幂等：重复删仍 204（无数据可删不是错误）
    let again = client.delete(&delete_url).send().await.unwrap();
    assert_eq!(again.status().as_u16(), 204);
}

/// 删除清的是持久层快照：写穿快照删净后，换一份空内存存储（等价重启）
/// 懒加载到的是缺省档案而非旧快照——绝不出现「声称已删而快照还在」
#[tokio::test]
async fn account_delete_clears_persisted_snapshot() {
    let backend = Arc::new(alpha_storage::MemoryStorage::new());
    let store = account::AccountStore::with_persistence(backend.clone());
    store.profile("alice", 100).await;
    store
        .update_profile(
            "alice",
            &account::ProfilePatch {
                display_name: Some("改名".into()),
                email: None,
                locale: None,
            },
            150,
        )
        .await
        .unwrap();

    let removed = store.delete("alice").await.unwrap();
    assert!(removed, "有数据可删应返回 true");
    let keys = backend.list_keys("alpha:account:").await.unwrap();
    assert!(keys.is_empty(), "持久层快照应删净: {keys:?}");

    // 空内存 + 同一后端 = 等价重启：懒加载不复活旧快照
    let fresh = account::AccountStore::with_persistence(backend);
    let profile = fresh.profile("alice", 300).await;
    assert_eq!(profile.rev, 1, "删除后懒加载应重建缺省档案而非旧快照");
    assert_eq!(profile.display_name, "alice", "改名不复活");

    // 幂等：无数据可删返回 false，不是错误
    assert!(!store.delete("alice").await.unwrap());
}

/// 写失败后端（账户写穿 fail-open 测试桩；`store` 恒失败）
#[derive(Default)]
struct FailingWriteBackend;

#[async_trait::async_trait]
impl alpha_storage::StorageBackend for FailingWriteBackend {
    async fn store(&self, _key: &str, _value: Vec<u8>) -> alpha_core::errors::AlphaResult<()> {
        Err(alpha_core::errors::AlphaError::StorageError(
            "injected write failure".into(),
        ))
    }
    async fn retrieve(&self, _key: &str) -> alpha_core::errors::AlphaResult<Option<Vec<u8>>> {
        Ok(None)
    }
    async fn delete(&self, _key: &str) -> alpha_core::errors::AlphaResult<bool> {
        Ok(false)
    }
    async fn exists(&self, _key: &str) -> alpha_core::errors::AlphaResult<bool> {
        Ok(false)
    }
    async fn list_keys(&self, _prefix: &str) -> alpha_core::errors::AlphaResult<Vec<String>> {
        Ok(Vec::new())
    }
    async fn clear(&self) -> alpha_core::errors::AlphaResult<()> {
        Ok(())
    }
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

/// jwt 模式装配：上游 = 第二参（realtime 侧可回声），data-engine 侧死端口
fn jwt_test_state(data_engine_url: String, realtime_url: String) -> GatewayState {
    let mut state = test_state(data_engine_url, realtime_url);
    state.auth = auth::AuthConfig {
        mode: auth::AuthMode::JwtRequired,
        secret: "test-secret".to_string(),
        expected_issuer: String::new(),
        expected_audience: String::new(),
        provision_key: String::new(),
    };
    state
}

async fn spawn_on(addr_app: axum::Router) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, addr_app).await.unwrap();
    });
    addr
}

/// WS 握手鉴权（auth.md §6 遗留项闭合）：jwt 模式下无票升级 → 401，
/// 且未拨上游（realtime_url 死端口不产生 502——401 优先于上游连接）
#[tokio::test]
async fn ws_proxy_rejects_unauthenticated_in_jwt_mode() {
    let app = build_router(jwt_test_state(
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    ));
    let addr = spawn_on(app).await;

    let err = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
        .await
        .unwrap_err();
    match err {
        tokio_tungstenite::tungstenite::Error::Http(resp) => {
            assert_eq!(resp.status(), 401, "无票 WS 升级应 401");
        }
        other => panic!("expected HTTP 401 handshake failure, got {other:?}"),
    }
}

/// 错 secret 签的票 → 401（与 REST 面同口径：不区分原因）
#[tokio::test]
async fn ws_proxy_rejects_wrong_secret_token_in_jwt_mode() {
    let app = build_router(jwt_test_state(
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    ));
    let addr = spawn_on(app).await;

    let bad = auth::create_token("wrong-secret", "mallory", "", Duration::from_secs(60)).unwrap();
    let err = tokio_tungstenite::connect_async(format!("ws://{addr}/ws?token={bad}"))
        .await
        .unwrap_err();
    match err {
        tokio_tungstenite::tungstenite::Error::Http(resp) => {
            assert_eq!(resp.status(), 401);
        }
        other => panic!("expected HTTP 401 handshake failure, got {other:?}"),
    }
}

/// 合法票走 `?token=` 查询参数（浏览器侧无自定义握手头时的标准回落）
/// → 握手放行且反代链路完好（回声往返）
#[tokio::test]
async fn ws_proxy_accepts_valid_token_query_param_in_jwt_mode() {
    let upstream = spawn_upstream().await;
    let app = build_router(jwt_test_state("http://127.0.0.1:1".to_string(), upstream));
    let addr = spawn_on(app).await;

    let token = auth::create_token("test-secret", "carol", "", Duration::from_secs(60)).unwrap();
    let (mut ws_stream, _) =
        tokio_tungstenite::connect_async(format!("ws://{addr}/ws?token={token}"))
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

/// 合法票走 `Authorization: Bearer` 头（服务端/代理侧客户端可设头）
/// → 握手放行
#[tokio::test]
async fn ws_proxy_accepts_bearer_header_in_jwt_mode() {
    let upstream = spawn_upstream().await;
    let app = build_router(jwt_test_state("http://127.0.0.1:1".to_string(), upstream));
    let addr = spawn_on(app).await;

    let token = auth::create_token("test-secret", "dave", "", Duration::from_secs(60)).unwrap();
    let mut req: tokio_tungstenite::tungstenite::http::Request<()> =
        tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(format!(
            "ws://{addr}/ws"
        ))
        .unwrap();
    req.headers_mut().insert(
        axum::http::header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    );
    let (_ws_stream, resp) = tokio_tungstenite::connect_async(req).await.unwrap();
    assert_eq!(resp.status(), 101, "持票升级应 101 Switching Protocols");
}

/// token 提取纯函数：Bearer 头优先于查询参数；两者皆无 → None；
/// 空 `token=` 不算票
#[test]
fn ws_auth_token_prefers_header_and_requires_presence() {
    let mut headers = HeaderMap::new();
    let uri: Uri = "/ws?token=query_token".parse().unwrap();
    assert_eq!(
        ws_auth_token(&headers, &uri).as_deref(),
        Some("query_token"),
        "无头回落查询参数"
    );

    let bare: Uri = "/ws".parse().unwrap();
    assert_eq!(ws_auth_token(&headers, &bare), None, "无头无票 → None");

    headers.insert(
        axum::http::header::AUTHORIZATION,
        HeaderValue::from_static("Bearer header_token"),
    );
    assert_eq!(
        ws_auth_token(&headers, &uri).as_deref(),
        Some("header_token"),
        "头优先于查询参数"
    );

    let no_query: Uri = "/ws".parse().unwrap();
    assert_eq!(
        ws_auth_token(&headers, &no_query).as_deref(),
        Some("header_token")
    );

    let mut empty_auth = HeaderMap::new();
    empty_auth.insert(
        axum::http::header::AUTHORIZATION,
        HeaderValue::from_static("Bearer"),
    );
    let empty_param: Uri = "/ws?token=".parse().unwrap();
    assert_eq!(
        ws_auth_token(&empty_auth, &empty_param),
        None,
        "空头回落空参数 → None"
    );
}

// ---- OIDC JWKS 接线（L502）----

/// oct JWKS 文档：k = base64url(密钥字节)（RFC 7517）
fn jwks_doc(entries: &[(&str, &str)]) -> String {
    use base64::Engine as _;
    let keys: Vec<String> = entries
        .iter()
        .map(|(kid, secret)| {
            let k = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(secret);
            format!(r#"{{"kid":"{kid}","kty":"oct","k":"{k}"}}"#)
        })
        .collect();
    format!(r#"{{"keys":[{}]}}"#, keys.join(","))
}

/// 本地 IdP JWKS 端点：flip=false 服务 key-1，置 true 服务 key-2
/// （模拟 IdP 轮换换表）
async fn spawn_jwks_server() -> (std::net::SocketAddr, Arc<std::sync::atomic::AtomicBool>) {
    let flip = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flip_state = flip.clone();
    let app = Router::new().route(
        "/jwks",
        test_get(move || async move {
            if flip_state.load(std::sync::atomic::Ordering::Relaxed) {
                jwks_doc(&[("key-2", "idp-secret-2")])
            } else {
                jwks_doc(&[("key-1", "idp-secret-1")])
            }
        }),
    );
    let addr = spawn_on(app).await;
    (addr, flip)
}

/// 铸 IdP 票：指定 kid 与签名密钥（模拟外部 IdP 签发面）
fn mint_idp_token(kid: &str, secret: &[u8], roles: &[String]) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    jwt::encode(
        &jwt::Header {
            kid: Some(kid.to_string()),
            ..jwt::Header::new(jwt::Algorithm::HS256)
        },
        &auth::Claims {
            sub: "carol".to_string(),
            scope: String::new(),
            roles: roles.to_vec(),
            iat: now,
            exp: now + 300,
        },
        &jwt::EncodingKey::from_secret(secret),
    )
    .unwrap()
}

/// L502：JWKS 接线 /api 面——IdP 票（kid）走键表核 200；自签票（无 kid）
/// 走自签核 200（bootstrap 与 IdP 并存）；unknown kid → 401 不回退自签
#[tokio::test]
async fn oidc_jwks_serves_idp_tokens_on_api_face() {
    let (jwks_addr, _flip) = spawn_jwks_server().await;
    let client = reqwest::Client::new();
    let store = init_jwks_store(&client, &format!("http://{jwks_addr}/jwks"), true)
        .await
        .unwrap()
        .expect("jwt 模式 + 端点必须建表");
    let upstream = spawn_upstream().await;
    let mut state = test_state(upstream, "http://127.0.0.1:1".to_string());
    state.auth = auth::AuthConfig {
        mode: auth::AuthMode::JwtRequired,
        secret: "test-secret".to_string(),
        expected_issuer: String::new(),
        expected_audience: String::new(),
        provision_key: String::new(),
    };
    state.jwks = Some(store);
    let addr = spawn_on(build_router(state)).await;

    let http = reqwest::Client::new();
    let url = format!("http://{addr}/api/v1/stocks/600519/history");

    let idp_ok = http
        .post(&url)
        .bearer_auth(mint_idp_token(
            "key-1",
            b"idp-secret-1",
            &[auth::ROLE_OPERATOR.to_string()],
        ))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(idp_ok.status().as_u16(), 200, "IdP 票（kid）走键表核");

    let self_ok = http
        .post(&url)
        .bearer_auth(
            auth::create_token_with_roles(
                "test-secret",
                "bootstrap",
                "",
                &[auth::ROLE_OPERATOR.to_string()],
                Duration::from_secs(60),
            )
            .unwrap(),
        )
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(
        self_ok.status().as_u16(),
        200,
        "自签票（无 kid）走自签核，两者并存"
    );

    let unknown = http
        .post(&url)
        .bearer_auth(mint_idp_token(
            "key-9",
            b"other-idp",
            &[auth::ROLE_OPERATOR.to_string()],
        ))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(
        unknown.status().as_u16(),
        401,
        "unknown kid 拒绝且不回退自签"
    );
}

/// L502：init 门禁——off 模式配端点忽略；jwt + 不可达端点 fail-fast；
/// jwt + 无 oct 键 fail-fast（空表起来等于认证面全拒，属配置错误）
#[tokio::test]
async fn init_jwks_store_fail_fast_gates() {
    let client = reqwest::Client::new();

    // off 模式：配置了端点也忽略（认证关闭零行为变化）
    let (jwks_addr, _flip) = spawn_jwks_server().await;
    let off = init_jwks_store(&client, &format!("http://{jwks_addr}/jwks"), false)
        .await
        .unwrap();
    assert!(off.is_none(), "off 模式忽略 JWKS 端点");

    // jwt 模式 + 不可达端点 → Err（初始拉取 fail-fast）
    assert!(init_jwks_store(&client, "http://127.0.0.1:1/jwks", true)
        .await
        .is_err());

    // jwt 模式 + 无 oct 键文档 → Err
    let empty_app = Router::new().route("/jwks", test_get(|| async { r#"{"keys":[]}"# }));
    let empty_addr = spawn_on(empty_app).await;
    let empty = init_jwks_store(&client, &format!("http://{empty_addr}/jwks"), true).await;
    assert!(empty.is_err(), "空键表必须拒绝启动");
}

/// L502：刷新换表——IdP 轮换后 refresh_jwks_once 换新表（旧 kid 401、
/// 新 kid 200）；空文档/失败保旧表（verify 语义不中断）
#[tokio::test]
async fn jwks_refresh_swaps_keys_and_keeps_stale_on_failure() {
    let (jwks_addr, flip) = spawn_jwks_server().await;
    let url = format!("http://{jwks_addr}/jwks");
    let client = reqwest::Client::new();
    let store = init_jwks_store(&client, &url, true).await.unwrap().unwrap();

    // IdP 轮换：服务端换表
    flip.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(refresh_jwks_once(&client, &url, &store).await, "换表成功");

    let jwks_read = store.read().await;
    assert!(
        jwks_read.contains_key("key-2") && !jwks_read.contains_key("key-1"),
        "键表应换到 key-2"
    );
    drop(jwks_read);

    // 端点坏掉 → 刷新失败保旧表（key-2 仍在，验签不中断）
    let dead = refresh_jwks_once(&client, "http://127.0.0.1:1/jwks", &store).await;
    assert!(!dead, "失败保旧表");
    assert!(store.read().await.contains_key("key-2"), "旧表未被清空");
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
