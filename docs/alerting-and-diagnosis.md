# 实时告警与智能故障诊断

口径：**工程项**——Prometheus 告警规则 + Alertmanager 路由 +
Webhook 多渠道分发 + 诊断引擎纯函数库。Jaeger OTLP 全量 span
导出仍登记不实现（见 docs/distributed-tracing.md 登记边界）。

## 1. 链路

```
四服务 /metrics → Prometheus（rules 评估）→ Alertmanager（分组/抑制/路由）
    → alert-webhook :8080（/alerts、/alerts/critical）
    → 钉钉/企微/Slack/PagerDuty（env 注入缺失则仅日志）+ 结构化日志 → Loki
```

## 2. 规则（config/alpha-alerts.yml，11 条）

| 告警 | 数据源（埋点位置） |
|---|---|
| GatewayRateLimitExceeded | gateway `alpha_gateway_rate_limit_total{mode="denied"}`（L446） |
| GatewayShieldTriggered | gateway `alpha_gateway_shield_total{mode="bot_denied\|burst_denied\|scan_denied"}`（L486，cae1f1c） |
| GatewayUpstreamUnhealthy | gateway `alpha_gateway_service_health{upstream=…}`（L459/L464） |
| GatewayErrorRateHigh | gateway `alpha_gateway_requests_total{method,status}`（L459） |
| DataEngineQueryLatencyHigh | data-engine `alpha_dataengine_query_duration_seconds` histogram（L464） |
| DataEngineMemoryPressure | data-engine `alpha_dataengine_memory_bytes{mode}` gauge，Linux /proc（L464） |
| RealtimeFeedConnectionLoss | realtime `alpha_realtime_feed_connected` gauge（L464） |
| RealtimeFeedMessageGap | realtime `alpha_realtime_messages_total` counter（L464） |
| CollectorSourceDegraded | collector `alpha_collector_source_health{task}` gauge 2 档（L504） |
| CollectorSourceDown | collector `alpha_collector_source_health{task}` gauge 3 档（L504） |
| PrometheusTargetDown | Prometheus 自带 `up == 0`（无需业务埋点） |

硬教训（已修，勿回退）：

1. 健康 gauge 按 **`upstream`** 标签分桶——Prometheus 抓取会用 target 的
   `job` 标签覆盖指标自报的同名标签（`honor_labels` 默认 false），规则写
   `{job="data-engine"}` 永远匹配不上（见网关 `probe_service` 注释）。
2. `memory_bytes{mode="used"}/{mode="total"}` 两序列 mode 值不同，直接相除
   向量匹配失败——必须 `/ on()` 空标签匹配。
3. 规则只引用已真实埋点的指标：初版含 Prometheus/Loki/alloc/SIMD 六条
   幻影规则（无任何 emitter），已删除。Loki 健康看 Grafana 数据源，
   TrackingAllocator 趋势看 `profile.sh`，SIMD 降级看启动日志——三者进
   指标体系是后续项，本单只在诊断引擎知识库留规则槽位。

## 3. Alertmanager（config/alertmanager.yml）

- 根路由按 `(alertname, job, severity)` 分组；`critical` 双投
  critical-webhook（`continue: true`，1h 重复）；
- `GatewayRateLimitExceeded` 同 job 10 分钟只通知一次（限流风暴天然高频）；
- `severity: info` 进 `null` 接收器只记录；
- 抑制三条：上游挂→屏蔽其 scrape 派生告警；内存压力→屏蔽查询延迟；
  连接中断→屏蔽消息间隔（因果方向：源在上、派生在下）。
- 内网 webhook 无需认证：配置里不写空 `basic_auth`/`bearer_token`
  （空用户名反而使 `amtool check-config` 报 invalid）；`templates` 段
  在模板文件落盘前不写（指向不存在路径 alertmanager 拒绝启动）。

## 4. Webhook（services/alert-webhook，随 workspace 门禁）

- Axum 三路由：`/health`、`/alerts`、`/alerts/critical`（与 alertmanager
  两 receiver URL 逐字对齐）；渠道 env 缺失只告警日志不阻断启动；
  分发 `tokio::spawn` 并行、单渠道失败不影响其他渠道。
- 反序列化兼容：Alertmanager 原生 `externalURL`/`generatorURL`
  （URL 全大写，`rename_all = "camelCase"` 推导不上，必须显式
  `rename`）；`generatorURL` 缺失默认空串（代理裁剪不整批 400）。
- Label 取值走 `label_or` helper——`unwrap_or(&"...".to_string())`
  借用函数内临时值（E0716/E0515），是本文件最高频的编译坑。

## 5. 诊断（两套并存，分工不同）

- **CLI 实际消费的**：`alpha_core::diagnosis`（`packages/core/src/diagnosis.rs`）
  ——指标快照（ServiceMetrics/Snapshot）→ Finding，阈值常量与 §2 告警规则同源。
  CLI `tools/diagnose`（`cargo run -p alpha-diagnose -- --help`）从 Prometheus
  拉快照（唯一 env `ALPHA_PROMETHEUS_URL`）→ JSON 报告，退出码 0/1/2/3。
- **已接线的规则知识库**：`packages/storage/src/diagnosis.rs` 的 `DiagnosisEngine`
  ——告警指纹+时间窗口输入 → `DiagnosisReport`（`DiagnosisRule`：告警通配/必需
  指标异常/日志模式/追踪特征/根因分类/基础置信分/修复动作），纯函数零 IO、
  builtin 规则在档。消费方：alert-webhook（2026-10 接线）——告警名 → 规则
  初判根因与建议动作富化进通知消息；无证据注入时按规则 base 置信度入册
  （告警名归属是一等信号），未匹配已知模式不硬塞根因。
- 拉数归属：alert-webhook 只做规则级富化不拉数；指标/日志/追踪证据增强
  随 Loki/Jaeger 拉数接线再进（届时置信分从 base 提升）。CLI 消费的是
  `alpha_core::diagnosis`（另一套，见上条）。

## 6. 非交互假设（自行判定，已注明）

1. 通知渠道只做到 Webhook 转发层；短信/电话升级链归运维侧。
2. `repeat_interval` 默认 4h（critical 1h）：告警风暴与打扰度的折中，
   随 on-call 制度调。
3. 诊断引擎知识库规则已覆盖全部 11 条告警（2026-10 补齐
   `GatewayShieldTriggered` → GW-004，护栏只标记不封禁、按 mode 甄别的
   处置面入规则建议动作；L504 `CollectorSourceDegraded/Down` →
   COL-001/002，降级=依赖劣化 70 分、中断=下游故障 85 分，处置建议=
   查 `/sources/health` 台账与原始响应归档、甄别改版/封禁/宕机）。
   INFRA-002/003（Loki/AllocTracker）对应的是登记未实现指标，属规则
   槽位（见上条硬教训 3）。置信分人工复核。
4. alert-webhook 用 reqwest 0.12（需 rustls-tls），与网关的 0.11
   并存——服务独立构建，版本不强制统一。
