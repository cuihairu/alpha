---
sidebar_position: 81
---

# 源码分层分析

本文按实现职责归纳运行面，不以 `packages/`、`services/` 等目录名代替架构结论。英文对照见 [Source Layer Analysis](./source-layer-analysis.en.md)，术语定义逐条见 [源码术语表](./source-layer-glossary.md)。

## 七个抽象层

| 层 | 职责 | 代表实现 |
| --- | --- | --- |
| L01 协议与共享领域模型 | 固定 REST/WebSocket/Envelope/gRPC/instrument 契约，并把行情、指标、回测、风控等计算模型集中成跨端共同口径。 | `packages/protocols/src/lib.rs:21`、`packages/core/src/lib.rs:8` |
| L02 存储与基础设施 | 抽象内存、Redis、Postgres/TimescaleDB、ClickHouse、对象存储、本地 KV/Parquet 等后端，并提供队列、限流、加密和分区写读能力。 | `packages/storage/src/lib.rs:56` |
| L03 采集与数据摄入 | 装载采集任务模板，执行 HTTP/脚本，做清洗、限流、健康观察、原始归档，并把产出发布到摄入流。 | `services/collector/src/main.rs:21`、`services/collector/src/main_simple.rs:21` |
| L04 流处理与计算服务 | 消费摄入流，规范行情，运行指标/风控/回测/SQL，并把热数据写入 TimescaleDB、ClickHouse 和 Parquet 湖。 | `services/data-engine/src/main.rs:190`、`services/data-engine/src/main.rs:245` |
| L05 传播、安全与外部出口 | 对外统一 REST/WebSocket，做认证、防滥用、限流、代理转发、账户同步、实时扇出和告警通知。 | `services/api-gateway/src/main.rs:152`、`services/real-time-feed/src/main.rs:94` |
| L06 终端运行时 | Web、桌面、移动与 WASM 运行时组装状态、本地持久化、平台桥、前端展示与浏览器引擎。 | `web/app/src/App.tsx:34`、`desktop/src/state.rs:15`、`mobile/src/state.rs:58`、`wasm-analyzer/src/lib.rs:52` |
| L07 运维与交付 | 提供容器、观测、告警、合规清单、构建与发布入口；服务把它当作配置与可观测出口，而非业务层。 | `docker-compose.yml:14`、`config/prometheus.yml:1` |

## 调用面

- **前端到后端**：React 壳经 `web/app/src/lib/liveFeed.ts:215` 调 real-time-feed `/ws`；DuckDB 工作台经 `web/app/src/lib/duckdb.ts:42` 调 data-engine `/query`；桌面面板经 `web/app/src/lib/desktop.ts:29` 检测 Tauri 注入面。
- **服务端主线**：collector 经 `services/collector/src/main_simple/executor.rs:393` 写 Redis Streams；data-engine 经 `services/data-engine/src/normalizer.rs:24` 消费它，再写 TimescaleDB、ClickHouse 或 Parquet。
- **出口与横切**：API gateway 经 `services/api-gateway/src/main.rs:204` 连 Redis 限流，经 `services/api-gateway/src/main.rs:374` 连可选 Postgres 账户库，转发代理目标在 `services/api-gateway/src/main.rs:152`。real-time-feed 经 `services/real-time-feed/src/stream.rs:12` 消费 Redis，Web 客户端只连 `/ws`。
- **告警链路**：Prometheus 按 `config/prometheus.yml:14` 请求指标，Alertmanager 转发到 alert-webhook `/alerts`；alert-webhook 经 `services/alert-webhook/src/main.rs:241` 读取外部渠道配置，用 HTTP 发送通知。
- **平台壳**：Android/iOS 通过 UniFFI 调 `mobile/src/state.rs:58`；Tauri 进程在 `desktop/src/main.rs:8` 把框架层接线到系统窗口和导出能力。

## 状态存储

| 状态 | 存储 | 证据 |
| --- | --- | --- |
| collector 任务、运行状态 | 进程内 `RwLock<HashMap>` | `services/collector/src/main_simple.rs:28` |
| raw collector events | Tokio broadcast | `services/collector/src/main_simple.rs:34` |
| 摄入消息、DLQ | Redis Streams | `packages/storage/src/redis_streams.rs:23` |
| data-engine 热序列 | 进程内 `TimeSeriesStorage` | `services/data-engine/src/main.rs:253` |
| 规范镜像 / 历史 | TimescaleDB、ClickHouse、Parquet 文件 | `services/data-engine/src/main.rs:193`、`services/data-engine/src/main.rs:217` |
| real-time 连接、告警窗口、同步水位 | 进程内 broadcast/Mutex/HashMap | `services/real-time-feed/src/main.rs:94` |
| gateway JWKS、防滥用状态、限流 | 内存；限流在 Redis | `services/api-gateway/src/main.rs:164`、`services/api-gateway/src/main.rs:158` |
| 账户与跨端同步 | 内存权威，可选 Postgres 写穿 | `services/api-gateway/src/main.rs:170` |
| Web workspace/theme/privacy | 浏览器 localStorage | `web/app/src/App.tsx:41` |
| desktop config/alerts/window/KV/export | 平台目录中的文件 | `desktop/src/paths.rs:9` |
| mobile offline/preferences | 宿主注入 KV；Rust core 内存决策 | `mobile/src/state.rs:70`、`mobile/android/app/src/main/java/com/alpha/finance/mobile/OfflineStore.kt:19` |
| 原始采集证据 | 可选 S3/MinIO 对象存储 | `services/collector/src/raw_archive.rs:28` |

