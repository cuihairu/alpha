# 部署 Runbook（Ubuntu 裸机 + Docker Compose）

本文只描述两条已落库、可照抄执行的部署路径：

- 裸机路径：`scripts/deploy-ubuntu.sh`，产 systemd 三单元 + nginx 站点 + ufw 规则，装在 `/opt/alpha`。
- 容器路径：仓库根 `docker-compose.yml`，15 个服务一键起（应用 5 个 + 基础设施 + 可观测栈）。

两条路径都不引入本文未列出的组件。Kafka、NATS、Kubernetes、Go 服务不在本仓栈内，
architecture-review §2.4 已裁定队列升级（Kafka/NATS）不立项；编排以 compose 为界。

## 1. 端口与拓扑

| 服务 | 容器内/裸机默认端口 | 说明 |
|---|---|---|
| api-gateway | 8080（compose）/ 9080（裸机 `--bind`） | `/api/v1/*` 反代 data-engine，`/ws` 反代 real-time-feed |
| data-engine | 8081（compose）/ 9082（裸机 `ALPHA__SERVER__ADDR`） | 行情 REST + SQL 查询 + gRPC(:50051) |
| real-time-feed | 8082（compose）/ 9081（裸机 `ALPHA_REALTIME_FEED_BIND`） | WebSocket 推送 `/ws` |
| collector | 8083 | 任务模板 + cron 调度 + 行情抓取 |
| alert-webhook | 容器 8080，compose 映射 8084 | Alertmanager 转发出口 |
| redis | 6379 | Streams 行情总线 + 缓存 + 限流令牌 |
| timescaledb | 5432 | 时序落库（可选镜像） |
| clickhouse | 8123(HTTP) 9000(TCP) 9004 9005 | 列存导出归档 |
| prometheus / loki / promtail / grafana | 9090 / 3100 / - / 3000 | 指标 + 日志 + 面板 |
| alertmanager | 9093 | 告警路由 |

数据流：collector 发布 `quotes.raw` → data-engine 消费、标准化、写 `quotes.normalized`
（毒消息进 `quotes.dlq`）→ real-time-feed 消费 normalized 推给 WebSocket 客户端。
api-gateway 是唯一对外 REST/WS 入口；collector 的任务管理面（`/tasks`）只在内网暴露。

裸机路径下端口挪到 9080-9082 是为了让 nginx 独占 80 端口；容器路径不改端口。
两个服务（data-engine、real-time-feed）没有 CLI 参数解析，绑址只能走 env，
裸机单元文件里 `ALPHA__SERVER__ADDR` / `ALPHA_REALTIME_FEED_BIND` 就是干这个的。

## 2. 容器路径：docker compose

```bash
cp .env.example .env        # 可调端口与 ClickHouse 账密，默认值即可用
docker compose up -d        # 起全部服务
docker compose ps           # HEALTHCHECK 状态（各应用镜像探 /health）
```

编排细节：

- 应用镜像由 `services/*/Dockerfile` 构建，`docker-compose.yml` 里 `build: context: .` 直连；
  预构建多架构镜像走 `scripts/build-images.sh`（tag 默认 git 短 SHA，见 docs/docker-deployment.md）。
- `web-origin` 容器服 `web/dist` 静态站，上线顺序是先 `cd web && npm run build` 再 `compose up web-origin`。
- 配置挂载：`config/clickhouse-schema.sql`、`config/nginx/web-origin.conf`、
  `config/prometheus.yml`、`config/loki/`、`config/promtail/`、`config/grafana/`、`config/alertmanager.yml`。
- `docker-compose.yml` 内 `ALPHA__*` 键遵守「section 双下划线分隔、键名内单下划线」规则
  （config crate `Environment::with_prefix("ALPHA").separator("__")`）。

初始化 ClickHouse schema：`scripts/clickhouse-init.sh`（deploy 脚本自动调用；
compose 路径下手工跑一次）。

## 3. 裸机路径：deploy-ubuntu.sh

前置：Ubuntu 24.04、sudo、已 clone 的仓库（或让脚本克隆到 `/opt/alpha`）。

