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

mod account;
mod audit;
mod auth;
mod shield;
use std::net::SocketAddr;
use std::sync::Arc;
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

    /// 认证模式：off（默认，直通）| jwt（/api 强制 Bearer 校验；env ALPHA_GATEWAY_AUTH_MODE 优先）
    #[arg(long, default_value = "off")]
    auth_mode: String,

    /// JWT HS256 共享 secret（jwt 模式必填；env ALPHA_GATEWAY_AUTH_SECRET 优先，空=拒绝启动）
    #[arg(long, default_value = "")]
    auth_secret: String,

    /// OIDC 期望签发者（空=跳过 iss 校验；env ALPHA_GATEWAY_AUTH_ISSUER 优先）
    #[arg(long, default_value = "")]
    auth_issuer: String,

    /// OIDC 期望受众（空=跳过 aud 校验；env ALPHA_GATEWAY_AUTH_AUDIENCE 优先）
    #[arg(long, default_value = "")]
    auth_audience: String,

    /// /auth/token bootstrap 签发口令（空=关闭该端点；env ALPHA_GATEWAY_AUTH_PROVISION_KEY 优先）
    #[arg(long, default_value = "")]
    auth_provision_key: String,

    /// 账户/同步持久化后端 URL（L476：留空 = 纯内存，单机形态零依赖；
    /// 配 Postgres 则账户状态写穿该实例，启动时连不上直接退出）
    #[arg(long, default_value = "")]
    account_store_url: String,

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
    /// 认证配置（Off = 零行为变化直通）
    auth: auth::AuthConfig,
    /// 防爬虫/DDoS 护栏（L486：UA 分类 + 路径扫描检测 + 秒级 burst）
    shield: shield::ShieldState,
    /// 安全审计与失败风暴检测（L487）
    audit: audit::AuditState,
    /// 统一账户与跨端数据同步存储（L476：内存权威 + 可选 Postgres 写穿）
    account_store: account::AccountStore,
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
        shield: shield::ShieldState::from_env(),
        audit: audit::AuditState::from_env(),
        account_store: init_account_store(&resolve_url(
            "ALPHA_GATEWAY_ACCOUNT_STORE_URL",
            &args.account_store_url,
        ))
        .await?,
        metrics,
        auth: {
            let mode =
                auth::AuthMode::parse(&resolve_url("ALPHA_GATEWAY_AUTH_MODE", &args.auth_mode));
            let secret = resolve_url("ALPHA_GATEWAY_AUTH_SECRET", &args.auth_secret);
            if mode == auth::AuthMode::JwtRequired && secret.is_empty() {
                anyhow::bail!("auth mode is jwt but no secret configured (set --auth-secret or ALPHA_GATEWAY_AUTH_SECRET)");
            }
            let mut cfg = auth::AuthConfig::disabled();
            cfg.mode = mode;
            cfg.secret = secret;
            cfg.expected_issuer = resolve_url("ALPHA_GATEWAY_AUTH_ISSUER", &args.auth_issuer);
            cfg.expected_audience = resolve_url("ALPHA_GATEWAY_AUTH_AUDIENCE", &args.auth_audience);
            cfg.provision_key =
                resolve_url("ALPHA_GATEWAY_AUTH_PROVISION_KEY", &args.auth_provision_key);
            cfg
        },
    };

    let app = build_router(state);

    // 启动服务器
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    tracing::info!("API Gateway listening on {}", args.bind);

    axum::serve(listener, app).await?;

    Ok(())
}

/// 账户存储装配（L476）：URL 为空 = 内存形态（默认，与历史启动行为完全
/// 一致）；非空则连 Postgres KV 表写穿账户快照——**连接失败即退出**
/// （静默降级到内存会让多副本部署各持一份互相看不见的账户数据，
/// 比启动失败更难排查）。
async fn init_account_store(url: &str) -> anyhow::Result<account::AccountStore> {
    if url.is_empty() {
        tracing::info!("account sync store: in-memory (no --account-store-url)");
        return Ok(account::AccountStore::in_memory());
    }
    let backend = alpha_storage::PostgresKvStorage::connect(url, "alpha_accounts", None, None)
        .await
        .map_err(|e| anyhow::anyhow!("account store connect failed: {e}"))?;
    tracing::info!("account sync store: postgres-backed");
    Ok(account::AccountStore::with_persistence(Arc::new(backend)))
}

