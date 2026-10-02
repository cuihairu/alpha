# 分布式追踪系统：tracing + Jaeger

落地分两层：**本单实现的 trace-id 贯穿链路**（可立即用）与
**Jaeger OTLP 全量 span 导出（登记边界，随发布节启用）**。

## 1. 已实现：gateway trace-id 贯穿

契约（`services/api-gateway`）：

| 环节 | 行为 |
|---|---|
| 入站带 `X-Trace-Id`（非空） | 原样沿用——跨服务/客户端发起的重试与后续请求串同链路 |
| 入站缺失/空白 | 网关生成 `tr-<uuid v4>`（网关即链路起点） |
| 转发上游 | 循环后统一注入（避免与透传头叠加成双值），上游日志可按 trace_id 检索 |
| 响应回填 | `X-Trace-Id` 原样返回客户端（前端可展示/关联报障） |

实现锚点：`resolve_trace_id`（纯函数，单测锁定三态）+ `api_proxy`
（转发注入/响应回填/`tracing::info!(trace_id = ...)` 结构化字段）。

限流/健康检查与 WS 反代不注入：限流响应自身带 Retry-After 语义；
WS 为长连接会话（会话级关联用连接日志），trace-id 单请求语义不适配。

## 2. 与 Loki 日志关联（已可用）

各服务 tracing 日志经 Promtail 进 Loki（见 docs/memory-profiling.md 的
监控栈组合）。检索链路：

```logql
# 全链路按 trace-id 串联（gateway → data-engine → collector）
{job=~"alpha-.*"} |= "tr-9f3c..."
```

要求服务日志含 trace-id 字面量：网关侧已带（结构化字段 +
`Proxying` 行）；其余服务转发请求头中的 `X-Trace-Id` 由上游 handler
自行 `tracing::info!(trace_id = ...)` 记录（按需逐步补齐，不阻塞链路串联）。

## 3. 登记边界：Jaeger OTLP span 导出（本单不实现）

全量 span 导出需要 opentelemetry 全家桶（`opentelemetry` +
`opentelemetry-otlp` + `tracing-opentelemetry`，版本需与
tracing-subscriber 对齐）接入四个服务的 subscriber 初始化。
**沿 L445 设计项先例：本单登记不实现**，启用路径：

1. workspace 增加 `opentelemetry = "0.27"` / `opentelemetry-otlp = "0.27"` /
   `tracing-opentelemetry = "0.28"`（以 lockfile 对齐为准）；
2. 各服务 tracing 初始化叠加 `tracing_opentelemetry::layer()` +
   `opentelemetry_otlp::SpanExporter`（`OTEL_EXPORTER_OTLP_ENDPOINT`
   指向 Jaeger Agent `jaeger:4317`，compose 增 `jaegertracing/all-in-one`）；
3. `resolve_trace_id` 生成的 `tr-<uuid>` 与 OTel trace-id 的映射：网关侧
   从 `Span::current().context().span()` 取 OTel trace-id 作为回填值
   （W3C `traceparent` 头与既有 `X-Trace-Id` 并存）；
4. 采样策略默认 `parentbased_traceidratio`（10%）——行情高 QPS 下全量
   span 不可承受。

## 4. 非交互假设

1. `tr-<uuid>` 格式为仓内约定（非 W3C trace-id 32hex）——第 3 节启用时
   以 OTel trace-id 为准回填，本格式仅服务网关启动期。
2. data-engine/collector/real-time-feed 侧的 trace-id 日志字段逐步补齐
   （读取转发头即可），不构成验收阻塞——链路串联在网关侧已闭环。