```bash
sudo ./scripts/deploy-ubuntu.sh
```

脚本实际做的事，按顺序：

1. 建 `/opt/alpha` 与项目用户（`$SUDO_USER`，缺省 `alpha`）；
2. apt 装依赖，装 rustup、Node 18、pm2、docker-compose v2；
3. `cargo build --release` 编五个服务，`./build-wasm.sh` 编 WASM，
   `web/` 下 `npm install && npm run build` 出 `dist/`；
4. `docker-compose up -d clickhouse` 只起 ClickHouse，跑 `scripts/clickhouse-init.sh`；
5. 写 systemd 三单元：`alpha-api-gateway`（`--bind 0.0.0.0:9080`）、
   `alpha-data-engine`（`ALPHA__SERVER__ADDR=0.0.0.0:9082`）、
   `alpha-real-time-feed`（`ALPHA_REALTIME_FEED_BIND=0.0.0.0:9081`）；
   Redis 连接各服务按 `REDIS_URL` 默认值 `redis://localhost:6379` 自取；
6. nginx 已装则写 `/etc/nginx/sites-available/alpha`（静态站 + `/api/` → 9080 +
   `/ws/` → 9081；未装则跳过，Web 端口 80 无人服务，需自行 `apt install -y nginx` 后重配）；
7. ufw 放行 22/80/443/9080/9081/9082 并启用；
8. 起服务、探 `localhost:9080/health` 与 ClickHouse `:8123/ping`，打印结果。

健康与状态：

```bash
systemctl status alpha-api-gateway alpha-data-engine alpha-real-time-feed
journalctl -u alpha-* -f
curl localhost:9080/health
```

更新：

```bash
cd /opt/alpha
git pull origin main
cargo build --release
sudo systemctl restart alpha-api-gateway alpha-data-engine alpha-real-time-feed
```

## 4. 服务配置面

各服务配置入口不同，没有统一配置文件：

**data-engine**（唯一有 schema 的服务）：`services/data-engine/config`、`Config`
（相对进程 CWD，仓内默认都不存在，全靠 env 覆盖默认值）。schema：
`server{addr,enable_cors,grpc_addr}` / `telemetry{level,json}` /
`data{seed_demo_data,seed_symbols,lookback_days}` /
`storage{persistence_enabled,timescale_url}` /
`clickhouse{enabled,url,database,user,password}` /
`sweeper{enabled,min_idle_ms,interval_secs,max_delivery_count}` /
`security{api_keys}`。env 形如 `ALPHA__STORAGE__PERSISTENCE_ENABLED=true`。

**api-gateway**：clap CLI（`--bind`、`--auth-mode off|jwt`、`--auth-secret`、
`--auth-jwks-url`/`--auth-jwks-refresh-secs`（OIDC 键表，oct 键联调形态）、
`--rate-*` 等），同名 env 兜底（`ALPHA_GATEWAY_*`）。鉴权细节见 docs/auth.md
（jwt 模式下 `/ws` 升级亦需持票：`Authorization: Bearer` 头或 `?token=` 查询参数，
客户端连 `ws://host/ws?token=<jwt>` 即可）。

**collector**：`ALPHA_COLLECTOR_BIND`（默认 `0.0.0.0:8083`）、
`ALPHA_COLLECTOR_TASKS`（任务模板路径；进程未设置即不装载，示例在
`config/collector.tasks.yaml`，docker-compose 默认挂载该示例并置此变量指向它）、
`ALPHA_WORKSPACE_ROOT`、`ALPHA_COLLECTOR_RAW_ARCHIVE_URL`（原始响应归档，
形如 `s3://bucket?endpoint=http://minio:9000&access_key=..&secret_key=..`；
未设置=关闭，默认零行为变化；设置后任务工作目录的 `raw_response.txt`/
`raw_meta.json` 上传对象存储，键布局 `raw/{YYYY-MM-DD}/{task_id}/{文件名}`，
归档失败只记 `alpha_collector_raw_archive_total{result=failed}` 不影响任务）、
`ALPHA_COLLECTOR_RAW_ARCHIVE_PREFIX`（可选，缺省 `raw`）。模板格式见该文件
与 architecture.md §24。