fn build_router(state: GatewayState) -> Router {
    // /api 子路由：认证 + 护栏 + 限流中间件只包 REST 反代面（健康检查、
    // 指标、WS 与 /auth/token 不占配额不鉴权）。layer 注册顺序注意：后注册
    // 先执行——auth → shield → rate_limit：未鉴权请求不消耗护栏/限流预算，
    // 护栏（bot/burst/扫描）挡下的流量不再消耗 Redis 限流配额。
    let api = Router::new()
        .route("/v1/*path", any(api_proxy))
        // 账户与跨端同步（L476）：走 /api 前缀与上游反代同级，故与反代面
        // 共用同一道 auth → shield → rate_limit 链（认证后才知账户 id）
        .route("/v1/account/profile", get(get_account_profile).put(put_account_profile))
        .route("/v1/account", axum::routing::delete(delete_account))
        .route("/v1/account/sync", axum::routing::post(post_account_sync))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            shield_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    Router::new()
        // 健康检查：真实探测三个上游
        .route("/health", get(health_check))
        // Prometheus 抓取端点（scrape 配置见 config/prometheus.yml）
        .route("/metrics", get(metrics_endpoint))
        // bootstrap 签发端点（provision_key 为空时 503 关闭，见函数注释）
        .route("/auth/token", axum::routing::post(provision_token))
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

/// 当前毫秒时间戳（审计/护栏判定面的时钟入参；纯逻辑保持可测）
fn system_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 护栏拒绝响应（与限流 429 同风格 JSON 错误体）
fn shield_rejection(status: StatusCode, error: &str) -> Response {
    (
        status,
        Json(serde_json::json!({
            "success": false,
            "error": error,
        })),
    )
        .into_response()
}

/// /api 防爬虫与 burst 护栏中间件（L486）：bot_deny 开启时拒绝空/已知
/// 脚本 UA（403）；burst 令牌桶耗尽 429；同身份窗口内离散路径扫描超阈
/// 403（后两者默认开启、宽阈值）。时钟只在 middleware 读（毫秒），
/// 判定逻辑全部在 shield 纯函数面（可测、可回放）。
async fn shield_middleware(
    State(state): State<GatewayState>,
    req: axum::extract::Request,
    next: middleware::Next,
) -> Response {
    let shield = &state.shield;
    let now_ms = system_now_ms();
    let identity = client_identity(req.headers());

    if shield.config.bot_deny {
        let class = shield::classify_user_agent(
            req.headers()
                .get(header::USER_AGENT)
                .and_then(|value| value.to_str().ok()),
        );
        if class != shield::UaClass::Normal {
            metrics::counter!("alpha_gateway_shield_total", "mode" => "bot_denied").increment(1);
            audit::emit(&audit::AuditEvent::AccessDenied {
                identity: identity.clone(),
                path: req.uri().path().to_string(),
                reason: "bot".to_string(),
            });
            return shield_rejection(StatusCode::FORBIDDEN, "blocked client signature");
        }
    }

    if shield.config.burst_enabled {
        let allowed = shield.burst.lock().unwrap().try_acquire(&identity, now_ms);
        if !allowed {
            metrics::counter!("alpha_gateway_shield_total", "mode" => "burst_denied").increment(1);
            audit::emit(&audit::AuditEvent::AccessDenied {
                identity: identity.clone(),
                path: req.uri().path().to_string(),
                reason: "burst".to_string(),
            });
            return shield_rejection(StatusCode::TOO_MANY_REQUESTS, "burst limit exceeded");
        }
    }

    if shield.config.scan_enabled {
        let path = req.uri().path().to_string();
        let suspicious = shield
            .scan
            .lock()
            .unwrap()
            .observe(&identity, &path, now_ms);
        if suspicious {
            metrics::counter!("alpha_gateway_shield_total", "mode" => "scan_denied").increment(1);
            audit::emit(&audit::AuditEvent::AccessDenied {
                identity: identity.clone(),
                path: req.uri().path().to_string(),
                reason: "scan".to_string(),
            });
            return shield_rejection(StatusCode::FORBIDDEN, "scanning behavior detected");
        }
    }

    next.run(req).await
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
        audit::emit(&audit::AuditEvent::AccessDenied {
            identity: subject.clone(),
            path: req.uri().path().to_string(),
            reason: "rate_limit".to_string(),
        });
        tracing::warn!(%subject, "rate limit exceeded");
        rate_limit_rejection(&decision)
    }
}

