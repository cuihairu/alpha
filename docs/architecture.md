# Alpha 平台方案

## 系统目标
- 以自建内网集群为主，持续采集 A 股公开/免费的行情、公告、财报、新闻与舆情等数据。
- 提供统一、低延迟、可订阅的 API（REST/WebSocket 走网关，gRPC 由 data-engine 直接提供）；
  通过 Cloudflare Tunnel 暴露给外部客户端为规划项，当前未配置。
- 架构需要可扩展（轻松增加数据源）、易回放（可追溯原始数据）、并具备完善的监控与告警能力。

## 总体架构
```
Crawler (Rust/多语言模板) -> Redis Streams -> Rust Processor -> TimescaleDB/ClickHouse/Redis
                                                   -> 对象存储后端 (packages/storage，原始归档已接线)
                                        -> API Gateway (Rust) -> Cloudflare Tunnel（规划）
```

### 功能分层
1. **采集层**：负责调度爬虫、访问免费数据源、解析结构化数据。
2. **消息层**：第一阶段使用 Redis Streams 解耦采集与处理，提供轻量持久化和消费位点能力。
3. **处理层（Rust）**：验证、补全、计算衍生指标并写入各类型存储。
4. **服务层（Rust）**：统一对外 API/订阅接口，进行鉴权与限流。
5. **运维层**：CI/CD、监控、告警、隧道等非功能支撑。

## 关键模块
### 1. 采集调度
- **任务描述**：以 YAML/JSON 定义每个数据源（URL、请求参数、解析策略、刷新频率）。已落地：`services/collector/src/task_templates.rs`（装载/校验/转 `TaskDefinition`），`ALPHA_COLLECTOR_TASKS` 指向模板文件或目录即启动装载，示例见 `config/collector.tasks.yaml`。
- **刷新频率执行面**：`services/collector/src/cron_scheduler.rs`（5/6 段 cron 解析 + 每秒扫描派发，`schedule` 到期自动执行，执行中不重入）；模板校验与调度执行共用同一解析器。执行链路复用 HTTP `POST /tasks/:id/execute`。
- **执行引擎**：多语言执行器（Python/Node/Go/Rust/Shell，`multilang_simple.rs`），默认走模板解析器（json/database 等）；Playwright 未接入。
- **抗封策略**：UA/Headers 每请求轮换（`sources/ua_rotation.rs`，UA 池 + Accept-Language 池，四个 A 股源接线，`CrawlerConfig.user_agent` 显式给定时不轮换）；单代理可配（`CrawlerConfig.proxy`，四源建 Client 时注入）；请求级重试（`send_with_retry`，网络错/5xx/429 按 `retry_times` 次数上限重试，退避走 `CrawlerConfig.backoff_strategy`，health_check 探测除外）。代理池/多级限流（`rate_limiter.rs`：ProxyPool/TokenBucket/SlidingWindow）与任务级重试退避、派发抖动（`BackoffStrategy` 60s 封顶、`dispatch_jitter_ms` 默认 250ms）均为 lib 组合面未接生产链路；cron 执行链（上条）当前单发、无任务级重试。
- **源健康面**：由执行结果推导三态（unknown/healthy/degraded/down，
  连败阈值 3，`services/collector/src/source_health.rs`），`GET /sources/health`
  + gauge `alpha_collector_source_health`（review §2.2 SourceDefinition
  收敛方向的 health_check 位——落法为推导而非声明，避免双源真相）。
- **产出**：结构化 JSON 写入 Redis Streams（`quotes.raw` 等）；Protobuf 仅用于 data-engine 的 gRPC 面。

### 2. 消息队列
- 当前默认方案：Redis Streams（stream：`quotes.raw`/`quotes.normalized`/`news.raw`/`announcements.raw`/`quotes.dlq`；实际在用的是 quotes 三流，公告/新闻域未实现）。
- NATS JetStream / Kafka 为阶段化预留，architecture-review §2.4 已裁定不立项
  （吞吐、审计、回放需求明确前不升级）。
- 当前实现已支持 consumer group + ack，异常消息进入 dead-letter stream。
- 当前 stream envelope 采用 Envelope v2（`packages/protocols/src/events.rs`）：v1 十字段
  （含 `source`、`version`、`ingest_ts`、`payload_hash`、`symbol`、`payload`）+ 加性字段
  `source_event_id`/`market`/`event_time`/`process_time`/`sequence`/`trace_id`，便于去重与审计。

### 3. 数据处理/ETL（Rust）
- **消费者**：当前使用 Redis Streams 轮询/消费组；队列升级不立项（见 §2）。
- **校验**：Schema 校验、字段缺失补全、异常值（价格\<0等）隔离到 DLQ
  （`quarantine_invalid` → `quotes.dlq`，非数据库表）。