**real-time-feed**：`ALPHA_REALTIME_FEED_BIND`、`ALPHA_REDIS_URL`/`REDIS_URL`、
`ALPHA_LOG_LEVEL`、`ALPHA_CLAIM_MIN_IDLE_MS`/`ALPHA_CLAIM_SWEEP_SECS`/
`ALPHA_CLAIM_MAX_DELIVERY`（孤儿消息认领）、`ALPHA_ALERT_RULES`。

**日志级别**：real-time-feed 走 `ALPHA_LOG_LEVEL → RUST_LOG → info` 显式初始化；
data-engine 走 `telemetry.level`（默认 info）；其余服务 `RUST_LOG` 语义。

测试专用 env（生产不设）：`REDIS_TEST_URL`、`TIMESCALE_TEST_URL`——不设则对应
集成测试打印 skipping 并通过。

## 5. 可观测与告警

抓取目标以 `config/prometheus.yml` 为准：api-gateway:8080、data-engine:8081、
real-time-feed:8082、collector:8083 各自 `/metrics`，10s 间隔。

常用指标：

- 网关：`alpha_gateway_requests_total`、`alpha_gateway_auth_total`、
  `alpha_gateway_rate_limit_total`、`alpha_gateway_shield_total`、`alpha_gateway_audit_total`
- 实时：`alpha_realtime_messages_total`、`alpha_realtime_feed_connected`
- 数据面：`alpha_dataengine_memory_bytes`、`alpha_dataengine_query_duration_seconds`
- 数据质量：`alpha_dataquality_sequence_gaps_total`、
  `alpha_dataquality_sequence_regressions_total`（sequence 断档/回退）、
  `alpha_dataquality_duplicates_total`（去重窗口命中）、
  `alpha_dataquality_invalid_payloads_total`（规范化失败丢弃）、
  `alpha_dataquality_quarantined_total`（解码失败入 DLQ）、
  `alpha_dataquality_price_outliers_total`（单跳价格超阈，阈值
  `ALPHA_DATAQUALITY_OUTLIER_PCT` 缺省 30%）、
  `alpha_dataquality_source_divergences_total`（同刻多源报价背离，容差
  `ALPHA_DATAQUALITY_DIVERGENCE_PCT` 缺省 1%，单源在报期间休眠），见
  architecture-review §5 P2

告警规则 11 条（`config/alpha-alerts.yml`）：GatewayRateLimitExceeded、
GatewayShieldTriggered、GatewayUpstreamUnhealthy、GatewayErrorRateHigh、
DataEngineQueryLatencyHigh、DataEngineMemoryPressure、RealtimeFeedConnectionLoss、
RealtimeFeedMessageGap、CollectorSourceDegraded、CollectorSourceDown（后两条
随 L504 源健康 gauge）、PrometheusTargetDown。链路：Prometheus → Alertmanager
（`config/alertmanager.yml`，分组/抑制/静默在档）→ alert-webhook
（`/health`、`/alerts`、`/alerts/critical`，渠道 env：`DINGTALK_WEBHOOK_URL`、
`WECHAT_WEBHOOK_URL`、`SLACK_WEBHOOK_URL`、`PAGERDUTY_INTEGRATION_KEY`，缺哪个
跳过哪个；通知正文经规则知识库富化——告警名命中即附根因初判与建议动作，
alerting-and-diagnosis §5）。

日志：promtail 按 docker 服务发现收容器日志（`config/promtail/promtail-config.yml`）
推 Loki（`config/loki/loki-config.yml`），Grafana 出 `alpha-services-overview` 面板
（`config/grafana/dashboards/` + provisioning 自动加载）。

## 6. 接口速查

对外与运维端点以各服务 router 为准，这里只列入口方向：

