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
mod engine;
mod rules;
mod schema;
#[cfg(test)]
mod tests;

pub use engine::DiagnosisEngine;
pub use schema::{
    ActionPriority, ActionType, DiagnosisReport, DiagnosisRequest, DiagnosisRule, LogEvidence,
    MatchedRule, MetricAnomalyType, MetricEvidence, RootCauseAssessment, RootCauseCategory,
    SuggestedAction, TraceEvidence,
};
