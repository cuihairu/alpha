# Alpha 架构收敛审查

> 状态：2026-10-04 审查产出（定位收敛 + 现状事实核查 + 问题清单 + 调整方案）。
> 本文是「审查 + 调整方案」，不是新的开发 roadmap——改动项需逐项立项推进。

## 0. 定位声明（收敛版）

**Alpha = 自托管金融数据与量化研究基础平台。**

一句话：采集公开行情/公告/新闻 → 标准化 → 存储 → 查询 → 实时分发 → 指标/分析 → 回测，全部自托管、可重放、可追溯。

这不是：券商交易系统 / 高频交易系统 / AI 股票预测器 / 投资建议系统 / 交易终端 / 超级微服务平台。

**产品边界以五个核心域收口**（映射到现有代码，见 §4.2）：

| 域 | 现有落点 |
|---|---|
| ingestion（采集） | `services/collector`（task templates + cron 调度 + sources + cleaner + rate_limiter） |
| data（数据仓） | `packages/storage` + `services/data-engine`（Timescale/ClickHouse/Redis/MinIO） |
| research（研究） | `packages/core`（indicators/backtest/optimize/risk/parallel）+ wasm-analyzer |
| realtime（实时） | `services/real-time-feed` + Redis Streams |
| access（访问） | `services/api-gateway`（REST/gRPC/WS + 鉴权/限流 + 追踪） |

## 1. 现状事实核查（外部评审论断 ⇄ 仓库代码）

| # | 外部评审论断 | 核查结果 | 证据 |
|---|---|---|---|
| 1 | 存储抽象过多 | ✅ 坐实 | `packages/storage` 17 个模块：4 种 KV（memory/disk_kv/redis_kv/postgres_kv）+ 2 种时序（timeseries/timescale）+ clickhouse/cloud/dal/cache/… |
| 2 | Event Envelope 应升级 | ✅ 部分 | `StreamEnvelope` 已有 `id/stream/version/event_type/source/symbol/ingest_ts/payload_hash/payload`；缺 `trace_id`、`sequence`、`source_event_id`（✅ 已修复，commit 0af6fe0：v2 六字段加性迁移） |
| 3 | Instrument 统一模型缺失 | ✅ 坐实 | 全仓无 Instrument 实体（仅 timescale.rs 出现标识字符串）；A股多形态符号风险真实存在（✅ 已修复，commit 024a63d：`protocols/src/instrument.rs` + `/instruments` 端点） |
| 4 | 时间三模型（event/ingest/process） | ✅ 缺 process_time | envelope 只有 `ingest_ts` + payload 内嵌行情 `timestamp`；normalized 层无处理时间戳（✅ 已修复，commit 0af6fe0） |
| 5 | Timescale 应该弱化、ClickHouse 第一 | ✅ 方向成立 | data-engine 活跃写路径 = Timescale 持久化镜像；ClickHouse 现仅两个导出端点（`/clickhouse/export.parquet`、`/clickhouse/market-data.parquet`）归档，无 INSERT 写路径 |
| 6 | Redis 只做缓存/流 | ✅ 现状符合但需守住 | cache/rate_limit/redis_streams ✓；redis_kv 属抽象债（见 §2.1） |
| 7 | Realtime 订阅收敛（fanout 去重） | ✅ 现状为「连接即订阅」 | real-time-feed 757 行注释：当前不区分动作；连接-级订阅，无按 symbol 的共享订阅 |
| 8 | API 三层（Public/Research/Internal） | ✅ 未分层 | 网关单层 `/api/v1/*` 反代 + `/ws` |
| 9 | 反爬不设为核心 | ✅ 已天然降级 | README 保留 proxy pool 描述；代码中代理池为可选组件（ProxyPool），UA 轮换在 RequestConfig |
| 10 | PROJECT_SUMMARY 宣传口径 | ❌ 严重过时 | 仍写 trading.rs（已删）、Tauri/JNI「计划中」（已有骨架）、WASM 高性能表述需按 §3.4 校准措辞（✅ P0 已处理：已按定位声明重写） |

## 2. 问题清单（按级别，附证据与建议动作）

### 2.1 职责重复 / 过度抽象（存储层）

**问题**：`memory.rs / disk_kv.rs / redis_kv.rs / postgres_kv.rs` 四套 KV 并存各自导出，`timeseries.rs / timescale.rs` 两套时序抽象并存，`dal.rs` + 各 Storage 自带头。抽象面比产品需要多。

**证据**：`packages/storage/src/lib.rs` 17 个模块、pub use 现为 17 条且仍全量 `pub use x::*` 重新导出〔现注：白名单化未执行，行动项登记在 TODO〕——消费者（data-engine/collector）实际只用到 redis_streams / cache / timescale / clickhouse / cloud / columnar / partition。

