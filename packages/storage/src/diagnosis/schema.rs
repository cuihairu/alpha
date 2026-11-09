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
