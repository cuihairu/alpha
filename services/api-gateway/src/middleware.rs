//! 横切中间件层（原 main.rs 452-928、1297-1329 行）：shield 护栏、Redis
//! 限流、认证（JWT/OIDC + RBAC）三道门，4xx 响应助手与请求日志中间件。

use std::time::Duration;

use crate::{audit, auth, shield, GatewayState};
use alpha_storage::RateDecision;
use axum::{
    extract::State,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};

/// 客户端标识（限流主体）：X-Forwarded-For 首段 → X-Real-IP → "anonymous"。
/// 反代链路下前者由边缘代理注入，最贴近真实来源。
pub(crate) fn client_identity(headers: &HeaderMap) -> String {
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
pub(crate) fn rate_limit_rejection(decision: &RateDecision) -> Response {
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
pub(crate) fn system_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 护栏拒绝响应（与限流 429 同风格 JSON 错误体）
pub(crate) fn shield_rejection(status: StatusCode, error: &str) -> Response {
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
pub(crate) async fn shield_middleware(
    State(state): State<GatewayState>,
    req: axum::extract::Request,
    next: Next,
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
pub(crate) async fn rate_limit_middleware(
    State(state): State<GatewayState>,
    req: axum::extract::Request,
    next: Next,
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
/// OIDC 键表分流（L502）经 auth::verify_any：JWKS 在位且票带 kid 走 IdP
/// 键表校验，其余回落 HS256 自签（默认部署未配 JWKS = 零行为变化）。
/// 认证失败一律 fail-closed（与限流 fail-open 方向相反：限流器坏了可以放，
/// 认不出来是谁绝不能放）。401 不区分“缺头/坏签/过期”，防用户枚举。
pub(crate) async fn auth_middleware(
    State(state): State<GatewayState>,
    req: axum::extract::Request,
    next: Next,
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
    // JWKS 读锁仅在验签期间持有（换表写锁无长持有者，无死锁面）
    let jwks_guard = match state.jwks.as_ref() {
        Some(store) => Some(store.read().await),
        None => None,
    };
    match auth::verify_any(&state.auth, jwks_guard.as_deref(), &token) {
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
pub(crate) fn request_account_id(claims: Option<&auth::Claims>) -> String {
    claims
        .map(|c| c.sub.clone())
        .unwrap_or_else(|| alpha_core::account::LOCAL_ACCOUNT_ID.to_string())
}
/// 401 响应：WWW-Authenticate 头 + 最小 JSON 错误体（不泄露过期/伪造区分）
pub(crate) fn unauthorized() -> Response {
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
pub(crate) fn forbidden() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({
            "success": false,
            "error": "forbidden: insufficient role",
        })),
    )
        .into_response()
}
/// 请求日志中间件
pub(crate) async fn request_logger(
    req: axum::extract::Request,
    next: Next,
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
