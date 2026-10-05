//! Alertmanager Webhook 接收器（L464 实时告警）
//!
//! 接收 Alertmanager 转发的告警 payload，解析后按渠道分发：
//! - 钉钉/企业微信/Slack/PagerDuty Webhook（env 注入，缺失则仅日志记录）
//! - 结构化日志（tracing）供 Loki 采集、Grafana 告警面板联动
//!
//! 告警 payload 结构遵循 Alertmanager v2 标准：
//! https://prometheus.io/docs/alerting/latest/configuration/#webhook_config

use alpha_storage::diagnosis::{DiagnosisEngine, DiagnosisRequest};
use axum::{
    extract::{Extension, Json},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json as JsonResponse},
    routing::{get, post},
    Router,
};
use chrono::{DateTime, Utc};
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tracing::{error, info, warn};

const WEBHOOK_TIMEOUT_SECS: u64 = 10;

/// 标签取值（缺失回退默认值；直接 unwrap_or(&"...".to_string())
/// 会借用函数内临时值导致 E0716，必须走此 helper）
fn label_or<'a>(map: &'a HashMap<String, String>, key: &str, fallback: &'a str) -> &'a str {
    map.get(key).map(String::as_str).unwrap_or(fallback)
}

/// 应用共享状态
#[derive(Clone)]
struct AppState {
    http: HttpClient,
    channels: NotificationChannels,
}

/// 可配置的通知渠道（全部可选，缺失不阻塞启动）
#[derive(Clone, Default)]
struct NotificationChannels {
    dingtalk_webhook: Option<String>,
    wechat_webhook: Option<String>,
    slack_webhook: Option<String>,
    pagerduty_key: Option<String>,
}

