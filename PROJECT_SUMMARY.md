# Alpha — 自托管金融数据与量化研究基础平台

> 定位声明（与 `docs/architecture-review.md` §0 对齐）：**采集公开行情/公告/新闻 →
> 标准化 → 存储 → 查询 → 实时分发 → 指标/分析 → 回测**，全部自托管、可重放、可追溯。
>
> 这不是：券商交易系统 / 高频交易系统 / AI 股票预测器 / 投资建议系统 / 交易终端 / 超级微服务平台。

## 项目状态

核心数据平面已打通并有测试覆盖；文档站与架构审查详见 `docs/`。平台持续开发中，
按 `docs/architecture-review.md` §5 的分期推进（P0 文档收敛 ✅ → P1 数据模型 ✅ →
P2 数据质量 → P3 API 分层/数据集/Experiment → P4 UI 收敛）。

## 五个核心域

| 域 | 落点 |
|---|---|
| ingestion（采集） | `services/collector`：task templates（YAML/JSON 声明式数据源）+ cron 调度 + 东方财富行情源 + cleaner + rate_limiter + 重试策略 |
| data（数据仓） | `packages/storage` + `services/data-engine`：Redis Streams 消息契约 + 内存/内存镜像时序 + ClickHouse 导出归档 + Parquet 数据湖 |
| research（研究） | `packages/core`：indicators（RSI/MACD/SMA/EMA/布林带/ATR…）/ backtest / optimize / risk / parallel + `wasm-analyzer`（WASM 契约层） |
| realtime（实时） | `services/real-time-feed`：Redis Streams 消费 + WebSocket 连接级订阅 + DLQ 兜底 |
| access（访问） | `services/api-gateway` + `services/data-engine` HTTP 面：REST/WS + API key 门 + 追踪 + Prometheus 指标 |

## 仓库布局

```
packages/
  core/       研究域：indicators / backtest / optimize / risk / analytics / parallel / crypto / hybrid_cache …
  protocols/  契约层（纯 serde、wasm-clean）：events（Envelope v2）/ instrument（Instrument 契约）/ rest / websocket / grpc
  storage/    Redis Streams 队列 / 时序存储 / Timescale 镜像 / ClickHouse / 缓存 / 限流 / 加密
services/
  collector/         数据源运行时：task templates + cron 调度 + 行情抓取 + 重试 + 限流
  data-engine/       数据面：normalize 消费 / /query SQL / 技术指标 / /instruments / ClickHouse 导出
  real-time-feed/    WebSocket 实时分发（连接级订阅 + DLQ）
  api-gateway/       外部访问面：REST/WS 反代 + 鉴权 + 追踪
  alert-webhook/     告警出口
  crawlers/          采集脚本（Python/多语言）
desktop/             Tauri 桌面骨架（跨端契约）
mobile/              Android（JNI）+ iOS（UniFFI）骨架
wasm-analyzer/       WASM 客户端分析契约（--no-default-features 纯 serde）
web/                 Web 前端（状态：UI 收敛为 API consumer，见 architecture-review §P4）
docs/                Docusaurus 文档站（architecture / architecture-review 等 42+ 篇）
scripts/             CI 门禁四件套：check-lint / check-cross-platform / check-desktop
```

## 已交付能力

### 消息契约（packages/protocols）
- **Envelope v2（`events.rs`）**：全管线消息契约——v1 十字段 + 加性字段
  `source_event_id / market / event_time / process_time / sequence / trace_id`；
  时间三跳模型（ingest/event/process）可度量端到端延迟；加性原则保证老消息可读、
  新消息缺省时字节面与 v1 一致。
- **Instrument 契约（`instrument.rs`）**：全仓唯一键 `cn.{exchange}.{symbol}`
  （`cn.sse.000001` 上证指数 vs `cn.szse.000001` 平安银行）；多形态符号消歧纯函数；
  data-engine 提供 `GET /instruments` 查询。

### 采集（services/collector）
- **task templates**：YAML/JSON 声明式数据源（source_type / url / schedule / retry / storage）
- **cron 调度器**：5/6 段 cron 解析 + 1s tick 派发 + 任务互斥（同类任务串行）+
  模板校验与调度执行面共用同一解析器

### 数据面（services/data-engine）
- Redis Streams 消费（quotes.raw）+ normalized 写路径（内存 serving + Timescale 可选镜像）
- `/query` DataFusion SQL、技术指标计算、`/stocks/:symbol/history`、ClickHouse parquet 导出
- 去重窗口 + 镜像失败重试缓冲 + API key 门（默认关）

### 研究（packages/core + wasm-analyzer）
- 技术指标库（RSI/MACD/SMA/EMA/布林/Stochastic/Williams %R/ATR/动量）、回测引擎、
  风险指标（夏普/回撤）、优化与并行骨架；WASM 契约层可编译验证

### 实时（services/real-time-feed）
- Redis Streams → WebSocket 连接级订阅；毒消息/解码失败进 DLQ 兜底 + ack 契约

## 工程纪律

- **门禁**：lint（clippy 零警告 + rustfmt）、`cargo test --workspace --all-targets
  --exclude alpha-desktop`、cross-platform（含 protocols wasm32）、desktop——全绿才合入。
- **契约演进**：流内消息加性演进（default + skip_serializing_if）；API 面严格
  （deny_unknown_fields）；新表/新端点只引用 `instrument_id`。
- **不做清单**（与 architecture-review §2.4 对齐）：Kafka/NATS 升级（吞吐/审计/回放
  需求明确前）、Kubernetes、AI 价格预测、反爬核心化、iOS 主线。

## 下一步（按分期）

- **P2 数据质量**：完整性/连续性/异常/重复检测 + sequence 断档告警（契约字段已就位）
- **P3**：三级 API 分层（Public/Research/Internal）；Research Dataset + Experiment 登记表；MCP 慢启动
- **P4**：UI 回归 API consumer 定位

**Alpha = 自托管金融数据与量化研究基础平台。**