/// /api 认证中间件（L483）：Off 直接放行；JwtRequired 强制 Bearer 校验。
/// 本增量走自签路径（HS256 + 启动期非空 secret）；`verify_oidc_token`
///（auth.rs，单测覆盖）是 OIDC IdP 路径的校验核——JWKS 拉取接线
///（--auth-jwks-url 定时刷新）归下一增量，本单不引入后台刷新任务。
/// 认证失败一律 fail-closed（与限流 fail-open 方向相反：限流器坏了可以放，
/// 认不出来是谁绝不能放）。401 不区分“缺头/坏签/过期”，防用户枚举。
async fn auth_middleware(
    State(state): State<GatewayState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if state.auth.mode == auth::AuthMode::Off {
        // 账户面（L476）需要知道「当前是谁」：认证关闭时显式插入 None，
        // 下游按本机缺省账户处理（不用 Extension 缺省值——Optional
        // FromRequestParts 在无扩展时报 500 而非回落）
        let mut req = req;
        req.extensions_mut().insert(None::<auth::Claims>);
        return next.run(req).await;
    }

    let Some(token) = auth::extract_bearer(req.headers()) else {
        // 缺失 token 是常规未认证流量，不进审计面（量纲归护栏/限流）
        return unauthorized();
    };
    match auth::verify_token(&state.auth.secret, &token) {
        Ok(claims) => {
            // L484：认证之后做 RBAC（401 管“你是谁”，403 管“你能干什么”）
            let method = req.method().to_string();
            let path = req.uri().path().to_string();
            if !auth::authorize(&claims, &method, &path) {
                metrics::counter!("alpha_gateway_auth_total", "mode" => "forbidden").increment(1);
                audit::emit(&audit::AuditEvent::AccessDenied {
                    identity: claims.sub.clone(),
                    path,
                    reason: "rbac".to_string(),
                });
                return forbidden();
            }
            metrics::counter!("alpha_gateway_auth_total", "mode" => "allowed").increment(1);
            tracing::debug!(sub = %claims.sub, "authenticated");
            let mut req = req;
            // 账户面按 sub 分区（L476）：校验通过的 Claims 下传，
            // 处理器据此取账户档案/同步记录，不再二次验签。插入类型必须
            // 与处理器提取的 `Extension<Option<Claims>>` 逐字一致——
            // 插 `Claims` 会让提取器找不到扩展、返回空体 500
            req.extensions_mut().insert(Some::<auth::Claims>(claims));
            next.run(req).await
        }
        Err(_) => {
            metrics::counter!("alpha_gateway_auth_total", "mode" => "denied").increment(1);
            state.audit.observe_auth_failure(
                &client_identity(req.headers()),
                "invalid_token",
                system_now_ms(),
            );
            unauthorized()
        }
    }
}

/// 请求所属账户 id（L476）：认证关闭 = 本机缺省账户（单账户本地形态）；
/// 开启认证时取 auth 中间件校验过的 `Claims`（存于请求扩展，避免处理器
/// 里二次验签——票据已在本链路校验过，再验一次只会多一处失败分支）。
fn request_account_id(claims: Option<&auth::Claims>) -> String {
    claims
        .map(|c| c.sub.clone())
        .unwrap_or_else(|| alpha_core::account::LOCAL_ACCOUNT_ID.to_string())
}

/// `GET /api/v1/account/profile`：当前账户档案（不存在则按 sub 建初始档案）
async fn get_account_profile(
    State(state): State<GatewayState>,
    axum::Extension(claims): axum::Extension<Option<auth::Claims>>,
) -> Response {
    let account_id = request_account_id(claims.as_ref());
    let profile = state
        .account_store
        .profile(&account_id, system_now_ms())
        .await;
    (
        StatusCode::OK,
        Json(account::ProfileResponse {
            account_id: profile.account_id,
            display_name: profile.display_name,
            email: profile.email,
            locale: profile.locale,
            rev: profile.rev,
        }),
    )
        .into_response()
}

/// `PUT /api/v1/account/profile`：字段级更新（缺省字段不改，空邮箱串 = 清除）
async fn put_account_profile(
    State(state): State<GatewayState>,
    axum::Extension(claims): axum::Extension<Option<auth::Claims>>,
    Json(patch): Json<account::ProfilePatch>,
) -> Response {
    let account_id = request_account_id(claims.as_ref());
    match state
        .account_store
        .update_profile(&account_id, &patch, system_now_ms())
        .await
    {
        Ok(profile) => (
            StatusCode::OK,
            Json(account::ProfileResponse {
                account_id: profile.account_id,
                display_name: profile.display_name,
                email: profile.email,
                locale: profile.locale,
                rev: profile.rev,
            }),
        )
            .into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": err,
            })),
        )
            .into_response(),
    }
}

