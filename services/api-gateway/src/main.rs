//! Alpha Finance API Gateway
//!
//! 统一的 API 入口点：REST 反代 data-engine、WebSocket 反代 real-time-feed、
//! 聚合健康检查（真实探测上游 `/health`，不再返回硬编码假状态）。
//!
//! 上游地址可经 CLI 参数或 `ALPHA_GATEWAY_*` 环境变量配置（env 优先，便于部署
//! 覆写，compose 已按容器网络注入）。

use axum::{
    routing::{any, get},
    Router,
};
use clap::Parser;

use alpha_storage::RedisRateLimiter;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

mod account;
mod audit;
mod auth;
mod jwt;
mod middleware;
mod proxy;
mod routes;
mod shield;
#[cfg(test)]
mod tests;
use middleware::{auth_middleware, rate_limit_middleware, request_logger, shield_middleware};
use proxy::{api_proxy, ws_proxy, ws_proxy_root};
use routes::{
    delete_account, get_account_profile, health_check, metrics_endpoint, post_account_sync,
    provision_token, put_account_profile,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
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

    /// OIDC JWKS 端点（jwt 模式下启用：票带 kid 走 IdP 键表校验，
    /// 无 kid 票仍走 --auth-secret 自签；URL 直接指向 JWKS 文档。
    /// env ALPHA_GATEWAY_AUTH_JWKS_URL 优先；空=关闭，默认自签路径）
    #[arg(long, default_value = "")]
    auth_jwks_url: String,

    /// JWKS 定时刷新间隔秒（0 = 仅启动取一次；env ALPHA_GATEWAY_JWKS_REFRESH_SECS 优先；
    /// 运行期刷新失败保旧键继续服务，认证面 fail-closed 语义不变）
    #[arg(long, default_value = "600")]
    auth_jwks_refresh_secs: u64,

    /// TLS 证书 PEM 路径（与 --tls-key 同给启用服务内 TLS；
    /// env ALPHA_GATEWAY_TLS_CERT 优先；缺省 TLS 终结归反代）
    #[arg(long, default_value = "")]
    tls_cert: String,

    /// TLS 私钥 PEM 路径（env ALPHA_GATEWAY_TLS_KEY 优先）
    #[arg(long, default_value = "")]
    tls_key: String,

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

/// OIDC JWKS 键表（L502）：kid → 校验键。启动拉取 + 后台定时刷新，
/// 刷新失败保旧表（fail-closed：无键票据照旧 401，不因刷新抖动放行）
pub type JwksStore = Arc<tokio::sync::RwLock<HashMap<String, jwt::DecodingKey>>>;

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
    /// OIDC 键表（None = 未配 --auth-jwks-url，全部走自签核）
    jwks: Option<JwksStore>,
    /// 防爬虫/DDoS 护栏（L486：UA 分类 + 路径扫描检测 + 秒级 burst）
    shield: shield::ShieldState,
    /// 安全审计与失败风暴检测（L487）
    audit: audit::AuditState,
    /// 统一账户与跨端数据同步存储（L476：内存权威 + 可选 Postgres 写穿）
    account_store: account::AccountStore,
    /// Prometheus 指标渲染句柄（/metrics）
    metrics: PrometheusHandle,
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

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(PROXY_TIMEOUT_SECS))
        .build()?;

    // 认证配置前置解析 + OIDC JWKS 键表（L502）：jwt 模式配了
    // --auth-jwks-url 即启用——初始拉取失败/空键表直接拒绝启动（空表
    // 会让所有 kid 票 401，静默起来等于认证面全拒，属配置错误不是运行态）；
    // 运行期刷新失败保旧键（见 refresh_jwks_once）。
    let auth_mode = auth::AuthMode::parse(&resolve_url("ALPHA_GATEWAY_AUTH_MODE", &args.auth_mode));
    let auth_secret = resolve_url("ALPHA_GATEWAY_AUTH_SECRET", &args.auth_secret);
    if auth_mode == auth::AuthMode::JwtRequired && auth_secret.is_empty() {
        anyhow::bail!("auth mode is jwt but no secret configured (set --auth-secret or ALPHA_GATEWAY_AUTH_SECRET)");
    }
    let jwks_url = resolve_url("ALPHA_GATEWAY_AUTH_JWKS_URL", &args.auth_jwks_url);
    let jwks = init_jwks_store(
        &http_client,
        &jwks_url,
        auth_mode == auth::AuthMode::JwtRequired,
    )
    .await?;

    // 后台定时刷新（L502）：--auth-jwks-refresh-secs 间隔换表（0 = 仅启动
    // 取一次不刷新）；首个 tick 立即返回故跳过（启动时已拉取）
    if let Some(store) = jwks.clone() {
        let refresh_secs = resolve_url(
            "ALPHA_GATEWAY_JWKS_REFRESH_SECS",
            &args.auth_jwks_refresh_secs.to_string(),
        )
        .parse::<u64>()
        .unwrap_or(600);
        if refresh_secs > 0 {
            let client = http_client.clone();
            let url = jwks_url.clone();
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_secs(refresh_secs));
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                ticker.tick().await;
                loop {
                    ticker.tick().await;
                    refresh_jwks_once(&client, &url, &store).await;
                }
            });
        }
    }

    let state = GatewayState {
        client: http_client,
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
            let mut cfg = auth::AuthConfig::disabled();
            cfg.mode = auth_mode;
            cfg.secret = auth_secret;
            cfg.expected_issuer = resolve_url("ALPHA_GATEWAY_AUTH_ISSUER", &args.auth_issuer);
            cfg.expected_audience = resolve_url("ALPHA_GATEWAY_AUTH_AUDIENCE", &args.auth_audience);
            cfg.provision_key =
                resolve_url("ALPHA_GATEWAY_AUTH_PROVISION_KEY", &args.auth_provision_key);
            cfg
        },
        jwks,
    };

    let app = build_router(state);

    // 启动服务器：TLS 两把钥匙齐备走服务内 rustls（裸机无反代直出 HTTPS
    // 用）；反代终结仍为默认形态。只配其一是部署配置错误 → 拒绝启动
    // （静默降级明文会把「以为加密了」变成最贵的坑）。
    match resolve_tls(
        &resolve_url("ALPHA_GATEWAY_TLS_CERT", &args.tls_cert),
        &resolve_url("ALPHA_GATEWAY_TLS_KEY", &args.tls_key),
    )? {
        None => {
            let listener = tokio::net::TcpListener::bind(args.bind).await?;
            tracing::info!(
                "API Gateway listening on {} (http, TLS 终结归反代)",
                args.bind
            );
            axum::serve(listener, app).await?;
        }
        Some((cert, key)) => {
            let config = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key)
                .await
                .map_err(|e| anyhow::anyhow!("TLS cert/key load failed: {e}"))?;
            tracing::info!("API Gateway listening on {} (https, 服务内 TLS)", args.bind);
            axum_server::bind_rustls(args.bind, config)
                .serve(app.into_make_service())
                .await?;
        }
    }

    Ok(())
}