## 配置输入与生命周期

各服务以显式启动装配为主：collector 使用 `ALPHA_WORKSPACE_ROOT`、`ALPHA_COLLECTOR_TASKS`、`ALPHA_COLLECTOR_BIND` 和可选 raw archive 变量；data-engine 由 `services/data-engine/src/settings.rs:10` 定义文件与环境覆盖；gateway 把 CLI 默认值和 `ALPHA_GATEWAY_*` 组合成 `services/api-gateway/src/main.rs:64`；real-time-feed 和 alert-webhook 读取 `ALPHA_*`/外部 webhook 变量。桌面使用 `desktop/tauri.conf.json:3` 构建 `web/app/dist`，并把平台目录契约注入 `desktop/src/paths.rs:9`；移动端配置在 FFI 构造时传入，后续由宿主 KV 接线。

进程生命周期都是“启动装配、后台任务运行、显式健康/指标端点”：collector 先建状态再启动 cron/HTTP（`services/collector/src/main.rs:22`）；data-engine 初始化热层、TimescaleDB、ClickHouse、湖表与流任务（`services/data-engine/src/main.rs:433`）；real-time-feed 在缺少 Redis 时退化为模拟流（`services/real-time-feed/src/main.rs:137`）；gateway 先连可选外部状态再启动 HTTP/TLS（`services/api-gateway/src/main.rs:176`）；桌面/移动应用把宿主路径或平台桥交给 Rust 核心状态。

## 文档与产物落位

协议 schema 在 `packages/protocols/proto/data_engine.proto:1`；REST/WS/Envelope 的 Rust 契约在 `packages/protocols/src/lib.rs:21`。服务专属 YAML/JSON schema 由各服务定义：collector 模板在 `services/collector/src/task_templates/schema.rs:40`，data-engine 配置在 `services/data-engine/src/settings.rs:10`，gateway CLI/env 默认值在 `services/api-gateway/src/main.rs:64`。文案挂在 web 文案表 `web/app/src/lib/i18n.ts:1` 与 Android 字符串资源 `mobile/android/app/src/main/res/values/strings.xml:1`；桌面复用 `web/app` bundle，iOS 壳文案在 SwiftUI 视图内。专属运维文件集中在 `docker-compose.yml:1` 和 `config/`。

## 拆分理由

1. 协议契约必须独立成包：REST、WS、Envelope、instrument 常量从 `packages/protocols/src/lib.rs:21` 统一导出，data-engine 的线上 gRPC 合同另有 `packages/protocols/proto/data_engine.proto:12`，避免服务各自复制字段语义。
2. 存储后端必须可替换且不被服务硬编码：`packages/storage/src/lib.rs:56` 的 `StorageBackend` 收敛六个操作，`packages/storage/src/lib.rs:76` 工厂支持 Memory、Postgres、Redis、S3、LocalDisk；队列、限流、ClickHouse、lake 保留专型接口以匹配各自访问模式。
3. 采集与处理之间必须用可重放边界解耦：collector 用进程内任务状态和广播事件驱动执行（`services/collector/src/main_simple.rs:21`），但产出经 Redis Streams 发布（`services/collector/src/main_simple/executor.rs:393`）；data-engine 可三态降级连 TimescaleDB/ClickHouse（`services/data-engine/src/bootstrap.rs:46`）。
4. 实时扇出不与持久处理同进程：real-time-feed 使用进程内 broadcast 与连接状态（`services/real-time-feed/src/main.rs:94`），WS 帧契约取自 `packages/protocols/src/websocket.rs:33`，Redis 只作为上游来源（`services/real-time-feed/src/stream.rs:12`）。
5. 安全与路由必须在业务代理前横切：gateway 在 `services/api-gateway/src/main.rs:406` 组织中间件，`/v1/*path` 转发（`services/api-gateway/src/main.rs:392`）、账户端点（`services/api-gateway/src/main.rs:395`）、WS 代理（`services/api-gateway/src/main.rs:420`）分开；账户语义本身留在 `services/api-gateway/src/account.rs:160`。
6. 跨端算法共享，平台差异只在桥/壳：mobile 明确“业务与计算全部在 alpha-core”（`mobile/src/lib.rs:11`），UniFFI 唯一导出在 `mobile/src/lib.rs:57`；desktop 把无 Tauri 依赖框架层与 Tauri 接线层分开（`desktop/src/lib.rs:5`），并直接复用 `desktop/tauri.conf.json:6` 里的 `web/app/dist`；WASM 只公开浏览器分析与同步缝（`wasm-analyzer/src/lib.rs:46`）。
7. 告警发送是独立边界：Alertmanager payload 模型与四个发送适配器集中在 `services/alert-webhook/src/main.rs:79` 和 `services/alert-webhook/src/main.rs:471`，服务不持有长期业务状态（`services/alert-webhook/src/main.rs:37`）。

