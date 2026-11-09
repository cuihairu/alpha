//! HTTP 处理器层（原 main.rs 174-188、694-996 行）：健康聚合探测、
//! 账户/跨端同步端点、bootstrap 签发与 metrics 端点。

use std::time::{Duration, Instant};

use crate::middleware::{client_identity, request_account_id, system_now_ms, unauthorized};
use crate::{account, audit, auth, GatewayState, HEALTH_PROBE_TIMEOUT_SECS};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
};
use serde::Serialize;

/// 健康检查响应
#[derive(Debug, Serialize)]
pub(crate) struct HealthResponse {
    pub(crate) status: String,
    pub(crate) version: String,
    pub(crate) timestamp: chrono::DateTime<chrono::Utc>,
    pub(crate) services: Vec<ServiceStatus>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ServiceStatus {
    pub(crate) name: String,
    pub(crate) status: String,
    pub(crate) response_time_ms: u128,
}

/// `GET /api/v1/account/profile`：当前账户档案（不存在则按 sub 建初始档案）
pub(crate) async fn get_account_profile(
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
pub(crate) async fn put_account_profile(
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
pub(crate) async fn delete_account(
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
pub(crate) async fn post_account_sync(
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
pub(crate) async fn provision_token(
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
/// Prometheus 抓取端点：渲染全局 metrics recorder 的文本快照
pub(crate) async fn metrics_endpoint(State(state): State<GatewayState>) -> String {
    state.metrics.render()
}

/// 健康检查端点：真实探测上游 /health（data-engine 与 real-time-feed 任一不可达
/// 则整体 degraded），返回实测时延。
pub(crate) async fn health_check(State(state): State<GatewayState>) -> Json<HealthResponse> {
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
pub(crate) async fn probe_service(
    client: &reqwest::Client,
    name: &str,
    base_url: &str,
) -> ServiceStatus {
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
