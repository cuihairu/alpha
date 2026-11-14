---
sidebar_position: 83
---

# 源码术语表

本表按实现职责登记抽象层概念；同义英文版见 [Source Glossary](./source-layer-glossary.en.md)，架构归纳见 [源码分层分析](./source-layer-analysis.md)。

| 术语 | English | 定义 | file:line |
| --- | --- | --- | --- |
| 协议包 | Protocol package | REST、WebSocket、事件、标的代码和 gRPC 合同的共享导出面。 | `packages/protocols/src/lib.rs:21` |
| REST 合同 | REST contract | API 响应、分页、搜索、历史请求和错误码的数据形状。 | `packages/protocols/src/rest.rs:9` |
| WebSocket 合同 | WebSocket contract | 订阅、认证、版本化同步、心跳和错误帧的共享结构。 | `packages/protocols/src/websocket.rs:8` |
| 事件信封 | Event envelope | Redis Streams 消息的版本、来源、去重、时间、序列和追踪字段。 | `packages/protocols/src/events.rs:29` |
| gRPC 合同 | gRPC contract | data-engine 指标、历史与绩效服务的 Protobuf schema。 | `packages/protocols/proto/data_engine.proto:12` |
| 标的标识 | Instrument ID | 市场、交易所、代码组成并解析回组件的稳定标识规则。 | `packages/protocols/src/instrument.rs:136` |
| 共享核心 | Shared core | 跨平台账务、指标、形态、回测、风控、缓存与流式算法集合。 | `packages/core/src/lib.rs:8` |
| 市场模型 | Market model | 行情、标的、指标与衍生数据的领域载荷。 | `packages/core/src/models.rs:1` |
| 混合缓存 | Hybrid cache | 热路径驻留与可驱逐回填结合的核心缓存策略。 | `packages/core/src/hybrid_cache.rs:1` |
| 平台适配 | Platform adapter | 核心库暴露给 WASM/JNI/宿主环境的抽象接口。 | `packages/core/src/platform.rs:1` |
| 存储抽象 | Storage abstraction | 统一 KV 操作并可装配五种后端的通用接口与工厂。 | `packages/storage/src/lib.rs:56` |
| Redis KV | Redis KV | 带 TTL 的 Redis 字符串 KV 后端。 | `packages/storage/src/redis_kv.rs:8` |
| Postgres KV | Postgres KV | API gateway 账户快照可用的 Postgres KV 写穿后端。 | `packages/storage/src/postgres_kv.rs:1` |
| Redis Streams | Redis Streams | 支持消费组、ACK、pending、DLQ 的摄入消息后端。 | `packages/storage/src/redis_streams.rs:23` |
| Redis 限流器 | Redis rate limiter | 以 Redis 维护每身份配额的分布式限流组件。 | `packages/storage/src/rate_limit.rs:1` |
| ClickHouse 存储 | ClickHouse storage | 市场数据批量写入与导出的列式历史后端。 | `packages/storage/src/clickhouse.rs:1` |
| Timescale 存储 | Timescale storage | 行情镜像与时间序列查询的 Postgres 时序后端。 | `packages/storage/src/timescale.rs:1` |
| Parquet 数据湖 | Parquet lake | 按分区写 Parquet、维护 manifest 和 compaction 的历史层。 | `packages/storage/src/lake.rs:1` |
| 对象存储 | Object storage | S3/MinIO 连接串、上传与取证产物后端。 | `packages/storage/src/cloud.rs:1` |
| 采集器 | Collector | 管理任务状态、执行、事件、健康、发布与 HTTP 管理面的进程。 | `services/collector/src/main_simple.rs:21` |
| 采集任务模板 | Collection task template | YAML/JSON 到 TaskDefinition 的声明式任务 schema 与校验器。 | `services/collector/src/task_templates/schema.rs:40` |
| 采集数据源 | Collection source | 东财、新浪、腾讯、网易等具体取数实现。 | `services/collector/src/sources/mod.rs:1` |
| 多语言爬虫 | Multilang crawler | Python/Node/Go/Rust/Shell 任务的进程执行器。 | `services/collector/src/multilang_simple.rs:1` |
| 源健康台账 | Source health ledger | 由执行结果推导 degraded/down 三态健康状态。 | `services/collector/src/source_health.rs:1` |
| 数据引擎 | Data engine | 消费流、验证规范、执行 SQL/指标、落多后端数据的计算服务。 | `services/data-engine/src/main.rs:190` |
| 热序列存储 | Hot series store | data-engine 内当前可查序列的进程内存状态。 | `services/data-engine/src/main.rs:253` |
| API 网关 | API gateway | 聚合认证、防滥用、限流、代理和账户同步的 HTTP/WS 边界。 | `services/api-gateway/src/main.rs:152` |
| 账户存储 | Account store | 内存权威、可选 Postgres 写穿的跨端账户与同步状态机。 | `services/api-gateway/src/account.rs:160` |
| JWT/JWKS | JWT/JWKS | 外部 IdP 公钥缓存与本地 JWT 验证的认证组合。 | `services/api-gateway/src/jwt.rs:1` |
| 防滥用盾 | Abuse shield | 扫描、突发和 bot 判定的进程内防护中间件。 | `services/api-gateway/src/shield.rs:1` |
| 实时推送 | Realtime feed | Redis 消费、模拟降级、WS 订阅与版本化扇出进程。 | `services/real-time-feed/src/main.rs:94` |
| 告警 Webhook | Alert webhook | 接收 Alertmanager 事件并适配外部通知渠道的出口。 | `services/alert-webhook/src/main.rs:37` |
| 终端运行时 | Terminal runtime | Web、desktop、mobile、WASM 各端的组合状态与桥接面。 | `web/app/src/App.tsx:34` |
| 浏览器分析引擎 | Browser analysis engine | 编译为 WASM 的行情分析、共享缓冲和流处理入口。 | `wasm-analyzer/src/lib.rs:52` |
| 桌面框架层 | Desktop framework layer | 无 Tauri 依赖的路径、配置、状态、离线与导出逻辑。 | `desktop/src/lib.rs:5` |
| 桌面接线层 | Desktop wiring layer | 把框架层暴露为 Tauri command/window/shortcut 的进程面。 | `desktop/src/gui.rs:1` |
| 移动核心 | Mobile core | 观察列表、分析、通知/同步/离线决策的 UniFFI 状态对象。 | `mobile/src/state.rs:58` |
| 宿主离线仓 | Host offline store | Android 平台壳为核心提供的文件 KV 与快照适配器。 | `mobile/android/app/src/main/java/com/alpha/finance/mobile/OfflineStore.kt:19` |
| 运维配置面 | Operations config surface | 容器、采集模板、观测、告警和合规的声明式入口。 | `docker-compose.yml:1` |
| 观测管线 | Observability pipeline | Prometheus 抓取、Loki 日志和 Grafana 展示的编排。 | `config/prometheus.yml:1` |

