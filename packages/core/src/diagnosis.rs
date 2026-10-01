//! 智能故障诊断（L464）：指标快照 → 规则推理 → 根因建议。
//!
//! 与 `config/alpha-alerts.yml` 的 PromQL 告警**同阈值口径**分工：
//! 告警管「通知」（Alertmanager 推送），本模块管「判读」——对一次采集到的
//! 指标快照做规则推理，并在多信号并存时给出**根因优先级关联**
//! （如网关 5xx 升高且上游服务宕机 → 根因指向上游而非网关本身）。
//!
//! 设计取舍：
//! - 字段全 `Option`：数据源缺失（未接该指标）时**规则跳过而非误报**——
//!   「没数据」不等于「有故障」；
//! - 阈值常量集中暴露（`THRESHOLD_*`），与告警规则文件同源维护；
//! - 纯函数零 IO：采集（Prometheus HTTP API 查询）归 `tools/diagnose`，
//!   本模块只做推理——无外部依赖即可全量单测。

use std::fmt;

use serde::{Deserialize, Serialize};

/// 5xx 错误率阈值（与 GatewayErrorRateHigh 一致，比率 0~1）
pub const THRESHOLD_ERROR_RATE: f64 = 0.05;
/// p95 延迟阈值毫秒（与 DataEngineQueryLatencyHigh 一致：2s）
pub const THRESHOLD_P95_LATENCY_MS: f64 = 2000.0;
/// 内存使用率阈值（与 DataEngineMemoryPressure 一致，比率 0~1）
pub const THRESHOLD_MEMORY_RATIO: f64 = 0.85;
/// 未释放分配数阈值（与 AllocTrackerUnreleased 一致）
pub const THRESHOLD_UNRELEASED_ALLOCS: u64 = 100;
/// 行情消息速率下限 msg/s（与 RealtimeFeedMessageGap 一致）
pub const THRESHOLD_MESSAGE_RATE: f64 = 1.0;

/// 单服务指标快照：每个字段 `Option` 表示该数据源是否可得。
/// 缺失字段对应的规则跳过（缺数据 ≠ 故障）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServiceMetrics {
    /// 服务标识（如 `api-gateway`，用于报告与关联定位）
    pub service: String,
    /// Prometheus `up`：scrape 可达性（false = 实例宕/端口失联）
    pub up: Option<bool>,
    /// 近窗 5xx 错误率（0~1）
    pub error_rate: Option<f64>,
    /// p95 延迟（毫秒）
    pub p95_latency_ms: Option<f64>,
    /// 内存使用率（0~1）
    pub memory_used_ratio: Option<f64>,
    /// TrackingAllocator 未释放分配数
    pub unreleased_allocs: Option<u64>,
    /// 行情上游连接状态（realtime-feed 专属）
    pub feed_connected: Option<bool>,
    /// 行情消息速率（msg/s）
    pub message_rate: Option<f64>,
}

impl ServiceMetrics {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            ..Default::default()
        }
    }
}

/// 一次采集的全量快照
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub services: Vec<ServiceMetrics>,
}

/// 诊断结论级别（派生序：Info < Warning < Critical，报告按此排序输出）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Severity::Info => "INFO",
            Severity::Warning => "WARNING",
            Severity::Critical => "CRITICAL",
        })
    }
}

/// 单条诊断发现
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// 所属服务（`service` 字段为空的全局性发现用 `"*"`）
    pub service: String,
    /// 规则标识（与 alpha-alerts.yml 的 alert 名对齐，便于双面互查）
    pub rule: &'static str,
    pub severity: Severity,
    /// 证据：触发时的实测值（报告要可复查）
    pub evidence: String,
    /// 处置建议（可执行的下一步）
    pub suggestion: String,
    /// 根因关联：非 None 表示本发现疑似由所指根因导致（`rule:service`）
    pub root_cause_of: Option<String>,
}

/// 诊断报告：findings 按 severity 降序、同级按服务名稳定排序
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Diagnosis {
    pub findings: Vec<Finding>,
}

