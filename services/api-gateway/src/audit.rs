//! 安全审计日志与异常行为检测（TODO L487）
//!
//! **审计面**：安全相关状态变更与拒绝事件的独立通道——`emit` 以
//! `target: "audit"` 结构化输出（Loki 侧可按 target 单独采集/告警）+
//! `alpha_gateway_audit_total{event}` 指标。量纲口径：**只记攻击信号率
//! （拒绝/凭据失败/签发），不记常规放行**（放行有访问日志与
//! `alpha_gateway_requests_total`，重复记只会淹没信号）。
//!
//! **异常行为检测**：`AuthFailureDetector` 同身份滑动窗口失败风暴识别
//! （爆破特征）。检测面只标记（warn + `alpha_gateway_audit_anomaly_total`），
//! **不自动封禁**——IP 黑名单/账号锁定的封禁动作登记为后续硬化项
//! （误封共享出口 IP 的代价高于漏标，先给信号位）。
//!
//! 全部纯逻辑、时间显式入参（可测可回放）；taxonomy 事件 serde 形状
//! 与 protocols/alerts 同风格（tag 无 rename，PascalCase）。

use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 安全审计事件分类
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum AuditEvent {
    /// 票据校验失败（签名/过期错——主动攻击信号；缺失 token 是常规未认证
    /// 流量，不进审计面，量纲由护栏/限流负责）
    AuthFailure { identity: String, reason: String },
    /// RBAC 授权拒绝（认证通过但越权——内部威胁/越权探测信号）
    AccessDenied {
        identity: String,
        path: String,
        reason: String,
    },
    /// bootstrap 签发密钥错误（签发面爆破信号）
    TokenProvisionDenied { identity: String },
    /// bootstrap 签发成功（状态变更，必记）
    TokenProvisioned { sub: String },
}

impl AuditEvent {
    /// 指标标签值（snake_case 事件名）
    pub fn kind(&self) -> &'static str {
        match self {
            AuditEvent::AuthFailure { .. } => "auth_failure",
            AuditEvent::AccessDenied { .. } => "access_denied",
            AuditEvent::TokenProvisionDenied { .. } => "token_provision_denied",
            AuditEvent::TokenProvisioned { .. } => "token_provisioned",
        }
    }
}

/// 发射一条审计事件：audit target 结构化日志 + 计数指标。序列化失败
/// 降级为只记 kind（审计面不可因载荷问题丢事件）。
pub fn emit(event: &AuditEvent) {
    metrics::counter!("alpha_gateway_audit_total", "event" => event.kind()).increment(1);
    let payload = serde_json::to_string(event)
        .unwrap_or_else(|_| format!("{{\"type\":\"{}\"}}", event.kind()));
    tracing::info!(target: "audit", event = %payload, "audit");
}

/// 分桶上限与闲置逐出（同 shield 护栏口径：防身份伪造撑爆内存）
const TRACK_EVICT: usize = 10_000;
const TRACK_IDLE_MS: i64 = 60_000;

/// 认证失败风暴检测：同身份滑动窗口内 AuthFailure 次数 ≥ 阈值 → 疑似爆破
#[derive(Debug)]
pub struct AuthFailureDetector {
    window_ms: i64,
    threshold: usize,
    tracks: HashMap<String, Vec<i64>>,
}

impl AuthFailureDetector {
    pub fn new(window_ms: i64, threshold: usize) -> Self {
        Self {
            window_ms: window_ms.max(1),
            threshold: threshold.max(1),
            tracks: HashMap::new(),
        }
    }

    /// 记录一次失败并判定：true = 窗口内失败数达阈（风暴信号位，重复
    /// 触发持续为 true——调用方按此持续告警，直到窗口滑出）
    pub fn observe(&mut self, identity: &str, now_ms: i64) -> bool {
        let (window_ms, threshold) = (self.window_ms, self.threshold);
        let count = {
            let track = self.tracks.entry(identity.to_string()).or_default();
            track.retain(|ts| now_ms - *ts < window_ms);
            track.push(now_ms);
            track.len()
        };
        if self.tracks.len() > TRACK_EVICT {
            self.tracks.retain(|_, track| {
                track
                    .last()
                    .map(|ts| now_ms - *ts < TRACK_IDLE_MS)
                    .unwrap_or(false)
            });
        }
        count >= threshold
    }
}

/// 审计运行态（GatewayState 成员；Arc 共享检测器推进状态）
#[derive(Debug, Clone)]
pub struct AuditState {
    pub detector: Arc<Mutex<AuthFailureDetector>>,
}

impl AuditState {
    pub fn new(window_ms: i64, threshold: usize) -> Self {
        Self {
            detector: Arc::new(Mutex::new(AuthFailureDetector::new(window_ms, threshold))),
        }
    }

    /// env 装配：窗口（默认 60s）与阈值（默认 20），非法值回退默认
    pub fn from_env() -> Self {
        let window_ms = std::env::var("ALPHA_GATEWAY_AUDIT_FAILURE_WINDOW_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60_000);
        let threshold = std::env::var("ALPHA_GATEWAY_AUDIT_FAILURE_THRESHOLD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(20);
        Self::new(window_ms, threshold)
    }

    /// 记认证失败并按风暴信号发 warn（auth 中间件路径）
    pub fn observe_auth_failure(&self, identity: &str, reason: &str, now_ms: i64) {
        emit(&AuditEvent::AuthFailure {
            identity: identity.to_string(),
            reason: reason.to_string(),
        });
        self.note_failure_for_storm(identity, now_ms);
    }

    /// 失败计入风暴检测（不产出事件——调用方已自行 emit 对应事件，
    /// 如 bootstrap 签发密钥错误发 TokenProvisionDenied）
    pub fn note_failure_for_storm(&self, identity: &str, now_ms: i64) {
        let storm = self.detector.lock().unwrap().observe(identity, now_ms);
        if storm {
            metrics::counter!("alpha_gateway_audit_anomaly_total").increment(1);
            tracing::warn!(
                target: "audit",
                identity = %identity,
                "auth failure storm detected (no auto-block; review required)"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_event_taxonomy_shapes() {
        // serde tag 原名 PascalCase + 指标标签 snake_case 的契约锚
        let event = AuditEvent::AuthFailure {
            identity: "10.0.0.1".into(),
            reason: "invalid_token".into(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "AuthFailure");
        assert_eq!(json["identity"], "10.0.0.1");
        assert_eq!(event.kind(), "auth_failure");

        let denied = AuditEvent::AccessDenied {
            identity: "alice".into(),
            path: "/api/v1/admin".into(),
            reason: "rbac".into(),
        };
        assert_eq!(
            serde_json::to_value(&denied).unwrap()["type"],
            "AccessDenied"
        );
        assert_eq!(denied.kind(), "access_denied");
        assert_eq!(
            AuditEvent::TokenProvisioned { sub: "bob".into() }.kind(),
            "token_provisioned"
        );
    }

    #[test]
    fn failure_storm_flags_at_threshold_and_recovers_after_window() {
        let mut detector = AuthFailureDetector::new(60_000, 3);
        // 阈下不标记
        assert!(!detector.observe("a", 1));
        assert!(!detector.observe("a", 2));
        // 第 3 次达阈 → 风暴位（此后窗口内持续为真）
        assert!(detector.observe("a", 3));
        assert!(detector.observe("a", 4));
        // 不同身份独立计数
        assert!(!detector.observe("b", 5));
        // 窗口滑出：旧失败清空，重新累计
        assert!(!detector.observe("a", 60_100));
        assert!(!detector.observe("a", 60_101));
        assert!(detector.observe("a", 60_102));
    }
}
