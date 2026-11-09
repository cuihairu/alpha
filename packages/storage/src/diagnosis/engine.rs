use super::rules::builtin_rules;
use super::schema::*;
use chrono::Utc;

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

            if matched_conditions.is_empty() {
                // 告警名归属本身是一等信号：无证据注入（规则级诊断，如
                // alert-webhook 的告警富化）时按规则 base 置信度入册——
                // 证据只加不减，缺失不把已知模式打成「未匹配」
                matched_conditions.push(format!("告警名称: {}", request.alert_name));
            }
            matched_rules.push(MatchedRule {
                rule_id: rule.id.clone(),
                rule_name: rule.name.clone(),
                matched_conditions,
                confidence_contribution: confidence.min(100) as u8,
            });
            total_confidence += confidence;
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
pub(crate) fn glob_match(pattern: &str, text: &str) -> bool {
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