impl Diagnosis {
    /// 最高级别（无发现 = Info，即健康）
    pub fn level(&self) -> Severity {
        self.findings
            .iter()
            .map(|f| f.severity)
            .max()
            .unwrap_or(Severity::Info)
    }

    pub fn is_healthy(&self) -> bool {
        self.findings.is_empty()
    }

    pub fn has_critical(&self) -> bool {
        self.findings
            .iter()
            .any(|f| f.severity == Severity::Critical)
    }
}

/// 对快照做规则推理（纯函数）。
///
/// 两遍推理：第一遍逐服务跑规则产 finding；第二遍做**根因关联**——
/// 「错误率升高」类发现在存在宕机上游时标注根因指向，处置建议随之改写。
pub fn diagnose(snapshot: &Snapshot) -> Diagnosis {
    let mut findings: Vec<Finding> = snapshot.services.iter().flat_map(findings_for).collect();

    // 关联推断：宕机服务集合（第二遍读第一遍结果，无借用纠缠）
    let down: Vec<String> = snapshot
        .services
        .iter()
        .filter(|s| s.up == Some(false))
        .map(|s| s.service.clone())
        .collect();

    for f in &mut findings {
        // 规则 1 的发现本身即根因候选，不自我关联
        if f.rule == "ServiceDown" {
            continue;
        }
        // 网关 5xx 升高 + 存在宕机上游 → 根因指向上游
        if f.rule == "GatewayErrorRateHigh" {
            if let Some(cause) = down.first() {
                f.root_cause_of = Some(format!("ServiceDown:{cause}"));
                f.suggestion = format!(
                    "上游 {cause} 宕机疑似根因：优先恢复该服务（docker logs alpha-{cause}），而非排查网关本身"
                );
            }
        }
        // 行情消息断流 + 连接已断 → 连接断是根因（消息率低是表象）
        if f.rule == "RealtimeFeedMessageGap" {
            let feed_down = snapshot
                .services
                .iter()
                .any(|s| s.feed_connected == Some(false));
            if feed_down {
                f.root_cause_of = Some("RealtimeFeedConnectionLoss:real-time-feed".into());
                f.suggestion =
                    "行情连接已断（上表 RealtimeFeedConnectionLoss）：消息断流是表象，先重连上游行情源"
                        .to_string();
            }
        }
    }

    findings.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.service.cmp(&b.service))
            .then_with(|| a.rule.cmp(b.rule))
    });
    Diagnosis { findings }
}

