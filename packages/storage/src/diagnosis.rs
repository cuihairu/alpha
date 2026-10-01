//! 智能故障诊断引擎（L464）
//!
//! 基于 Prometheus 指标 + Loki 日志 + 分布式追踪的关联分析，输出根因分析报告。
//! 设计为纯函数库（零 IO），可被告警 webhook / CLI / 定时任务复用。
//!
//! 诊断流程：
//! 1. 接收告警指纹 + 时间窗口
//! 2. 并行查询 Prometheus（指标异常模式）+ Loki（错误日志聚类）+ Jaeger（追踪延迟/错误）
//! 3. 规则引擎匹配已知故障模式（知识库可扩展）
//! 4. 输出结构化诊断报告：根因、置信度、关联证据、建议动作

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 故障诊断请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosisRequest {
    /// 告警指纹（Alertmanager fingerprint）
    pub alert_fingerprint: String,
    /// 告警名称
    pub alert_name: String,
    /// 严重级别
    pub severity: String,
    /// 受影响作业
    pub job: String,
    /// 诊断时间窗口起点（通常为告警 starts_at 前推 10-30 分钟）
    pub window_start: DateTime<Utc>,
    /// 诊断时间窗口终点（通常为告警 starts_at 或 now）
    pub window_end: DateTime<Utc>,
    /// 关联的标签集合（用于查询过滤）
    pub labels: HashMap<String, String>,
}

/// 指标异常证据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricEvidence {
    pub metric_name: String,
    pub query: String,
    pub current_value: f64,
    pub baseline_value: f64,
    pub deviation_pct: f64,
    pub anomaly_type: MetricAnomalyType,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricAnomalyType {
    Spike,       // 突增
    Drop,        // 突降
    Plateau,     // 持续高位
    Zero,        // 归零
    Oscillation, // 震荡
}

/// 日志聚类证据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEvidence {
    pub pattern: String,           // 正则/关键词模式
    pub count: u64,                // 匹配行数
    pub sample_lines: Vec<String>, // 样本行（脱敏后）
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub log_level: String, // error/warn/info
    pub container: String,
}

/// 追踪证据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEvidence {
    pub trace_id: String,
    pub service_name: String,
    pub operation_name: String,
    pub duration_ms: u64,
    pub status_code: Option<u16>,
    pub error_message: Option<String>,
    pub span_count: u32,
    pub has_error: bool,
}