/// TLS 装配解析（L485）：两把钥匙都空 = 不走服务内 TLS（反代终结，默认）；
/// 都非空 = 启用（证书/私钥 PEM 路径）；只给其一是配置错误。
fn resolve_tls(cert: &str, key: &str) -> anyhow::Result<Option<(String, String)>> {
    match (cert.is_empty(), key.is_empty()) {
        (true, true) => Ok(None),
        (false, false) => Ok(Some((cert.to_string(), key.to_string()))),
        _ => Err(anyhow::anyhow!(
            "TLS 配置不完整：--tls-cert/--tls-key（ALPHA_GATEWAY_TLS_CERT/ALPHA_GATEWAY_TLS_KEY）须同给——只给其一会被静默降级为明文，拒绝启动"
        )),
    }
}

/// 账户存储装配（L476）：URL 为空 = 内存形态（默认，与历史启动行为完全
/// 一致）；非空则连 Postgres KV 表写穿账户快照——**连接失败即退出**
/// （静默降级到内存会让多副本部署各持一份互相看不见的账户数据，
/// 比启动失败更难排查）。
/// OIDC JWKS 键表初始化（L502）：jwt 模式 + 配置了端点才启用，初始拉取
/// fail-fast；off 模式配置了端点则忽略 + warn（认证关闭零行为变化）。
async fn init_jwks_store(
    client: &reqwest::Client,
    jwks_url: &str,
    jwt_mode: bool,
) -> anyhow::Result<Option<JwksStore>> {
    if jwks_url.is_empty() {
        return Ok(None);
    }
    if !jwt_mode {
        tracing::warn!("--auth-jwks-url 配置了但 auth-mode=off，忽略（认证关闭零行为变化）");
        return Ok(None);
    }
    let keys = auth::fetch_jwks(client, jwks_url)
        .await
        .map_err(|e| anyhow::anyhow!("auth jwks initial fetch failed: {e}"))?;
    if keys.is_empty() {
        anyhow::bail!("auth jwks url served no oct keys: {jwks_url}");
    }
    Ok(Some(Arc::new(tokio::sync::RwLock::new(keys))))
}

/// 单轮 JWKS 刷新（L502）：成功且非空 → 换表返回 true；空表/网络失败 →
/// 保旧表返回 false（warn）。fail-closed 语义不受影响：旧表仍在，验签照旧。
async fn refresh_jwks_once(client: &reqwest::Client, url: &str, store: &JwksStore) -> bool {
    match auth::fetch_jwks(client, url).await {
        Ok(keys) if !keys.is_empty() => {
            let kid_count = keys.len();
            *store.write().await = keys;
            tracing::info!("JWKS refreshed: {kid_count} keys");
            true
        }
        Ok(_) => {
            tracing::warn!("JWKS refresh returned no oct keys; keeping previous keys");
            false
        }
        Err(err) => {
            tracing::warn!("JWKS refresh failed; keeping previous keys: {err}");
            false
        }
    }
}

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
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            rate_limit_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            shield_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
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
                .layer(axum::middleware::from_fn(request_logger)),
        )
        .with_state(state)
}
