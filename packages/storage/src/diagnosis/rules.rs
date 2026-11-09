use super::schema::*;

/// 内置知识库（可外部化为文件/数据库）
pub(crate) fn builtin_rules() -> Vec<DiagnosisRule> {
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

        DiagnosisRule {
            id: "GW-004".to_string(),
            name: "API 网关防护栏触发".to_string(),
            description: "护栏（UA 拒绝/burst 令牌桶/路径扫描检测）拒绝率>0——爬虫或滥用流量特征，按 mode 标签甄别".to_string(),
            alert_patterns: vec!["GatewayShieldTriggered".to_string()],
            required_metric_anomalies: vec![MetricAnomalyType::Spike],
            log_patterns: vec!["shield".to_string(), "burst".to_string(), "scan".to_string()],
            trace_indicators: vec![],
            root_cause_category: RootCauseCategory::CapacityLimit,
            base_confidence: 75,
            suggested_actions: vec![
                SuggestedAction {
                    action_type: ActionType::ReviewLogs,
                    description: "按 mode 甄别攻击面（bot_denied=伪造 UA、burst_denied=短洪峰、scan_denied=广撒网探测），结合访问日志提取来源分布".to_string(),
                    command: Some("curl -s http://<gateway>:9080/metrics | grep alpha_gateway_shield_total".to_string()),
                    priority: ActionPriority::High,
                    estimated_downtime_secs: None,
                },
                SuggestedAction {
                    action_type: ActionType::Custom,
                    description: "确认误伤面：护栏只标记不封禁（alerting §6），误伤自有客户端时调 ALPHA_GATEWAY_BURST_CAPACITY / SCAN_MAX_DISTINCT".to_string(),
                    command: None,
                    priority: ActionPriority::Medium,
                    estimated_downtime_secs: None,
                },
            ],
            runbook_url: Some("docs/alerting-and-diagnosis.md".to_string()),
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

        // ===== 采集层规则（L504 源健康面，告警名经 alert-webhook 富化）=====
        DiagnosisRule {
            id: "COL-001".to_string(),
            name: "采集源降级（连败 1..=3）".to_string(),
            description: "数据源连续失败但未断绝，gauge alpha_collector_source_health=2；调度仍按 cron 重试，失败原因在 /sources/health 的 last_error".to_string(),
            alert_patterns: vec!["CollectorSourceDegraded".to_string()],
            required_metric_anomalies: vec![MetricAnomalyType::Drop],
            log_patterns: vec!["task failed".to_string(), "execute".to_string(), "error".to_string()],
            trace_indicators: vec![],
            root_cause_category: RootCauseCategory::DependencyDegraded,
            base_confidence: 70,
            suggested_actions: vec![
                SuggestedAction {
                    action_type: ActionType::CheckDependency,
                    description: "查源健康台账定位最近失败原因与时间（三态/连败计数/last_error 同屏）".to_string(),
                    command: Some("curl -s http://localhost:8083/sources/health".to_string()),
                    priority: ActionPriority::High,
                    estimated_downtime_secs: None,
                },
                SuggestedAction {
                    action_type: ActionType::ReviewLogs,
                    description: "查原始响应归档（开启 ALPHA_COLLECTOR_RAW_ARCHIVE_URL 时 raw/{日期}/{task_id}/）甄别内容改版还是网络抖动".to_string(),
                    command: Some("docker logs alpha-collector --since 30m".to_string()),
                    priority: ActionPriority::Medium,
                    estimated_downtime_secs: None,
                },
            ],
            runbook_url: Some("docs/alerting-and-diagnosis.md".to_string()),
        },

        DiagnosisRule {
            id: "COL-002".to_string(),
            name: "采集源中断（连败 >3）".to_string(),
            description: "数据源持续失败超过阈值，gauge=3，产出已中断；常见根因：源站宕机/被限频封禁/页面改版致解析器失效".to_string(),
            alert_patterns: vec!["CollectorSourceDown".to_string()],
            required_metric_anomalies: vec![MetricAnomalyType::Zero],
            log_patterns: vec!["task failed".to_string(), "timeout".to_string(), "connection refused".to_string()],
            trace_indicators: vec![],
            root_cause_category: RootCauseCategory::DownstreamFailure,
            base_confidence: 85,
            suggested_actions: vec![
                SuggestedAction {
                    action_type: ActionType::CheckNetwork,
                    description: "直接探数据源 URL 甄别可达性（连通但解析失败=改版；拒绝/超时=封禁或宕机）".to_string(),
                    command: Some("curl -s -o /dev/null -w '%{http_code}' <数据源URL>".to_string()),
                    priority: ActionPriority::Immediate,
                    estimated_downtime_secs: None,
                },
                SuggestedAction {
                    action_type: ActionType::Custom,
                    description: "改版则修任务模板解析器；封禁则调模板重试参数或换源；确认短期不恢复可 POST /tasks/:id/cancel 停调度止血".to_string(),
                    command: None,
                    priority: ActionPriority::High,
                    estimated_downtime_secs: None,
                },
            ],
            runbook_url: Some("docs/alerting-and-diagnosis.md".to_string()),
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
