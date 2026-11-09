use super::engine::glob_match;
use super::*;
use chrono::Utc;
use std::collections::HashMap;

use chrono::Duration;

fn sample_request() -> DiagnosisRequest {
    DiagnosisRequest {
        alert_fingerprint: "abc123".to_string(),
        alert_name: "GatewayRateLimitExceeded".to_string(),
        severity: "critical".to_string(),
        job: "api-gateway".to_string(),
        window_start: Utc::now() - Duration::minutes(30),
        window_end: Utc::now(),
        labels: HashMap::from([
            ("job".to_string(), "api-gateway".to_string()),
            ("severity".to_string(), "critical".to_string()),
        ]),
    }
}

#[test]
fn engine_matches_gateway_rate_limit_rule() {
    let engine = DiagnosisEngine::new();
    let request = sample_request();

    let metric_evidence = vec![MetricEvidence {
        metric_name: "alpha_gateway_rate_limit_total".to_string(),
        query: "rate(alpha_gateway_rate_limit_total{mode=\"denied\"}[5m])".to_string(),
        current_value: 15.0,
        baseline_value: 0.0,
        deviation_pct: 100.0,
        anomaly_type: MetricAnomalyType::Spike,
        timestamp: Utc::now(),
    }];

    let log_evidence = vec![LogEvidence {
        pattern: "rate limit exceeded".to_string(),
        count: 42,
        sample_lines: vec!["WARN rate limit exceeded for client 1.2.3.4".to_string()],
        first_seen: Utc::now() - Duration::minutes(10),
        last_seen: Utc::now(),
        log_level: "warn".to_string(),
        container: "alpha-api-gateway".to_string(),
    }];

    let trace_evidence = vec![];

    let report = engine.diagnose(&request, metric_evidence, log_evidence, trace_evidence);

    assert!(!report.matched_rules.is_empty());
    assert_eq!(report.matched_rules[0].rule_id, "GW-001");
    assert_eq!(report.root_cause.category, RootCauseCategory::CapacityLimit);
    assert!(report.confidence_score > 70);
    assert!(!report.recommended_actions.is_empty());
}

#[test]
fn engine_matches_upstream_unhealthy_rule() {
    let engine = DiagnosisEngine::new();
    let mut request = sample_request();
    request.alert_name = "GatewayUpstreamUnhealthy".to_string();

    let metric_evidence = vec![MetricEvidence {
        metric_name: "alpha_gateway_service_health".to_string(),
        query: "alpha_gateway_service_health{job=\"data-engine\"}".to_string(),
        current_value: 0.0,
        baseline_value: 1.0,
        deviation_pct: 100.0,
        anomaly_type: MetricAnomalyType::Zero,
        timestamp: Utc::now(),
    }];

    let log_evidence = vec![LogEvidence {
        pattern: "unreachable".to_string(),
        count: 5,
        sample_lines: vec!["data-engine unreachable: connection refused".to_string()],
        first_seen: Utc::now() - Duration::minutes(5),
        last_seen: Utc::now(),
        log_level: "error".to_string(),
        container: "alpha-api-gateway".to_string(),
    }];

    let trace_evidence = vec![];

    let report = engine.diagnose(&request, metric_evidence, log_evidence, trace_evidence);

    assert!(!report.matched_rules.is_empty());
    assert_eq!(report.matched_rules[0].rule_id, "GW-002");
    assert_eq!(
        report.root_cause.category,
        RootCauseCategory::DownstreamFailure
    );
}

#[test]
fn engine_matches_shield_triggered_rule() {
    let engine = DiagnosisEngine::new();
    let mut request = sample_request();
    request.alert_name = "GatewayShieldTriggered".to_string();

    let metric_evidence = vec![MetricEvidence {
        metric_name: "alpha_gateway_shield_total".to_string(),
        query: "rate(alpha_gateway_shield_total[5m])".to_string(),
        current_value: 0.4,
        baseline_value: 0.0,
        deviation_pct: 100.0,
        anomaly_type: MetricAnomalyType::Spike,
        timestamp: Utc::now(),
    }];
    let report = engine.diagnose(&request, metric_evidence, vec![], vec![]);

    assert!(!report.matched_rules.is_empty());
    assert_eq!(report.matched_rules[0].rule_id, "GW-004");
    assert_eq!(report.root_cause.category, RootCauseCategory::CapacityLimit);
    assert!(!report.recommended_actions.is_empty());
}