- **衍生计算**：查询态指标（SMA/RSI/Bollinger 等）已实现；复权价、行业/概念映射、
  资金流归集为规划项，无代码。
- **批处理**：每日/每周任务（复权因子校准、行业分类同步）为规划项，无调度实现。

### 4. 存储
- **TimescaleDB/PostgreSQL**：K 线、tick、指标的可选镜像（`storage.persistence_enabled`）；
  压缩与分区策略未配置（当前仅 `create_hypertable`）。
- **ClickHouse**：parquet 导出归档（`/clickhouse/export.parquet` 等两个导出端点，无
  INSERT 写路径）；公告/新闻/舆情主仓与倒排/全文索引为规划项。
- **Redis**：热点缓存、限流 token、去重锁。
- **对象存储（MinIO/S3）**：`packages/storage` 后端已实现，collector
  `RawArchiver` 原始响应归档已接线（`ALPHA_COLLECTOR_RAW_ARCHIVE_URL`
  门控，默认关）。

### 5. API 服务
- **网关**：Axum，REST（`/api/v1/*` 反代）+ WebSocket 反代 + 健康/指标面；网关不终结
  gRPC，gRPC 由 data-engine（tonic，`:50051`）直接提供。
- **鉴权**：JWT（HS256 自签 / OIDC）+ RBAC（viewer/operator/admin），配额限频走
  RedisRateLimiter；data-engine 另有 X-Api-Key 门。均默认关闭。
- **实时推送**：WebSocket 订阅 topic（如 `quotes.{symbol}`），内部使用发布/订阅。
- **查询层**：内存时序 + 可选 Timescale 镜像；ClickHouse 只读用于导出，不参与查询。

### 6. 监控与运维
- **指标**：Prometheus + Grafana，观测爬虫成功率、队列积压、API 延迟、数据库资源。
- **日志**：集中到 Loki（promtail 抓取），按 `trace_id`/`source` 关联。
- **告警**：Alertmanager -> 钉钉/企业微信/Slack/PagerDuty（`services/alert-webhook`）。
- **CI/CD**：GitHub Actions 打包 Rust 多目标二进制与 Docker 镜像；内网服务器使用 systemd 或容器编排。
- **安全**：Cloudflare Tunnel 暴露网关为规划项；内部服务仅限内网访问；TLS 缺省由
  反代终结，api-gateway 亦可以 `--tls-cert`/`--tls-key` 启用服务内 rustls TLS
  （见 deployment-runbook §8）。

## 数据流
1. 调度器触发采集任务，爬虫访问数据源并写入 Redis stream，如 `quotes.raw`。
2. `data-engine` 读取 `quotes.raw`，做最小标准化后写入 `quotes.normalized`。
3. 实时推送和查询层优先消费 `quotes.normalized`，持久化服务再落表并更新缓存。
4. API 服务从存储/缓存读取数据，按请求格式返回或推送。
5. 监控系统实时收集各组件指标，触发自动化告警。

## 技术选型摘要
- **语言**：Rust（核心处理、API、调度器）；采集模板支持 Python/Node/Go/Rust/Shell。
- **通信**：Redis Streams（当前，唯一在用）；NATS/Kafka 不立项（review §2.4）；
  gRPC + Protobuf（data-engine）；HTTP/JSON（网关与 REST 面）。
- **存储**：TimescaleDB/PostgreSQL、ClickHouse、Redis；MinIO/S3（collector
  原始归档已接线）。
- **其他**：Grafana/Prometheus/Loki、GitHub Actions。

## 部署策略
- 单机 PoC：Docker Compose（15 服务：应用 5 + TimescaleDB/ClickHouse/Redis +
  Prometheus/Grafana/Loki/promtail/Alertmanager/alert-webhook + web-origin）。
- 生产：多节点（采集节点、处理节点、存储集群）为规划口径；systemd 单机为当前唯一
  生产路径（`scripts/deploy-ubuntu.sh`），K8s 不立项；Cloudflare Tunnel 部署在 API
  节点为规划项。
- 灰度能力：API 通过路由实现 v1/v2 并行；Kafka topic 版本号随队列不立项而作废。

## 后续路线
1. ✅ 完成 Redis Streams 消息 envelope 与 stream 命名约定（Envelope v2，0af6fe0）。
2. ✅ 初始化 Rust 模块（`services`：collector、data-engine、api-gateway、real-time-feed、
   alert-webhook；`packages`：protocols、storage、core）。
3. ✅ 构建采集框架（任务模板、代理池、调度）。
4. 上线最小可用数据集（指数/主板行情 + 公告）——公告域未实现。
5. 补齐监控与自动告警（告警 11 条规则已上线，含 L504 采集源健康两档）；结合 Cloudflare Tunnel 发布外部访问
   地址（未配置）。