**建议（收敛、不删除）**：
1. 确立**三个权威后端**：ClickHouse（主数据仓）、Redis（缓存/流/限流/锁）、MinIO/S3（原始归档 + parquet 数据集）。Timescale 保留为可选后端（性能/回退），不再进入新功能默认路径。
2. `lib.rs` 的 `pub use` 改为**显式白名单**（列实际消费面），KV 次级后端不再公共导出，标注 "legacy/可选"，避免新代码顺手引用。
3. 新写数据面代码一律经 `dal.rs` 单入口，禁止直接 new 具体 Storage。

**收益**：新读者 5 分钟看懂存储形态；迁移成本为 0（纯导出面调整，代码不删）。

### 2.2 错误分层

**问题 A**：`StreamEnvelope` 是**存储层类型**（`packages/storage/src/redis_streams.rs`），但它是全管线的消息契约——collector 生产、data-engine 消费、real-time-feed 转发。契约应属于 `packages/protocols`（纯 serde，无存储依赖），存储层只负责持久化它。

**建议**：将 `StreamEnvelope` 迁移至 `packages/protocols`（rest.rs 的姐妹文件 `events.rs`），`redis_streams.rs` 改为引用。`packages/protocols` 已是 wasm-clean 无锁依赖的契约层，天然是正确归宿。这是「契约与实现剥离」的最小改法，也是 §3.2 envelope 升级的前置。

**问题 B**：反爬能力（proxy pool / fingerprint/UA 轮换）在 README 与代码层多处出现，但 Alpha 的差异化是**数据可靠性**。采集层按「数据源运行时」收敛（已有雏形：task templates 声明 source/url/parser/schedule + cron 执行 + 重试策略），代理池保留为可选注入，不进核心叙事。

**建议**：README 技术决策中反爬段落降为「可选组件，接入时按源配置」；新采集功能围绕 SourceDefinition（type/schedule/rate_limit/retry/parser/schema/health_check）扩展，不围绕反爬扩展。

### 2.3 文档债

**问题**：`PROJECT_SUMMARY.md` 与仓库现状脱节（trading.rs 已删、desktop 已有骨架、wasm-analyzer 已存在、结构图过时）；README 的 roadmap 未反映已完成的采集框架（task templates + cron 调度已在 architecture §1 标注，README 未同步）。（✅ P0 已处理：两者均已重写/勾选）

**建议**：PROJECT_SUMMARY 按本次定位声明重写结构图与状态节（独立小任务）；README 的 Roadmap 第 3 项勾选完成。

### 2.4 未来风险（提前不引入清单）

- **不做**：Kafka / NATS 升级（在吞吐、审计、回放需求明确后再说，README 已声明阶段化）——Redis Streams + claim/sweeper/DLQ 已满足当前规模。
- **不做**：Kubernetes（systemd + compose 足够）。
- **不做**：AI 价格预测 / TensorFlow.js 列为路线（即便将来做也是 Alpha 数据的消费者，见 §3.6）。
- **不做**：iOS 作为主线（现有 mobile 骨架保留，发布节奏自定）。

## 3. 数据模型调整方案

### 3.1 EventEnvelope v2（在现有 StreamEnvelope 上做加法）

```text
EventEnvelope {                    # 现状 → 建议
  event_id          # ✓ 已有 id（建议改由 producer 必填生成）
  stream            # ✓
  version           # ✓
  event_type        # ✓
  source            # ✓
  source_event_id   # ✗ 新增：数据源自己的事件 ID（去重/对账锚点）
  symbol            # ✓
  market            # ✗ 新增：cn/hk/us（配合 Instrument）
  timestamp         # ✓ 已在 payload 内嵌 —— 提升到 envelope 层（event_time）
  ingest_ts         # ✓ 已有（ingest_time）
  process_time      # ✗ 新增：每跳处理完成时间（normalized 写入时打）
  sequence          # ✗ 新增：per-source 单调序列号——检测断档的核心
  payload_hash      # ✓
  trace_id          # ✗ 新增：与 gateway X-Trace-Id 链路对接
  payload           # ✓
}
```

字段全部为**新增或重命名**，向前兼容（serde `#[serde(default)]`），老消息可读。断档检测 = `sequence` 跳号 + 超时未到 → 告警（补 §2.5 数据质量）。

### 3.2 Instrument（A股核心实体，新建于 packages/protocols）

```text
Instrument {
  instrument_id   # 全仓唯一主键，如 "cn.600519"
  exchange, market, symbol, name,
  type, currency, status,
  listed_at, delisted_at
}
```

消歧映射（600519 / SH.600519 / 600519.SH / 贵州茅台）内置为纯函数 + 数据表；data-engine 提供 `/instruments` 查询；所有新表/新端点只引用 `instrument_id`。这是「数据契约」的地基，做在存储结构定型前。

### 3.3 时间模型

全链三类时间戳纪律（envelope v2 已含 event/ingest/process）：
- `event_time`：数据源侧事件时刻（原 payload.timestamp 上提）
- `ingest_time`：Alpha 收到时刻（原 ingest_ts）
- `process_time`：各处理跳完成时刻（新增，normalized/derived 各打一列）

