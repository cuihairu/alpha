[English](README.md) | [中文](README.zh.md)

# alpha

内部 A 股市场数据平台，专注于低延迟采集、清洗与分发可自由获取的公开数据。定位：**自托管金融数据与量化研究底座**（采集 → 标准化 → 存储 → 查询 → 实时分发 → 指标/分析 → 回测）；范围边界见 `docs/architecture-review.md`。当前实现以 Rust 为主，队列采用分阶段策略：从 Redis Streams 起步，可随规模增长演进到 NATS JetStream 或 Kafka。  
中文架构方案说明见 `docs/architecture.md`。

## 目标
- 从公开/免费来源采集股票数据（行情、公告、财报、新闻、舆情），按源设置请求间隔节流。
- 对时序数据（行情、指标）与文档型数据（公告、新闻）进行标准化、增强与存储。
- 为内部工具与潜在下游量化管线提供一致的 API 与 WebSocket 流。
- 完全运行在自托管局域网环境；对外发布（如 Cloudflare Tunnel）为规划项，尚未配置。

## 总体架构
1. **数据采集**  
   - Rust collector（`services/collector`）为默认实现：YAML/JSON 任务模板 + cron 调度器 + UA/Accept-Language 每请求轮换 + 请求级重试（带抖动指数退避）。  
   - `crawlers/python/` 存放标准库参考脚本（东财行情抓取）；模板执行器支持多语言执行（Python/Node/Go/Rust/Shell）。  
   - 代理池组件以库级组合面形式存在，未接入生产链路。

2. **消息队列层**  
   - 第一阶段以 Redis Streams 作为采集与消费解耦的低成本默认实现。  
   - 第二阶段在多消费者持久化与回放成为刚需时，可迁移至 NATS JetStream。  
   - Kafka 仅在数据域与审计/回放需求证明其成本合理后，才作为高吞吐选项。  
   - 当前在用流：`quotes.raw`、`quotes.normalized`、`quotes.dlq`。公告/新闻域尚未实现。

3. **处理服务**  
   - Rust 服务消费流消息，处理校验、schema 映射与增强（价格标准化、查询态指标）。  
   - 存储写入目标：
     - TimescaleDB/PostgreSQL 存 tick/日 K（可选镜像，默认关闭）。
     - ClickHouse 用于行情 Parquet 导出/归档（数据管线尚无写入路径）。
     - Redis 承担热缓存、去重锁与限流令牌。
   - 批量重算目前在 data-engine 内部（`/query` 执行前刷新 MemTable）；无独立 ETL 作业。

4. **对内/对外 API**  
   - `services/api-gateway` 提供 REST（`/api/v1/*` 反代）+ WebSocket（`/ws`）+ 健康检查/指标。  
   - gRPC 由 data-engine 直接提供（`:50051`），不经网关。  
   - 鉴权（默认全部关闭）：data-engine X-Api-Key 门、网关 JWT（HS256 自签 / OIDC）+ RBAC、Redis 限流。  
   - Web UI 为行情/分析看板；监控看板在 Grafana。

5. **部署与网络**  
   - Rust 二进制静态构建，经 docker-compose 或 systemd 运行（`scripts/deploy-ubuntu.sh`；快速指南：`README_DEPLOYMENT.md`）。  
   - 内网承载整条管线；仅经 Cloudflare Tunnel 发布 API 网关为规划选项。  
   - CI/CD 走 GitHub Actions（lint/test/wasm/build/e2e/security + release 流水线）。

6. **可观测性与运维**  
   - Prometheus 抓取四个服务的 `/metrics` 端点。  
   - Grafana 看板（`config/grafana/`）覆盖采集时延、队列积压、API 时延；Loki 收集容器日志。  
   - Alertmanager + `alert-webhook` 按告警规则（`config/alpha-alerts.yml`，11 条）通知钉钉/企业微信/Slack/PagerDuty。

## 技术决策
- **核心语言**：Rust 用于处理服务、调度器与 API。
- **爬虫**：默认 Rust collector；`crawlers/python` 作即席参考脚本。
- **队列**：默认 Redis Streams，规划了到 NATS JetStream 与 Kafka 的升级路径。
- **数据库**：TimescaleDB/PostgreSQL + ClickHouse（仅导出）+ Redis（缓存/限流）。S3/MinIO 存储后端已在 `packages/storage` 就绪；其上的原始响应归档写路径已实现（`RawArchiver`，env 门控，默认关闭）。
- **Schema 定义**：队列/REST 用 serde JSON（Envelope v2，加性演进）；Protobuf 仅用于 gRPC 面。

## 路线图
1. 定义 Protobuf schema 与主题契约。
2. 在引入更重的消息中间件之前，保持 Redis Streams 管线精简可靠。
3. ✅ 建设采集框架（调度 + UA/指纹轮换）——经 collector 任务模板（YAML/JSON 数据源定义）+ cron 派发器 + 带抖动退避的请求级重试实现。
4. 建立可观测性栈与隧道配置。
5. 扩充数据集覆盖（沪深两市、港股延伸、另类数据）。