impl NotificationChannels {
    fn from_env() -> Self {
        Self {
            dingtalk_webhook: std::env::var("DINGTALK_WEBHOOK_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            wechat_webhook: std::env::var("WECHAT_WEBHOOK_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            slack_webhook: std::env::var("SLACK_WEBHOOK_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            pagerduty_key: std::env::var("PAGERDUTY_INTEGRATION_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
        }
    }

    fn any_configured(&self) -> bool {
        self.dingtalk_webhook.is_some()
            || self.wechat_webhook.is_some()
            || self.slack_webhook.is_some()
            || self.pagerduty_key.is_some()
    }
}

/// Alertmanager v2 告警 payload（精简必要字段）
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AlertmanagerPayload {
    receiver: String,
    status: String, // "firing" | "resolved"
    alerts: Vec<Alert>,
    group_labels: HashMap<String, String>,
    common_labels: HashMap<String, String>,
    common_annotations: HashMap<String, String>,
    // Alertmanager 原生字段为全大写 URL（externalURL），camelCase 推导是 externalUrl，对不上
    #[serde(rename = "externalURL")]
    external_url: String,
    version: String,
    group_key: String,
    truncated_alerts: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Alert {
    status: String,
    labels: HashMap<String, String>,
    annotations: HashMap<String, String>,
    starts_at: DateTime<Utc>,
    ends_at: Option<DateTime<Utc>>,
    // 同上：原生 generatorURL 全大写
    #[serde(default, rename = "generatorURL")]
    generator_url: String,
    fingerprint: String,
}

/// 钉钉机器人消息格式
#[derive(Debug, Serialize)]
#[serde(tag = "msgtype")]
enum DingTalkMessage {
    #[serde(rename = "markdown")]
    Markdown {
        markdown: DingTalkMarkdown,
        at: Option<DingTalkAt>,
    },
}

#[derive(Debug, Serialize)]
struct DingTalkMarkdown {
    title: String,
    text: String,
}

#[derive(Debug, Serialize, Default)]
struct DingTalkAt {
    #[serde(skip_serializing_if = "Option::is_none")]
    at_mobiles: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    at_user_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_at_all: Option<bool>,
}

/// 企业微信消息格式
#[derive(Debug, Serialize)]
#[serde(tag = "msgtype")]
enum WeChatMessage {
    #[serde(rename = "markdown")]
    Markdown { markdown: WeChatMarkdown },
}

#[derive(Debug, Serialize)]
struct WeChatMarkdown {
    content: String,
}

/// Slack 消息格式（Block Kit 简化）
#[derive(Debug, Serialize)]
struct SlackMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    blocks: Vec<SlackBlock>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum SlackBlock {
    #[serde(rename = "header")]
    Header { text: SlackText },
    #[serde(rename = "section")]
    Section { text: SlackText },
    #[serde(rename = "divider")]
    Divider,
}

#[derive(Debug, Serialize)]
struct SlackText {
    #[serde(rename = "type")]
    kind: String, // "plain_text" | "mrkdwn"
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    emoji: Option<bool>,
}

/// PagerDuty Events API v2 格式
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PagerDutyEvent {
    routing_key: String,
    event_action: String, // "trigger" | "resolve"
    dedup_key: String,
    payload: PagerDutyPayload,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PagerDutyPayload {
    summary: String,
    severity: String, // "critical" | "error" | "warning" | "info"
    source: String,
    component: Option<String>,
    group: Option<String>,
    class: Option<String>,
    custom_details: HashMap<String, serde_json::Value>,
}

/// 应用错误
#[derive(Error, Debug)]
enum AppError {
    #[error("HTTP 请求失败: {0}")]
    Http(#[from] reqwest::Error),
    #[error("序列化失败: {0}")]
    Serde(#[from] serde_json::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            AppError::Http(e) => (StatusCode::BAD_GATEWAY, format!("上游请求失败: {e}")),
            AppError::Serde(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("序列化错误: {e}"),
            ),
        };
        (
            status,
            JsonResponse(serde_json::json!({ "error": message })),
        )
            .into_response()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,alpha_alert_webhook=debug".into()),
        )
        .init();

    info!("Starting Alpha Alert Webhook Receiver");

    let state = AppState {
        http: HttpClient::builder()
            .timeout(Duration::from_secs(WEBHOOK_TIMEOUT_SECS))
            .build()?,
        channels: NotificationChannels::from_env(),
    };

    if state.channels.any_configured() {
        info!(
            "Notification channels configured: dingtalk={}, wechat={}, slack={}, pagerduty={}",
            state.channels.dingtalk_webhook.is_some(),
            state.channels.wechat_webhook.is_some(),
            state.channels.slack_webhook.is_some(),
            state.channels.pagerduty_key.is_some()
        );
    } else {
        warn!("No notification channels configured (DINGTALK_WEBHOOK_URL, WECHAT_WEBHOOK_URL, SLACK_WEBHOOK_URL, PAGERDUTY_INTEGRATION_KEY). Alerts will only be logged.");
    }

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/alerts", post(receive_alerts))
        .route("/alerts/critical", post(receive_critical_alerts))
        .layer(Extension(Arc::new(state)));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    info!("Alert webhook listening on 0.0.0.0:8080");

    axum::serve(listener, app).await?;

    Ok(())
}

/// 健康检查
async fn health_check() -> &'static str {
    "OK"
}

/// 通用告警接收端点（默认渠道）
async fn receive_alerts(
    Extension(state): Extension<Arc<AppState>>,
    _headers: HeaderMap,
    Json(payload): Json<AlertmanagerPayload>,
) -> Result<StatusCode, AppError> {
    handle_alerts(state, "default", payload).await
}

/// 关键告警专用端点（可配更激进的重试/升级）
async fn receive_critical_alerts(
    Extension(state): Extension<Arc<AppState>>,
    _headers: HeaderMap,
    Json(payload): Json<AlertmanagerPayload>,
) -> Result<StatusCode, AppError> {
    handle_alerts(state, "critical", payload).await
}

/// 统一处理告警：解析 → 结构化日志 → 多渠道分发
async fn handle_alerts(
    state: Arc<AppState>,
    route: &str,
    payload: AlertmanagerPayload,
) -> Result<StatusCode, AppError> {
    // Arc 共享：各渠道 spawn 任务分别持有，不整体移动 payload
    let payload = Arc::new(payload);

    // 结构化日志：Loki 采集 + Grafana 告警面板查询
    for alert in &payload.alerts {
        let labels_json = serde_json::to_string(&alert.labels).unwrap_or_default();
        let annotations_json = serde_json::to_string(&alert.annotations).unwrap_or_default();

        info!(
            route = %route,
            alertname = %label_or(&alert.labels, "alertname", "unknown"),
            severity = %label_or(&alert.labels, "severity", "unknown"),
            job = %label_or(&alert.labels, "job", "unknown"),
            status = %alert.status,
            fingerprint = %alert.fingerprint,
            starts_at = %alert.starts_at,
            labels = %labels_json,
            annotations = %annotations_json,
            "Alertmanager 告警接收"
        );
    }

    // 构建通用消息文本（各渠道复用）
    let message = build_message(&payload, route);

    // 并行分发到所有已配置渠道
    let mut tasks = Vec::new();

    if let Some(url) = &state.channels.dingtalk_webhook {
        let url = url.clone();
        let msg = message.clone();
        let http = state.http.clone();
        let payload = Arc::clone(&payload);
        tasks.push(tokio::spawn(async move {
            send_dingtalk(&http, &url, &payload, &msg).await
        }));
    }
    if let Some(url) = &state.channels.wechat_webhook {
        let url = url.clone();
        let msg = message.clone();
        let http = state.http.clone();
        let payload = Arc::clone(&payload);
        tasks.push(tokio::spawn(async move {
            send_wechat(&http, &url, &payload, &msg).await
        }));
    }
    if let Some(url) = &state.channels.slack_webhook {
        let url = url.clone();
        let msg = message.clone();
        let http = state.http.clone();
        let payload = Arc::clone(&payload);
        tasks.push(tokio::spawn(async move {
            send_slack(&http, &url, &payload, &msg).await
        }));
    }
    if let Some(key) = &state.channels.pagerduty_key {
        let key = key.clone();
        let http = state.http.clone();
        let payload = Arc::clone(&payload);
        tasks.push(tokio::spawn(async move {
            send_pagerduty(&http, &key, &payload).await
        }));
    }

    // 等待所有分发完成（不阻塞 HTTP 响应，fire-and-forget 语义）
    // 实际生产可改为有界通道 + 后台 worker 池
    for task in tasks {
        if let Err(e) = task.await {
            error!("通知分发任务 panic: {e}");
        }
    }

    Ok(StatusCode::OK)
}

/// Alert → 规则知识库诊断请求（窗口 = starts_at 前推 15 分钟 .. 结束/现在；
/// 无证据注入——规则级初判，指标/日志/追踪证据增强随后续接线）
fn alert_diagnosis_request(alert: &Alert) -> DiagnosisRequest {
    DiagnosisRequest {
        alert_fingerprint: alert.fingerprint.clone(),
        alert_name: label_or(&alert.labels, "alertname", "").to_string(),
        severity: label_or(&alert.labels, "severity", "unknown").to_string(),
        job: label_or(&alert.labels, "job", "").to_string(),
        window_start: alert.starts_at - chrono::Duration::minutes(15),
        window_end: alert.ends_at.unwrap_or_else(Utc::now),
        labels: alert.labels.clone(),
    }
}

/// 构建人类可读的告警消息（Markdown 兼容）
fn build_message(payload: &AlertmanagerPayload, route: &str) -> String {
    let mut lines = Vec::new();

    lines.push(format!(
        "**[{}] Alpha Finance 告警 — {}**",
        payload.status.to_uppercase(),
        route.to_uppercase()
    ));
    lines.push(String::new());

    // 规则知识库诊断富化（alerting §5）：告警名 → builtin 规则匹配给根因
    // 初判与建议动作——纯函数零 IO，不拉指标/日志；未匹配到已知模式时
    // 不硬塞根因，保持原有信息面
    let engine = DiagnosisEngine::new();

    for alert in &payload.alerts {
        let alertname = label_or(&alert.labels, "alertname", "Unknown");
        let severity = label_or(&alert.labels, "severity", "unknown");
        let job = label_or(&alert.labels, "job", "unknown");
        let summary = label_or(&alert.annotations, "summary", "");
        let description = label_or(&alert.annotations, "description", "");
        let runbook = label_or(&alert.annotations, "runbook", "");

        lines.push(format!("### 🔔 {alertname} (`{severity}`)"));
        lines.push(format!("- **作业**: {job}"));
        lines.push(format!("- **状态**: {}", alert.status));
        lines.push(format!("- **摘要**: {summary}"));
        if !description.is_empty() {
            lines.push(format!("- **详情**: {description}"));
        }
        if !runbook.is_empty() {
            lines.push(format!("- **运行手册**: {runbook}"));
        }
        let report = engine.diagnose(
            &alert_diagnosis_request(alert),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        if let Some(primary) = report
            .matched_rules
            .iter()
            .max_by_key(|r| r.confidence_contribution)
        {
            lines.push(format!(
                "- **根因初判**: {}（`{}` · {}，置信 {}）",
                report.root_cause.description,
                primary.rule_id,
                primary.rule_name,
                report.confidence_score
            ));
            for action in report.recommended_actions.iter().take(2) {
                lines.push(format!("  - 建议: {}", action.description));
            }
        }
        lines.push(format!(
            "- **起始**: {}",
            alert.starts_at.format("%Y-%m-%d %H:%M:%S UTC")
        ));
        if let Some(ends_at) = alert.ends_at {
            lines.push(format!(
                "- **结束**: {}",
                ends_at.format("%Y-%m-%d %H:%M:%S UTC")
            ));
        }
        lines.push(String::new());
    }

    if payload.truncated_alerts > 0 {
        lines.push(format!(
            "_... 另有 {} 条告警被截断_",
            payload.truncated_alerts
        ));
    }

    lines.push(format!("- **Alertmanager**: {}", payload.external_url));
    lines.push(format!("- **分组键**: `{}`", payload.group_key));

    lines.join("\n")
}

/// 发送钉钉
async fn send_dingtalk(
    http: &HttpClient,
    webhook_url: &str,
    payload: &AlertmanagerPayload,
    message: &str,
) -> Result<(), AppError> {
    let title = format!(
        "Alpha Finance 告警 — {} ({})",
        payload.status.to_uppercase(),
        payload
            .alerts
            .first()
            .map(|a| label_or(&a.labels, "alertname", "Unknown"))
            .unwrap_or("Unknown")
    );

    let msg = DingTalkMessage::Markdown {
        markdown: DingTalkMarkdown {
            title,
            text: message.to_string(),
        },
        at: None,
    };

    let resp = http.post(webhook_url).json(&msg).send().await?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        error!(%webhook_url, status=%status, body=%text, "钉钉发送失败");
    } else {
        info!(%webhook_url, "钉钉告警发送成功");
    }
    Ok(())
}

/// 发送企业微信
async fn send_wechat(
    http: &HttpClient,
    webhook_url: &str,
    _payload: &AlertmanagerPayload,
    message: &str,
) -> Result<(), AppError> {
    let msg = WeChatMessage::Markdown {
        markdown: WeChatMarkdown {
            content: message.to_string(),
        },
    };

    let resp = http.post(webhook_url).json(&msg).send().await?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        error!(%webhook_url, status=%status, body=%text, "企业微信发送失败");
    } else {
        info!(%webhook_url, "企业微信告警发送成功");
    }
    Ok(())
}

/// 发送 Slack
async fn send_slack(
    http: &HttpClient,
    webhook_url: &str,
    payload: &AlertmanagerPayload,
    message: &str,
) -> Result<(), AppError> {
    let blocks = vec![
        SlackBlock::Header {
            text: SlackText {
                kind: "plain_text".to_string(),
                text: format!("Alpha Finance 告警 — {}", payload.status.to_uppercase()),
                emoji: Some(true),
            },
        },
        SlackBlock::Divider,
        SlackBlock::Section {
            text: SlackText {
                kind: "mrkdwn".to_string(),
                text: message.to_string(),
                emoji: None,
            },
        },
    ];

    let msg = SlackMessage {
        text: Some(format!("Alpha Finance 告警: {}", payload.status)),
        blocks,
    };

    let resp = http.post(webhook_url).json(&msg).send().await?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        error!(%webhook_url, status=%status, body=%text, "Slack 发送失败");
    } else {
        info!(%webhook_url, "Slack 告警发送成功");
    }
    Ok(())
}

/// 发送 PagerDuty
async fn send_pagerduty(
    http: &HttpClient,
    integration_key: &str,
    payload: &AlertmanagerPayload,
) -> Result<(), AppError> {
    for alert in &payload.alerts {
        let event_action = if alert.status == "firing" {
            "trigger"
        } else {
            "resolve"
        };
        let severity = label_or(&alert.labels, "severity", "warning").to_string();

        let mut custom_details = HashMap::new();
        for (k, v) in &alert.labels {
            custom_details.insert(k.clone(), serde_json::Value::String(v.clone()));
        }
        for (k, v) in &alert.annotations {
            custom_details.insert(k.clone(), serde_json::Value::String(v.clone()));
        }
        custom_details.insert(
            "generator_url".to_string(),
            serde_json::Value::String(alert.generator_url.clone()),
        );
        custom_details.insert(
            "fingerprint".to_string(),
            serde_json::Value::String(alert.fingerprint.clone()),
        );

        let event = PagerDutyEvent {
            routing_key: integration_key.to_string(),
            event_action: event_action.to_string(),
            dedup_key: alert.fingerprint.clone(),
            payload: PagerDutyPayload {
                summary: label_or(&alert.annotations, "summary", "Alert").to_string(),
                severity,
                source: "alpha-finance".to_string(),
                component: alert.labels.get("job").cloned(),
                group: alert.labels.get("alertname").cloned(),
                class: None,
                custom_details,
            },
        };

        let resp = http
            .post("https://events.pagerduty.com/v2/enqueue")
            .json(&event)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            error!(status=%status, body=%text, "PagerDuty 发送失败");
        } else {
            info!(fingerprint=%alert.fingerprint, action=%event_action, "PagerDuty 事件发送成功");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_channels_from_env_empty() {
        std::env::remove_var("DINGTALK_WEBHOOK_URL");
        std::env::remove_var("WECHAT_WEBHOOK_URL");
        std::env::remove_var("SLACK_WEBHOOK_URL");
        std::env::remove_var("PAGERDUTY_INTEGRATION_KEY");
        let ch = NotificationChannels::from_env();
        assert!(!ch.any_configured());
    }

    #[test]
    fn build_message_formats_correctly() {
        let mut labels = HashMap::new();
        labels.insert(
            "alertname".to_string(),
            "GatewayRateLimitExceeded".to_string(),
        );
        labels.insert("severity".to_string(), "critical".to_string());
        labels.insert("job".to_string(), "api-gateway".to_string());

        let mut annotations = HashMap::new();
        annotations.insert("summary".to_string(), "API 网关限流触发".to_string());
        annotations.insert("description".to_string(), "检测到请求被拒绝".to_string());
        annotations.insert(
            "runbook".to_string(),
            "https://wiki.example.com".to_string(),
        );

        let alert = Alert {
            status: "firing".to_string(),
            labels,
            annotations,
            starts_at: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            ends_at: None,
            generator_url: "http://prometheus:9090".to_string(),
            fingerprint: "abc123".to_string(),
        };

        let payload = AlertmanagerPayload {
            receiver: "default-webhook".to_string(),
            status: "firing".to_string(),
            alerts: vec![alert],
            group_labels: HashMap::new(),
            common_labels: HashMap::new(),
            common_annotations: HashMap::new(),
            external_url: "http://alertmanager:9093".to_string(),
            version: "2.0".to_string(),
            group_key: "test-group".to_string(),
            truncated_alerts: 0,
        };

        let msg = build_message(&payload, "default");
        assert!(msg.contains("GatewayRateLimitExceeded"));
        assert!(msg.contains("critical"));
        assert!(msg.contains("API 网关限流触发"));
        assert!(msg.contains("检测到请求被拒绝"));
        assert!(msg.contains("https://wiki.example.com"));
        // 规则知识库诊断富化（alerting §5）：告警名命中 builtin 规则 →
        // 根因初判 + 规则 ID + 建议动作
        assert!(msg.contains("根因初判"), "诊断富化缺失:\n{msg}");
        assert!(msg.contains("GW-001"), "应带命中规则 ID:\n{msg}");
        assert!(msg.contains("建议"), "应带建议动作:\n{msg}");
    }

    #[test]
    fn build_message_leaves_unknown_alert_without_root_cause() {
        // 未匹配已知模式的告警不硬塞根因（保持原有信息面）
        let mut labels = HashMap::new();
        labels.insert("alertname".to_string(), "NeverSeenAlert".to_string());
        labels.insert("severity".to_string(), "warning".to_string());
        let alert = Alert {
            status: "firing".to_string(),
            labels,
            annotations: HashMap::new(),
            starts_at: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            ends_at: None,
            generator_url: String::new(),
            fingerprint: "x".to_string(),
        };
        let payload = AlertmanagerPayload {
            receiver: "default".to_string(),
            status: "firing".to_string(),
            alerts: vec![alert],
            group_labels: HashMap::new(),
            common_labels: HashMap::new(),
            common_annotations: HashMap::new(),
            external_url: String::new(),
            version: "2.0".to_string(),
            group_key: "g".to_string(),
            truncated_alerts: 0,
        };
        let msg = build_message(&payload, "default");
        assert!(msg.contains("NeverSeenAlert"));
        assert!(!msg.contains("根因初判"));
    }

    #[test]
    fn alertmanager_payload_deserializes() {
        let json = r#"{
            "receiver": "default-webhook",
            "status": "firing",
            "alerts": [{
                "status": "firing",
                "labels": {"alertname": "TestAlert", "severity": "warning", "job": "test"},
                "annotations": {"summary": "Test", "description": "Desc"},
                "startsAt": "2026-10-02T12:00:00Z",
                "endsAt": null,
                "generatorURL": "http://prometheus:9090",
                "fingerprint": "abc123"
            }],
            "groupLabels": {},
            "commonLabels": {},
            "commonAnnotations": {},
            "externalURL": "http://alertmanager:9093",
            "version": "2.0",
            "groupKey": "test",
            "truncatedAlerts": 0
        }"#;

        let payload: AlertmanagerPayload = serde_json::from_str(json).expect("反序列化应成功");
        assert_eq!(payload.receiver, "default-webhook");
        assert_eq!(payload.alerts.len(), 1);
        assert_eq!(payload.alerts[0].labels["alertname"], "TestAlert");
    }
}