/// 单服务规则集（缺数据的规则跳过——缺数据 ≠ 故障）
fn findings_for(s: &ServiceMetrics) -> Vec<Finding> {
    let mut out = Vec::new();

    if s.up == Some(false) {
        out.push(Finding {
            service: s.service.clone(),
            rule: "ServiceDown",
            severity: Severity::Critical,
            evidence: "prometheus up == 0（scrape 不可达）".into(),
            suggestion: format!(
                "确认进程与端口：docker ps | grep alpha-{} && docker logs alpha-{} --tail 50",
                s.service, s.service
            ),
            root_cause_of: None,
        });
    }

    if let Some(rate) = s.error_rate {
        if rate > THRESHOLD_ERROR_RATE {
            out.push(Finding {
                service: s.service.clone(),
                rule: "GatewayErrorRateHigh",
                severity: Severity::Warning,
                evidence: format!("5xx 错误率 {:.2}% > {:.0}%", rate * 100.0, THRESHOLD_ERROR_RATE * 100.0),
                suggestion: String::from(
                    "取样 5xx 请求的 trace-id，LogQL `{job=~\"alpha-.*\"} |= \"<trace-id>\"` 串联定位",
                ),
                root_cause_of: None,
            });
        }
    }

    if let Some(ms) = s.p95_latency_ms {
        if ms > THRESHOLD_P95_LATENCY_MS {
            out.push(Finding {
                service: s.service.clone(),
                rule: "QueryLatencyHigh",
                severity: Severity::Warning,
                evidence: format!("p95 延迟 {:.0}ms > {:.0}ms", ms, THRESHOLD_P95_LATENCY_MS),
                suggestion: "检查慢 SQL 与分区裁剪（data-engine 湖层 trade_date 裁剪归 L447/L445）"
                    .into(),
                root_cause_of: None,
            });
        }
    }

    if let Some(ratio) = s.memory_used_ratio {
        if ratio > THRESHOLD_MEMORY_RATIO {
            out.push(Finding {
                service: s.service.clone(),
                rule: "MemoryPressure",
                severity: Severity::Warning,
                evidence: format!("内存使用率 {:.1}% > {:.0}%", ratio * 100.0, THRESHOLD_MEMORY_RATIO * 100.0),
                suggestion:
                    "核对进程 RSS 走势（process_resident_memory_bytes）与数据页缓存占用，判断是缓存还是泄漏"
                        .into(),
                root_cause_of: None,
            });
        }
    }

    if let Some(n) = s.unreleased_allocs {
        if n > THRESHOLD_UNRELEASED_ALLOCS {
            out.push(Finding {
                service: s.service.clone(),
                rule: "AllocTrackerUnreleased",
                severity: Severity::Warning,
                evidence: format!("未释放分配 {n} > {THRESHOLD_UNRELEASED_ALLOCS}"),
                suggestion:
                    "对照 docs/memory-profiling.md：测试语境即泄漏，长驻服务看存活字节趋势是否单调只增"
                        .into(),
                root_cause_of: None,
            });
        }
    }

    if s.feed_connected == Some(false) {
        out.push(Finding {
            service: s.service.clone(),
            rule: "RealtimeFeedConnectionLoss",
            severity: Severity::Critical,
            evidence: "行情上游连接状态 = 断开".into(),
            suggestion: "检查行情源凭据/网络出口与重连日志（Loki: job=real-time-feed）".into(),
            root_cause_of: None,
        });
    }

    if let Some(rate) = s.message_rate {
        if rate < THRESHOLD_MESSAGE_RATE {
            out.push(Finding {
                service: s.service.clone(),
                rule: "RealtimeFeedMessageGap",
                severity: Severity::Warning,
                evidence: format!(
                    "消息速率 {:.2} msg/s < {:.0} msg/s",
                    rate, THRESHOLD_MESSAGE_RATE
                ),
                suggestion: "确认上游是否停推或采集管道积压（消费延迟 = 生产速率 - 消费速率）"
                    .into(),
                root_cause_of: None,
            });
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc(service: &str) -> ServiceMetrics {
        ServiceMetrics::new(service)
    }

    #[test]
    fn healthy_snapshot_yields_empty_diagnosis() {
        let snap = Snapshot {
            services: vec![
                ServiceMetrics {
                    up: Some(true),
                    error_rate: Some(0.0),
                    p95_latency_ms: Some(120.0),
                    memory_used_ratio: Some(0.4),
                    unreleased_allocs: Some(3),
                    feed_connected: Some(true),
                    message_rate: Some(42.0),
                    ..svc("api-gateway")
                },
                ServiceMetrics {
                    up: Some(true),
                    error_rate: Some(0.01),
                    p95_latency_ms: Some(800.0),
                    ..svc("data-engine")
                },
            ],
        };
        let d = diagnose(&snap);
        assert!(d.is_healthy(), "健康快照不应有发现: {:?}", d.findings);
        assert_eq!(d.level(), Severity::Info);
        assert!(!d.has_critical());
    }

    #[test]
    fn missing_metrics_do_not_false_alarm() {
        // 全 None（指标未接）：缺数据 ≠ 故障，零发现
        let d = diagnose(&Snapshot {
            services: vec![svc("api-gateway"), svc("collector")],
        });
        assert!(d.is_healthy(), "缺数据不应误报: {:?}", d.findings);
    }

    #[test]
    fn service_down_is_critical_with_actionable_suggestion() {
        let d = diagnose(&Snapshot {
            services: vec![ServiceMetrics {
                up: Some(false),
                ..svc("data-engine")
            }],
        });
        assert_eq!(d.findings.len(), 1);
        let f = &d.findings[0];
        assert_eq!(f.rule, "ServiceDown");
        assert_eq!(f.severity, Severity::Critical);
        assert!(d.has_critical());
        assert!(f.suggestion.contains("docker logs"));
        assert!(f.evidence.contains("up == 0"));
    }

    #[test]
    fn thresholds_are_exclusive_boundaries() {
        // 恰好等于阈值不触发（> 语义），超过才触发
        let at = diagnose(&Snapshot {
            services: vec![ServiceMetrics {
                error_rate: Some(THRESHOLD_ERROR_RATE),
                ..svc("api-gateway")
            }],
        });
        assert!(at.is_healthy(), "等于阈值不应触发: {:?}", at.findings);

        let over = diagnose(&Snapshot {
            services: vec![ServiceMetrics {
                error_rate: Some(THRESHOLD_ERROR_RATE + 0.001),
                ..svc("api-gateway")
            }],
        });
        assert_eq!(over.findings.len(), 1);
        assert_eq!(over.findings[0].rule, "GatewayErrorRateHigh");
        assert!(over.findings[0].evidence.contains("5.10%"));
    }

    #[test]
    fn realtime_rules_trigger_independently() {
        let d = diagnose(&Snapshot {
            services: vec![ServiceMetrics {
                feed_connected: Some(false),
                message_rate: Some(0.2),
                ..svc("real-time-feed")
            }],
        });
        // 连接断（Critical）+ 消息断流（Warning）
        assert_eq!(d.findings.len(), 2);
        assert_eq!(d.level(), Severity::Critical);
        // 排序：Critical 在前
        assert_eq!(d.findings[0].rule, "RealtimeFeedConnectionLoss");
    }

    #[test]
    fn gateway_error_rate_gets_root_cause_linked_to_down_upstream() {
        let d = diagnose(&Snapshot {
            services: vec![
                ServiceMetrics {
                    up: Some(true),
                    error_rate: Some(0.20),
                    ..svc("api-gateway")
                },
                ServiceMetrics {
                    up: Some(false),
                    ..svc("data-engine")
                },
            ],
        });
        assert_eq!(d.findings.len(), 2);
        // 排序：Critical（data-engine 宕机）在前，Warning（网关错误率）在后
        assert_eq!(d.findings[0].rule, "ServiceDown");
        let gw = &d.findings[1];
        assert_eq!(gw.rule, "GatewayErrorRateHigh");
        assert_eq!(
            gw.root_cause_of.as_deref(),
            Some("ServiceDown:data-engine"),
            "错误率升高应关联宕机上游为根因"
        );
        assert!(gw.suggestion.contains("data-engine"));
        assert!(gw.suggestion.contains("优先恢复"));
    }

    #[test]
    fn message_gap_without_disconnected_feed_has_no_root_cause() {
        // 连接正常但速率低：不臆造根因关联
        let d = diagnose(&Snapshot {
            services: vec![ServiceMetrics {
                feed_connected: Some(true),
                message_rate: Some(0.3),
                ..svc("real-time-feed")
            }],
        });
        assert_eq!(d.findings.len(), 1);
        assert_eq!(d.findings[0].rule, "RealtimeFeedMessageGap");
        assert!(d.findings[0].root_cause_of.is_none());
    }

    #[test]
    fn all_rules_have_threshold_constants_aligned_with_alert_rules() {
        // 阈值与 config/alpha-alerts.yml 同源口径锁定（改一处须同步另一处）
        assert_eq!(THRESHOLD_ERROR_RATE, 0.05); // GatewayErrorRateHigh > 0.05
        assert_eq!(THRESHOLD_P95_LATENCY_MS, 2000.0); // QueryLatencyHigh > 2s
        assert_eq!(THRESHOLD_MEMORY_RATIO, 0.85); // MemoryPressure > 85
        assert_eq!(THRESHOLD_UNRELEASED_ALLOCS, 100); // AllocTrackerUnreleased > 100
        assert_eq!(THRESHOLD_MESSAGE_RATE, 1.0); // MessageGap < 1 msg/s
    }
}