| 面 | 入口 | 文档 |
|---|---|---|
| 行情/查询 REST | api-gateway `/v1/*` → data-engine（`/query`、`/stocks/:symbol/history`、`/instruments` 等） | docs/market-data-api.md |
| WebSocket | api-gateway `/ws` → real-time-feed `/ws`（版本化 Full/Delta 帧） | docs/realtime-sync-protocol.md |
| 任务管理 | collector `GET/POST /tasks`、`GET /tasks/:id`、`DELETE /tasks/:id`（204 幂等；运行中 409）、`POST /tasks/:id/cancel`（清 schedule 不再调度；运行中任务本次跑完后停）、`POST /tasks/:id/execute`（重试语义亦由它承担——对 Failed/Cancelled 任务重跑即 retry）、`GET /stats`、`GET /events`(SSE) | 内网暴露，勿上公网 |
| 源健康 | collector `GET /sources/health`（L504：任务表 ∪ 执行台账，三态 unknown/healthy/degraded/down + 连败计数/最近成功失败/错误文本）；gauge `alpha_collector_source_health{task}`（0/1/2/3），告警 CollectorSourceDegraded（≥2 持续 5m）/ Down（≥3 持续 10m） | 健康由执行结果推导，非模板声明 |
| 鉴权 | api-gateway `POST /auth/token`（jwt 模式） | docs/auth.md |
| 健康/指标 | 四服务 `/health`、`/metrics` | 本文 §5 |

collector 任务管理闭环见上表（cancel/delete 已落，retry 由 execute 承担）；
边界：运行中任务的子进程无法中断（crawler 无 kill 句柄），cancel 只阻止
后续调度。real-time-feed 的 WS 不推任务事件，推的是行情帧与 alerts 通道。

## 7. 故障排查

```bash
# 端口与监听
sudo ss -tlnp | grep -E '8080|8081|8082|8083|9080|9081|9082'

# 服务日志（裸机）
journalctl -u alpha-data-engine -n 100 --no-pager

# 容器日志
docker logs alpha-data-engine --tail 100

# 毒消息积压（DLQ）
docker exec alpha-redis redis-cli XLEN quotes.dlq

# 积压的 pending（认领 sweeper 每 30s 扫一轮）
docker exec alpha-redis redis-cli XPENDING quotes.raw data-engine

# ClickHouse
curl http://localhost:8123/ping
```

常见形态：

- WS 连不上：先探 real-time-feed `/health`，再确认 nginx `/ws/` 指向的端口
  （裸机 9081，容器走网关 `/ws`）。
- `/query` 500：多为 DataFusion 表注册问题，看 data-engine 日志中 register/deregister 行。
- 指标断流：Prometheus `up == 0` 对应 target 不可达，检查容器端口映射或防火墙。
- 镜像写失败重试缓冲溢出：日志出现 mirror retry 计数，检查 Timescale
  （`ALPHA__STORAGE__TIMESCALE_URL`）连通性；未启用持久化时该告警不应出现。

## 8. 边界与已知项

- **TLS 服务内已接线**：api-gateway 以 `--tls-cert`/`--tls-key`（env
  `ALPHA_GATEWAY_TLS_CERT`/`ALPHA_GATEWAY_TLS_KEY`）启用服务内 rustls TLS
  （axum-server），两把钥匙须同给——只给其一按部署配置错误拒绝启动（静默降级
  明文比拒绝启动更贵）；都缺省保持反代终结形态。裸机 nginx 只配了 80 端口，
  443 需自行补证书或改走网关服务内 TLS。
- **Cargo.lock 不入库**：容器与裸机构建每次解析最新兼容依赖，构建不完全可复现。
  锁文件入库策略归质量保证统筹（docker-deployment §5 登记）。
- **单节点为界**：compose 是开发/测试栈，无副本、无编排级资源限额；
  生产加固项（镜像按 SHA 出库、secrets 注入、限额）登记在 docker-deployment §4。
- **数据备份**：仓内无自动化备份脚本，Timescale/ClickHouse 卷自行 `docker run --rm
  -v ... pg_dump` 或宿主机方案；这是运维缺口，不是遗漏。
- **ClickHouse 账密默认 admin/admin123**：仅限内网演示，公网部署先改
  `config/clickhouse-users.xml` 与 `.env`。