依赖：可计算 source latency / ingestion latency / processing latency / end-to-end（项目定位 low-latency 的可度量基础）。

### 3.4 存储三职责（替代「存储工厂」叙事）

```text
ClickHouse ← 主数据仓（k线/tick/indicators/snapshot/研究数据集；Parquet 为冷层）
Redis     ← 缓存 / Streams / 锁 / 限流 / fanout 状态（禁止当第二数据库）
MinIO/S3  ← raw 原始归档 + dataset parquet + exports
```

Timescale 保留为可选 TimescaleDB 后端（data-engine persistence 已是三态装配，不改代码，只降级默认文案）。

### 3.5 数据集（Research Dataset）与 Experiment（研究可复现）

- `dataset://{domain}/{granularity}/{version}` 概念：dataset_id + source + time_range + symbols + schema_version + checksum + created_at——回测/研究引用的是**某一版数据**而非当前表。
- Experiment 记录（experiment_id / dataset_id+version / strategy+code_version / parameters / seed / result / created_at）：回答「为什么昨天 1.32 今天 1.17」。
- 这两个先落**协议与登记表**（小），再落包级实现（P3）。

### 3.6 MCP 访问面（access 域扩展，慢启动）

`Alpha MCP Server`：query quote / history / indicators / backtest / dataset / search news——把 Alpha 变成 AI Agent 的数据后端（与现有 AI 编码体系对接）。不承诺 AI 预测。

## 4. 目录 / 模块调整方案

### 4.1 存储层收敛动作（立即可做，纯导出面）

〔现注：尚未执行——pub use 仍全量导出；行动项已登记 TODO。〕

```text
packages/storage/src/lib.rs
  - pub use 白名单化（保留：cache/clickhouse/cloud/columnar/dal/encryption/
    memory(查询面)/partition/prefetch/rate_limit/redis_kv(仅限流键)/redis_streams/
    timescale(可选)/timeseries/... 主要面）
  - disk_kv/postgres_kv 标注 legacy，不进白名单
```

### 4.2 契约层迁移（数据模型前置项）

`StreamEnvelope` → `packages/protocols/src/events.rs`（纯 serde）+ `Instrument` 契约同置；`redis_streams.rs` 依赖协议层。这一步做完，envelope v2 升级才有干净的落点。

### 4.3 采集层收敛（已有雏形，继续按此口径）

```text
Collector = Data Source Runtime
  SourceDefinition（模板已含 source_type/url/params/schedule/retry/parser/storage）
  + cron 调度（已落地）
  + rate limit / 重试 / 健康检查（接线中）
  目标：任何数据源 = 一份声明文件，无新代码
```

## 5. 分期

| 阶段 | 内容 | 性质 | 状态 |
|---|---|---|---|
| P0（立即，docs-only 可先行） | 定位声明落 README/PROJECT_SUMMARY；本文入文档站 | 文档收敛 | ✅ 已落（6968e57 + 后续对账批） |
| P1（下一开发轮） | envelope v2 字段 + protocols 迁移 + process_time；Instrument 契约 + /instruments | 数据模型 | ✅ 已落（0af6fe0、024a63d） |
| P2 | 数据质量系统（完整性/连续性/异常/重复/Source Divergence）+ sequence 断档告警 | 可靠性 | 进行中——sequence 断档已落（9254efe）；重复/完整性观测面已落（重复命中、规范化失败、DLQ 隔离三计数，2026-10）；异常检测与 Source Divergence 未开始 |
| P3 | 三级 API 分层；Research Dataset + Experiment 登记表；MCP 慢启动 | 能力面 | 未开始 |
| P4 | UI 回归 API consumer 定位（不设独立路线图） | 收敛 | 未开始 |

**不做清单在此文档 §2.4（Kafka/K8s/AI 预测/反爬核心化/iOS 主线）——后续评审以此为对齐锚。**

## 6. 与外部评审的差异说明（诚实保留）

1. **Timescale 弱化**：采纳为「新功能默认走 ClickHouse」，但**现有持久化镜像不迁移**（C 侧工作量、功能等价）。降级是路径调整不是重构。
2. **反爬降级**：采纳描述口径；不删现有 ProxyPool/UA 轮换代码（采集面保留注入点）。
3. **ClickHouse 立即第一库**：不立即做——现 ClickHouse 无活跃写路径，一步到位切换需 schema 定义 + 写路径迁移 + 回放验证，单独立项（P1 数据模型定型后）。

## 7. 附带发现（审查中顺手记录的独立小债）

- `docs/sidebars.js` 手写登记已覆盖本仓库文档；新增文档必须登记，否则 orphan 警告。
- PROJECT_SUMMARY 结构图与 `ls packages services` 实际布局已有偏差（wasm-analyzer 未列、trading.rs 已删仍列）。（✅ P0 重写已纠正）
- README 的 roadmap 第 3 项（crawler framework with scheduling + proxy rotation）实际已完成（task templates + cron 调度 + ProxyPool），未勾选。（✅ 已勾选）