/// `DELETE /api/v1/account`：删除当前账户的全部服务端数据（档案 + 同步
/// 记录），data-privacy §4（GDPR Art.17 / CCPA 删除权）的服务端落实面。
/// 幂等：重复删/无数据都 204；后端删除失败 500（不假称已删）。
async fn delete_account(
    State(state): State<GatewayState>,
    axum::Extension(claims): axum::Extension<Option<auth::Claims>>,
) -> Response {
    let account_id = request_account_id(claims.as_ref());
    match state.account_store.delete(&account_id).await {
        Ok(_) => {
            audit::emit(&audit::AuditEvent::AccountDataDeleted {
                account_id: account_id.clone(),
            });
            StatusCode::NO_CONTENT.into_response()
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": err,
            })),
        )
            .into_response(),
    }
}

/// `POST /api/v1/account/sync`：同步往返（接受推送 + 回传增量 + 新水位）
async fn post_account_sync(
    State(state): State<GatewayState>,
    axum::Extension(claims): axum::Extension<Option<auth::Claims>>,
    Json(request): Json<alpha_core::account::SyncRequest>,
) -> Response {
    let account_id = request_account_id(claims.as_ref());
    let response = state
        .account_store
        .sync(&account_id, &request, system_now_ms())
        .await;
    (StatusCode::OK, Json(response)).into_response()
}

/// bootstrap 签发端点（L483）：`POST /auth/token` + `X-Provision-Key` 头。
/// 生产形态是 OIDC IdP 的授权码/设备流，本端点只是“无 IdP 环境下能跑通
/// 签发→校验闭环”的最小签发器：provision_key 为空 → 503 关闭（默认关闭，
/// 不扩大攻击面）；scope 默认空；ttl 上限 24h（防一次性签出长期票据）。
async fn provision_token(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    if state.auth.provision_key.is_empty() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "success": false,
                "error": "token provisioning disabled",
            })),
        )
            .into_response();
    }
    let ok = headers
        .get("x-provision-key")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|k| k == state.auth.provision_key);
    if !ok {
        let identity = client_identity(&headers);
        audit::emit(&audit::AuditEvent::TokenProvisionDenied {
            identity: identity.clone(),
        });
        state
            .audit
            .note_failure_for_storm(&identity, system_now_ms());
        return unauthorized();
    }
    let sub = body
        .get("sub")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if sub.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": "sub is required",
            })),
        )
            .into_response();
    }
    let ttl_secs = body
        .get("ttl_secs")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(3600)
        .min(86_400);
    let scope = body
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    // L484：roles 原样写入票据（provision_key 保管等级须高于所授最高角色）
    let roles: Vec<String> = body
        .get("roles")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    // 空 roles 走无角色签发（viewer 只读语义）；非空原样写入
    let minted = if roles.is_empty() {
        auth::create_token(
            &state.auth.secret,
            sub,
            scope,
            Duration::from_secs(ttl_secs),
        )
    } else {
        auth::create_token_with_roles(
            &state.auth.secret,
            sub,
            scope,
            &roles,
            Duration::from_secs(ttl_secs),
        )
    };
    match minted {
        Ok(token) => {
            audit::emit(&audit::AuditEvent::TokenProvisioned {
                sub: sub.to_string(),
            });
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "access_token": token,
                    "token_type": "Bearer",
                    "expires_in": ttl_secs,
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": e.to_string(),
            })),
        )
            .into_response(),
    }
}

/// 401 响应：WWW-Authenticate 头 + 最小 JSON 错误体（不泄露过期/伪造区分）
fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Bearer realm=\"alpha-api\""),
        )],
        Json(serde_json::json!({
            "success": false,
            "error": "unauthorized",
        })),
    )
        .into_response()
}
/// 403 响应：已认证但角色不足（与 401 区分：401=重登/换票，403=找管理员加角色）
fn forbidden() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({
            "success": false,
            "error": "forbidden: insufficient role",
        })),
    )
        .into_response()
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

        let created: serde_json::Value =
            client.get(&url).send().await.unwrap().json().await.unwrap();
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
        let viewer =
            auth::create_token("test-secret", "carol", "", Duration::from_secs(60)).unwrap();
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