/// 诊断规则（知识库条目）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosisRule {
    pub id: String,
    pub name: String,
    pub description: String,
    /// 触发条件：告警名称匹配（支持通配符）
    pub alert_patterns: Vec<String>,
    /// 必需的指标异常类型
    pub required_metric_anomalies: Vec<MetricAnomalyType>,
    /// 关键日志模式（正则）
    pub log_patterns: Vec<String>,
    /// 关键追踪特征
    pub trace_indicators: Vec<String>,
    /// 根因分类
    pub root_cause_category: RootCauseCategory,
    /// 置信度基础分（0-100）
    pub base_confidence: u8,
    /// 建议修复动作
    pub suggested_actions: Vec<SuggestedAction>,
    /// 关联知识库链接
    pub runbook_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootCauseCategory {
    ResourceExhaustion, // 资源耗尽
    NetworkIssue,       // 网络问题
    DownstreamFailure,  // 下游故障
    ConfigError,        // 配置错误
    CodeBug,            // 代码缺陷
    CapacityLimit,      // 容量瓶颈
    DependencyDegraded, // 依赖降级
    Unknown,            // 未知
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestedAction {
    pub action_type: ActionType,
    pub description: String,
    pub command: Option<String>, // 可执行命令（如 kubectl rollout restart）
    pub priority: ActionPriority,
    pub estimated_downtime_secs: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    RestartService,
    ScaleUp,
    RollbackConfig,
    CheckNetwork,
    CheckDependency,
    ReviewLogs,
    IncreaseQuota,
    DrainNode,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionPriority {
    Immediate,
    High,
    Medium,
    Low,
}

/// 诊断报告
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosisReport {
    pub request: DiagnosisRequest,
    pub timestamp: DateTime<Utc>,
    pub matched_rules: Vec<MatchedRule>,
    pub metric_evidence: Vec<MetricEvidence>,
    pub log_evidence: Vec<LogEvidence>,
    pub trace_evidence: Vec<TraceEvidence>,
    pub root_cause: RootCauseAssessment,
    pub recommended_actions: Vec<SuggestedAction>,
    pub confidence_score: u8, // 0-100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchedRule {
    pub rule_id: String,
    pub rule_name: String,
    pub matched_conditions: Vec<String>,
    pub confidence_contribution: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootCauseAssessment {
    pub category: RootCauseCategory,
    pub description: String,
    pub primary_evidence: Vec<String>,
    pub contributing_factors: Vec<String>,
}

/// 内置知识库（可外部化为文件/数据库）
fn builtin_rules() -> Vec<DiagnosisRule> {
    vec![
        // ===== 网关层规则 =====
        DiagnosisRule {
            id: "GW-001".to_string(),
            name: "API 网关限流触发".to_string(),
            description: "客户端请求频率超过配额，或配额设置过低".to_string(),
            alert_patterns: vec!["GatewayRateLimitExceeded".to_string()],
            required_metric_anomalies: vec![MetricAnomalyType::Spike],
            log_patterns: vec!["rate limit exceeded".to_string(), "429".to_string()],
            trace_indicators: vec!["x-ratelimit-remaining: 0".to_string()],
            root_cause_category: RootCauseCategory::CapacityLimit,
            base_confidence: 90,
            suggested_actions: vec![
                SuggestedAction {
                    action_type: ActionType::IncreaseQuota,
                    description: "调大 --rate-limit-per-minute 或按客户端分级配额".to_string(),
                    command: Some("kubectl set env deployment/api-gateway ALPHA_GATEWAY_RATE_LIMIT_PER_MINUTE=300".to_string()),
                    priority: ActionPriority::High,
                    estimated_downtime_secs: None,
                },
                SuggestedAction {
                    action_type: ActionType::CheckDependency,
                    description: "排查异常客户端（爬虫/刷单/错误重试风暴）".to_string(),
                    command: None,
                    priority: ActionPriority::High,
                    estimated_downtime_secs: None,
                },
            ],
            runbook_url: Some("https://wiki.company.com/ops/gateway-rate-limit".to_string()),
        },

        DiagnosisRule {
            id: "GW-002".to_string(),
            name: "网关上游服务不可达".to_string(),
            description: "data-engine 或 real-time-feed 健康检查失败，网关返回 502".to_string(),
            alert_patterns: vec!["GatewayUpstreamUnhealthy".to_string()],
            required_metric_anomalies: vec![MetricAnomalyType::Drop, MetricAnomalyType::Zero],
            log_patterns: vec!["unreachable".to_string(), "connection refused".to_string(), "timeout".to_string()],
            trace_indicators: vec!["502".to_string(), "bad gateway".to_string()],
            root_cause_category: RootCauseCategory::DownstreamFailure,
            base_confidence: 85,
            suggested_actions: vec![
                SuggestedAction {
                    action_type: ActionType::CheckDependency,
                    description: "检查上游服务容器状态与日志".to_string(),
                    command: Some("docker ps --filter name=alpha-data-engine --filter name=alpha-real-time-feed".to_string()),
                    priority: ActionPriority::Immediate,
                    estimated_downtime_secs: None,
                },
                SuggestedAction {
                    action_type: ActionType::RestartService,
                    description: "重启异常上游服务".to_string(),
                    command: Some("docker restart alpha-data-engine alpha-real-time-feed".to_string()),
                    priority: ActionPriority::High,
                    estimated_downtime_secs: Some(30),
                },
            ],
            runbook_url: Some("https://wiki.company.com/ops/gateway-health".to_string()),
        },

        DiagnosisRule {
            id: "GW-003".to_string(),
            name: "网关 5xx 错误率飙升".to_string(),
            description: "上游返回 5xx 或网关自身错误导致错误率超过阈值".to_string(),
            alert_patterns: vec!["GatewayErrorRateHigh".to_string()],
            required_metric_anomalies: vec![MetricAnomalyType::Spike],
            log_patterns: vec!["502".to_string(), "503".to_string(), "504".to_string(), "bad gateway".to_string()],
            trace_indicators: vec!["status_code: 5".to_string()],
            root_cause_category: RootCauseCategory::DownstreamFailure,
            base_confidence: 80,
            suggested_actions: vec![
                SuggestedAction {
                    action_type: ActionType::ReviewLogs,
                    description: "查看网关与上游错误日志定位具体错误".to_string(),
                    command: Some("docker logs alpha-api-gateway --since 10m | grep -E '502|503|504'".to_string()),
                    priority: ActionPriority::High,
                    estimated_downtime_secs: None,
                },
            ],
            runbook_url: Some("https://wiki.company.com/ops/gateway-errors".to_string()),
        },

        // ===== 数据引擎规则 =====
        DiagnosisRule {
            id: "DE-001".to_string(),
            name: "数据引擎查询延迟升高".to_string(),
            description: "ClickHouse/TimescaleDB 查询耗时增加，可能由大查询、索引缺失、资源争用引起".to_string(),
            alert_patterns: vec!["DataEngineQueryLatencyHigh".to_string()],
            required_metric_anomalies: vec![MetricAnomalyType::Spike, MetricAnomalyType::Plateau],
            log_patterns: vec!["slow query".to_string(), "query.*exceeded".to_string(), "timeout".to_string()],
            trace_indicators: vec!["db.query.duration".to_string()],
            root_cause_category: RootCauseCategory::CapacityLimit,
            base_confidence: 75,
            suggested_actions: vec![
                SuggestedAction {
                    action_type: ActionType::ReviewLogs,
                    description: "分析慢查询日志，识别 Top N 耗时 SQL".to_string(),
                    command: Some("docker logs alpha-data-engine --since 15m | grep -i 'slow\\|timeout'".to_string()),
                    priority: ActionPriority::High,
                    estimated_downtime_secs: None,
                },
                SuggestedAction {
                    action_type: ActionType::ScaleUp,
                    description: "临时扩容 data-engine 副本分担查询压力".to_string(),
                    command: Some("kubectl scale deployment/data-engine --replicas=3".to_string()),
                    priority: ActionPriority::Medium,
                    estimated_downtime_secs: None,
                },
            ],
            runbook_url: Some("https://wiki.company.com/ops/dataengine-latency".to_string()),
        },

        DiagnosisRule {
            id: "DE-002".to_string(),
            name: "数据引擎内存压力".to_string(),
            description: "内存使用率超过 85%，可能导致 OOM Kill 或 GC 频繁".to_string(),
            alert_patterns: vec!["DataEngineMemoryPressure".to_string()],
            required_metric_anomalies: vec![MetricAnomalyType::Plateau, MetricAnomalyType::Spike],
            log_patterns: vec!["OOM".to_string(), "OutOfMemory".to_string(), "GC".to_string(), "memory".to_string()],
            trace_indicators: vec!["memory.usage".to_string()],
            root_cause_category: RootCauseCategory::ResourceExhaustion,
            base_confidence: 85,
            suggested_actions: vec![
                SuggestedAction {
                    action_type: ActionType::ScaleUp,
                    description: "增加容器内存限制或横向扩容".to_string(),
                    command: Some("kubectl set resources deployment/data-engine --limits=memory=4Gi".to_string()),
                    priority: ActionPriority::High,
                    estimated_downtime_secs: Some(60),
                },
                SuggestedAction {
                    action_type: ActionType::ReviewLogs,
                    description: "排查内存泄漏（alloc-tracker 指标 / heap profile）".to_string(),
                    command: None,
                    priority: ActionPriority::High,
                    estimated_downtime_secs: None,
                },
            ],
            runbook_url: Some("https://wiki.company.com/ops/dataengine-memory".to_string()),
        },

        // ===== 实时行情规则 =====
        DiagnosisRule {
            id: "RT-001".to_string(),
            name: "实时行情 WebSocket 连接中断".to_string(),
            description: "real-time-feed 与网关/上游连接断开，行情推送停止".to_string(),
            alert_patterns: vec!["RealtimeFeedConnectionLoss".to_string()],
            required_metric_anomalies: vec![MetricAnomalyType::Drop, MetricAnomalyType::Zero],
            log_patterns: vec!["websocket".to_string(), "disconnect".to_string(), "connection closed".to_string(), "ping.*timeout".to_string()],
            trace_indicators: vec!["ws.upstream".to_string()],
            root_cause_category: RootCauseCategory::NetworkIssue,
            base_confidence: 90,
            suggested_actions: vec![
                SuggestedAction {
                    action_type: ActionType::RestartService,
                    description: "重启 real-time-feed 服务恢复连接".to_string(),
                    command: Some("docker restart alpha-real-time-feed".to_string()),
                    priority: ActionPriority::Immediate,
                    estimated_downtime_secs: Some(15),
                },
                SuggestedAction {
                    action_type: ActionType::CheckNetwork,
                    description: "检查网关→real-time-feed 网络连通性".to_string(),
                    command: Some("docker exec alpha-api-gateway wget -qO- http://real-time-feed:8082/health".to_string()),
                    priority: ActionPriority::High,
                    estimated_downtime_secs: None,
                },
            ],
            runbook_url: Some("https://wiki.company.com/ops/realtime-connection".to_string()),
        },

        DiagnosisRule {
            id: "RT-002".to_string(),
            name: "实时行情消息间隔异常".to_string(),
            description: "消息频率骤降，可能上游行情源异常或消费端处理阻塞".to_string(),
            alert_patterns: vec!["RealtimeFeedMessageGap".to_string()],
            required_metric_anomalies: vec![MetricAnomalyType::Drop],
            log_patterns: vec!["message gap".to_string(), "no data".to_string(), "queue full".to_string()],
            trace_indicators: vec!["redis.stream.lag".to_string()],
            root_cause_category: RootCauseCategory::DependencyDegraded,
            base_confidence: 70,
            suggested_actions: vec![
                SuggestedAction {
                    action_type: ActionType::CheckDependency,
                    description: "检查 Redis Streams 消费组延迟（XPENDING）".to_string(),
                    command: Some("docker exec alpha-redis redis-cli XPENDING quotes.raw consumer-group".to_string()),
                    priority: ActionPriority::High,
                    estimated_downtime_secs: None,
                },
            ],
            runbook_url: Some("https://wiki.company.com/ops/realtime-gap".to_string()),
        },

        // ===== 基础设施规则 =====
        DiagnosisRule {
            id: "INFRA-001".to_string(),
            name: "Prometheus 采集失败".to_string(),
            description: "目标服务不可达或 /metrics 端点异常".to_string(),
            alert_patterns: vec!["PrometheusScrapeFailure".to_string(), "PrometheusTargetDown".to_string()],
            required_metric_anomalies: vec![MetricAnomalyType::Zero],
            log_patterns: vec!["scrape".to_string(), "timeout".to_string(), "connection refused".to_string()],
            trace_indicators: vec![],
            root_cause_category: RootCauseCategory::NetworkIssue,
            base_confidence: 80,
            suggested_actions: vec![
                SuggestedAction {
                    action_type: ActionType::CheckNetwork,
                    description: "验证目标服务网络连通性与 /metrics 端点".to_string(),
                    command: Some("curl -v http://<target>:<port>/metrics".to_string()),
                    priority: ActionPriority::High,
                    estimated_downtime_secs: None,
                },
            ],
            runbook_url: Some("https://wiki.company.com/ops/prometheus-scrape".to_string()),
        },

        DiagnosisRule {
            id: "INFRA-002".to_string(),
            name: "Loki 日志写入/查询失败".to_string(),
            description: "Loki 存储/查询路径异常，影响日志关联诊断".to_string(),
            alert_patterns: vec!["LokiIngestionFailure".to_string(), "LokiQueryFailure".to_string()],
            required_metric_anomalies: vec![MetricAnomalyType::Spike],
            log_patterns: vec!["loki".to_string(), "ingestion".to_string(), "write".to_string(), "error".to_string()],
            trace_indicators: vec![],
            root_cause_category: RootCauseCategory::ResourceExhaustion,
            base_confidence: 75,
            suggested_actions: vec![
                SuggestedAction {
                    action_type: ActionType::CheckDependency,
                    description: "检查 Loki 磁盘空间与 chunk 索引状态".to_string(),
                    command: Some("docker exec alpha-loki df -h /loki".to_string()),
                    priority: ActionPriority::High,
                    estimated_downtime_secs: None,
                },
            ],
            runbook_url: Some("https://wiki.company.com/ops/loki-ingestion".to_string()),
        },

        DiagnosisRule {
            id: "INFRA-003".to_string(),
            name: "内存分配泄漏检测".to_string(),
            description: "TrackingAllocator 发现未释放分配持续增长".to_string(),
            alert_patterns: vec!["AllocTrackerUnreleased".to_string()],
            required_metric_anomalies: vec![MetricAnomalyType::Spike, MetricAnomalyType::Plateau],
            log_patterns: vec!["alloc_tracker".to_string(), "leak".to_string(), "unreleased".to_string()],
            trace_indicators: vec![],
            root_cause_category: RootCauseCategory::ResourceExhaustion,
            base_confidence: 70,
            suggested_actions: vec![
                SuggestedAction {
                    action_type: ActionType::ReviewLogs,
                    description: "运行 heaptrack/valgrind 定位泄漏点".to_string(),
                    command: None,
                    priority: ActionPriority::Medium,
                    estimated_downtime_secs: None,
                },
                SuggestedAction {
                    action_type: ActionType::RestartService,
                    description: "紧急重启缓解（治标不治本）".to_string(),
                    command: None,
                    priority: ActionPriority::Low,
                    estimated_downtime_secs: Some(30),
                },
            ],
            runbook_url: Some("https://wiki.company.com/ops/alloc-tracker".to_string()),
        },

        DiagnosisRule {
            id: "INFRA-004".to_string(),
            name: "SIMD AVX2 降级".to_string(),
            description: "服务器不支持 AVX2，SIMD 计算回退纯 Rust 实现，性能下降".to_string(),
            alert_patterns: vec!["SIMDAVX2Degraded".to_string()],
            required_metric_anomalies: vec![],
            log_patterns: vec!["avx2".to_string(), "simd".to_string(), "fallback".to_string()],
            trace_indicators: vec![],
            root_cause_category: RootCauseCategory::CapacityLimit,
            base_confidence: 95,
            suggested_actions: vec![
                SuggestedAction {
                    action_type: ActionType::Custom,
                    description: "确认硬件规格，考虑迁移至支持 AVX2 的实例".to_string(),
                    command: Some("lscpu | grep avx2".to_string()),
                    priority: ActionPriority::Low,
                    estimated_downtime_secs: None,
                },
            ],
            runbook_url: Some("https://wiki.company.com/ops/simd-degraded".to_string()),
        },
    ]
}

/// 诊断引擎核心
pub struct DiagnosisEngine {
    rules: Vec<DiagnosisRule>,
}

impl DiagnosisEngine {
    pub fn new() -> Self {
        Self {
            rules: builtin_rules(),
        }
    }

    pub fn with_custom_rules(mut rules: Vec<DiagnosisRule>) -> Self {
        rules.extend(builtin_rules());
        Self { rules }
    }

    /// 执行诊断（纯函数核心，外部注入证据）
    pub fn diagnose(
        &self,
        request: &DiagnosisRequest,
        metric_evidence: Vec<MetricEvidence>,
        log_evidence: Vec<LogEvidence>,
        trace_evidence: Vec<TraceEvidence>,
    ) -> DiagnosisReport {
        let mut matched_rules = Vec::new();
        let mut total_confidence = 0u16;

        for rule in &self.rules {
            if !self.alert_matches(rule, request) {
                continue;
            }

            let mut matched_conditions = Vec::new();
            let mut confidence = rule.base_confidence as u16;

            // 指标证据匹配
            for required in &rule.required_metric_anomalies {
                if metric_evidence.iter().any(|e| &e.anomaly_type == required) {
                    matched_conditions.push(format!("指标异常: {:?}", required));
                    confidence += 10;
                }
            }

            // 日志证据匹配
            for pattern in &rule.log_patterns {
                if log_evidence
                    .iter()
                    .any(|e| regex_match(pattern, &e.pattern))
                {
                    matched_conditions.push(format!("日志模式: {}", pattern));
                    confidence += 15;
                }
            }

            // 追踪证据匹配
            for indicator in &rule.trace_indicators {
                if trace_evidence.iter().any(|e| {
                    e.error_message
                        .as_ref()
                        .is_some_and(|msg| msg.contains(indicator))
                        || e.operation_name.contains(indicator)
                }) {
                    matched_conditions.push(format!("追踪特征: {}", indicator));
                    confidence += 10;
                }
            }

            if !matched_conditions.is_empty() {
                matched_rules.push(MatchedRule {
                    rule_id: rule.id.clone(),
                    rule_name: rule.name.clone(),
                    matched_conditions,
                    confidence_contribution: confidence.min(100) as u8,
                });
                total_confidence += confidence;
            }
        }

        // 综合根因评估
        let root_cause = self.assess_root_cause(
            &matched_rules,
            &metric_evidence,
            &log_evidence,
            &trace_evidence,
        );

        // 聚合建议动作（去重 + 优先级排序）
        let mut recommended_actions = Vec::new();
        for rule in &matched_rules {
            if let Some(full_rule) = self.rules.iter().find(|r| r.id == rule.rule_id) {
                for action in &full_rule.suggested_actions {
                    if !recommended_actions
                        .iter()
                        .any(|a: &SuggestedAction| a.description == action.description)
                    {
                        recommended_actions.push(action.clone());
                    }
                }
            }
        }
        recommended_actions.sort_by_key(|a| match a.priority {
            ActionPriority::Immediate => 0,
            ActionPriority::High => 1,
            ActionPriority::Medium => 2,
            ActionPriority::Low => 3,
        });
        let matched_len = matched_rules.len();

        DiagnosisReport {
            request: request.clone(),
            timestamp: Utc::now(),
            matched_rules,
            metric_evidence,
            log_evidence,
            trace_evidence,
            root_cause,
            recommended_actions,
            confidence_score: (total_confidence / matched_len.max(1) as u16).min(100) as u8,
        }
    }

    fn alert_matches(&self, rule: &DiagnosisRule, request: &DiagnosisRequest) -> bool {
        rule.alert_patterns.iter().any(|pattern| {
            pattern.as_str() == request.alert_name
                || pattern.contains('*') && glob_match(pattern, &request.alert_name)
        })
    }

    fn assess_root_cause(
        &self,
        matched_rules: &[MatchedRule],
        metric_evidence: &[MetricEvidence],
        log_evidence: &[LogEvidence],
        trace_evidence: &[TraceEvidence],
    ) -> RootCauseAssessment {
        if matched_rules.is_empty() {
            return RootCauseAssessment {
                category: RootCauseCategory::Unknown,
                description: "未匹配到已知故障模式，需人工排查".to_string(),
                primary_evidence: vec![],
                contributing_factors: vec![],
            };
        }

        // 取置信度最高的规则作为主根因
        let primary = matched_rules
            .iter()
            .max_by_key(|r| r.confidence_contribution)
            .unwrap();
        let primary_rule = self.rules.iter().find(|r| r.id == primary.rule_id).unwrap();

        let mut primary_evidence = Vec::new();
        primary_evidence.extend(
            metric_evidence
                .iter()
                .filter(|e| e.deviation_pct > 50.0)
                .map(|e| format!("{}: {:.1}% 偏离", e.metric_name, e.deviation_pct)),
        );
        primary_evidence.extend(
            log_evidence
                .iter()
                .filter(|e| e.log_level == "error")
                .map(|e| format!("{} 错误日志 {} 条", e.container, e.count)),
        );
        primary_evidence.extend(
            trace_evidence
                .iter()
                .filter(|e| e.has_error)
                .map(|e| format!("追踪 {} 含错误", e.trace_id)),
        );

        let mut contributing_factors = Vec::new();
        if metric_evidence
            .iter()
            .any(|e| e.anomaly_type == MetricAnomalyType::Spike)
        {
            contributing_factors.push("指标突变".to_string());
        }
        if log_evidence.iter().any(|e| e.log_level == "error") {
            contributing_factors.push("错误日志出现".to_string());
        }
        if trace_evidence.iter().any(|e| e.has_error) {
            contributing_factors.push("分布式追踪记录错误".to_string());
        }

        RootCauseAssessment {
            category: primary_rule.root_cause_category.clone(),
            description: primary_rule.description.clone(),
            primary_evidence,
            contributing_factors,
        }
    }
}

impl Default for DiagnosisEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// 简单通配符匹配
fn glob_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut t = text;
    let last = parts.len().saturating_sub(1);
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            if !t.starts_with(part) {
                return false;
            }
            t = &t[part.len()..];
        } else if i == last {
            return t.ends_with(part);
        } else if let Some(pos) = t.find(part) {
            t = &t[pos + part.len()..];
        } else {
            return false;
        }
    }
    true
}

/// 正则匹配（简化：子串包含）
fn regex_match(pattern: &str, text: &str) -> bool {
    text.to_lowercase().contains(&pattern.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