#[test]
fn engine_matches_by_alert_name_without_evidence() {
    // 规则级诊断（alert-webhook 告警富化）：无证据注入也按规则 base
    // 置信度入册——告警名归属是一等信号，证据只加不减
    let engine = DiagnosisEngine::new();
    let mut request = sample_request();
    request.alert_name = "GatewayShieldTriggered".to_string();
    let report = engine.diagnose(&request, vec![], vec![], vec![]);
    assert_eq!(report.matched_rules.len(), 1);
    assert_eq!(report.matched_rules[0].rule_id, "GW-004");
    assert_eq!(report.matched_rules[0].confidence_contribution, 75);
    assert_eq!(report.root_cause.category, RootCauseCategory::CapacityLimit);
}

#[test]
fn engine_matches_collector_source_rules_without_evidence() {
    // L504 源健康两档告警（COL-001/002）：无证据按 base 置信度入册，
    // 根因分类随档位区分（降级=依赖劣化、中断=下游故障）
    let engine = DiagnosisEngine::new();

    let mut degraded = sample_request();
    degraded.alert_name = "CollectorSourceDegraded".to_string();
    let report = engine.diagnose(&degraded, vec![], vec![], vec![]);
    assert_eq!(report.matched_rules.len(), 1);
    assert_eq!(report.matched_rules[0].rule_id, "COL-001");
    assert_eq!(report.matched_rules[0].confidence_contribution, 70);
    assert_eq!(
        report.root_cause.category,
        RootCauseCategory::DependencyDegraded
    );

    let mut down = sample_request();
    down.alert_name = "CollectorSourceDown".to_string();
    let report = engine.diagnose(&down, vec![], vec![], vec![]);
    assert_eq!(report.matched_rules.len(), 1);
    assert_eq!(report.matched_rules[0].rule_id, "COL-002");
    assert_eq!(report.matched_rules[0].confidence_contribution, 85);
    assert_eq!(
        report.root_cause.category,
        RootCauseCategory::DownstreamFailure
    );
    assert!(!report.recommended_actions.is_empty());
}

#[test]
fn engine_no_match_returns_unknown() {
    let engine = DiagnosisEngine::new();
    let mut request = sample_request();
    request.alert_name = "UnknownAlert".to_string();

    let report = engine.diagnose(&request, vec![], vec![], vec![]);

    assert!(report.matched_rules.is_empty());
    assert_eq!(report.root_cause.category, RootCauseCategory::Unknown);
    assert_eq!(report.confidence_score, 0);
}

#[test]
fn glob_match_works() {
    assert!(glob_match("Gateway*", "GatewayRateLimitExceeded"));
    assert!(glob_match("*Error*", "GatewayErrorRateHigh"));
    assert!(glob_match("Test", "Test"));
    assert!(!glob_match("Foo*", "Bar"));
}

#[test]
fn suggested_actions_deduplicated_and_sorted() {
    let engine = DiagnosisEngine::new();
    let request = sample_request();

    let metric_evidence = vec![MetricEvidence {
        metric_name: "test".to_string(),
        query: "test".to_string(),
        current_value: 100.0,
        baseline_value: 10.0,
        deviation_pct: 900.0,
        anomaly_type: MetricAnomalyType::Spike,
        timestamp: Utc::now(),
    }];

    let log_evidence = vec![LogEvidence {
        pattern: "rate limit exceeded".to_string(),
        count: 10,
        sample_lines: vec![],
        first_seen: Utc::now(),
        last_seen: Utc::now(),
        log_level: "warn".to_string(),
        container: "test".to_string(),
    }];

    let report = engine.diagnose(&request, metric_evidence, log_evidence, vec![]);

    // GW-001 有两个建议动作，去重后应保留两个
    let immediate_count = report
        .recommended_actions
        .iter()
        .filter(|a| a.priority == ActionPriority::Immediate)
        .count();
    let high_count = report
        .recommended_actions
        .iter()
        .filter(|a| a.priority == ActionPriority::High)
        .count();
    assert!(immediate_count + high_count >= 2);
}
