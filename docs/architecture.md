# Alpha 平台方案

## 系统目标
- 以自建内网集群为主，持续采集 A 股公开/免费的行情、公告、财报、新闻与舆情等数据。
- 提供统一、低延迟、可订阅的 API（REST/gRPC/WebSocket），通过 Cloudflare Tunnel 暴露给外部客户端。
- 架构需要可扩展（轻松增加数据源）、易回放（可追溯原始数据）、并具备完善的监控与告警能力。

## 总体架构
```
Crawler (Py/Go) -> Kafka/NATS -> Go Processor -> TimescaleDB/ClickHouse/Redis
                                                -> Object Storage (MinIO)
                                      -> API Gateway (Go) -> Cloudflare Tunnel
```

### 功能分层
1. **采集层**：负责调度爬虫、访问免费数据源、解析结构化数据。
2. **消息层**：解耦采集与处理，提供缓存、回放和多消费者能力。
3. **处理层（Go）**：验证、补全、计算衍生指标并写入各类型存储。
4. **服务层（Go）**：统一对外 API/订阅接口，进行鉴权与限流。
5. **运维层**：CI/CD、监控、告警、隧道等非功能支撑。

## 关键模块
### 1. 采集调度
- **任务描述**：以 YAML/JSON 定义每个数据源（URL、请求参数、解析策略、刷新频率）。
- **执行引擎**：Python（requests/Playwright/asyncio）为主，部分场景可选 Go 或 Node.js。
- **抗封策略**：UA/Headers 轮换、可插拔代理池、随机延迟、失败自动重试。
- **产出**：遵循 Protobuf Schema 的结构化 JSON，写入 Kafka/NATS。

### 2. 消息队列
- 推荐 Kafka（topic：quotes/announcements/news/financials/fundflow/...），开启压缩与保留策略。
- 轻量部署可选 NATS JetStream，适合单节点或低延迟需求。
- 所有消息均携带：source、version、ingest_ts、payload_hash，便于去重与审计。

### 3. 数据处理/ETL（Go）
- **消费者**：使用 sarama/segmentio-kafka-go（Kafka）或 nats.go（NATS）并行消费。
- **校验**：Schema 校验、字段缺失补全、异常值（价格<0等）隔离到 quarantine 表。
- **衍生计算**：复权价、均线、波动率、行业/概念映射、资金流归集。
- **批处理**：每日/每周任务校准前复权因子、同步行业分类、重算指标。

### 4. 存储
- **TimescaleDB/PostgreSQL**：K 线、tick、指标，配合压缩与分区。
- **ClickHouse**：公告/新闻/舆情，利用倒排索引和全文检索。
- **Redis**：热点缓存、限流 token、去重锁、任务租约。
- **对象存储（MinIO/S3）**：落地原始响应与快照，方便回溯。

### 5. API 服务
- **网关**：Go (Gin/Fiber + gRPC)，提供 REST/gRPC/WebSocket 三种接口。
- **鉴权**：API Key + HMAC，可配置配额、限频（Redis）。
- **实时推送**：WebSocket 订阅 topic（如 `quotes.{symbol}`），内部使用发布/订阅。
- **查询层**：对 TimescaleDB、ClickHouse 建立只读连接池，支持分页与多维过滤。

### 6. 监控与运维
- **指标**：Prometheus + Grafana，观测爬虫成功率、队列积压、API 延迟、数据库资源。
- **日志**：集中到 Loki/ELK，按 `trace_id`/`source` 关联。
- **告警**：Alertmanager -> 钉钉/飞书/邮件。
- **CI/CD**：GitHub Actions 打包 Go 静态二进制与 Docker 镜像；内网服务器使用 systemd 或容器编排。
- **安全**：Cloudflare Tunnel 暴露网关；内部服务仅限内网访问；API 强制 TLS、配额/限频。

## 数据流
1. 调度器触发采集任务，爬虫访问数据源并写入 `raw_{type}` topic。
2. Go 消费者按 topic 读取数据，进行校验、衍生计算后写入 `normalized_{type}` topic（可选）。
3. 持久化服务订阅 normalized topic，落表并更新缓存。
4. API 服务从存储/缓存读取数据，按请求格式返回或推送。
5. 监控系统实时收集各组件指标，触发自动化告警。

## 技术选型摘要
- **语言**：Go（核心处理、API、调度器）、Python/Node（爬虫）。
- **通信**：Kafka or NATS、gRPC + Protobuf、HTTP/JSON。
- **存储**：TimescaleDB/PostgreSQL、ClickHouse、Redis、MinIO。
- **其他**：Grafana/Prometheus/Loki、Cloudflare Tunnel、GitHub Actions。

## 部署策略
- 单机 PoC：Docker Compose（Kafka/NATS、TimescaleDB、ClickHouse、Redis、MinIO、Prometheus、Grafana）。
- 生产：多节点（采集节点、处理节点、存储集群）；systemd or K8s；Cloudflare Tunnel 部署在 API 节点。
- 灰度能力：Kafka topic 与数据库 schema 带版本号；API 通过路由实现 v1/v2 并行。

## 后续路线
1. 完成 Protobuf schema & topic 定义。
2. 初始化 Go 模块（`cmd`：crawler-scheduler、ingestion-processor、api-gateway；`pkg`：schema、storage、mq）。
3. 构建采集框架（任务模板、代理池、调度）。
4. 上线最小可用数据集（指数/主板行情 + 公告）。
5. 补齐监控与自动告警，结合 Cloudflare Tunnel 发布外部访问地址。
