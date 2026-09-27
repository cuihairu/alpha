# Alpha 平台现状分析（2026-09-27）

> 范围：main @ e88de7e。目标：盘点各模块完成度、端到端管线真实状态、测试与部署现状，
> 并给出按价值排序的行动清单（见 §5，已同步到 TODO.md）。

## 1. 模块完成度

| 模块 | 完成度 | 状态 |
|---|---|---|
| packages/core（指标/策略/分析） | 高 | 纯计算库，自带测试，质量尚可 |
| packages/storage（KV/时序/Streams/ClickHouse/Timescale） | 中高 | 后端齐全；Redis/Timescale 集成测试用 `REDIS_TEST_URL`/`TIMESCALE_TEST_URL` 环境变量门控 |
| packages/protocols（REST/WS/gRPC proto） | 中 | proto + 类型定义可用；REST 模块与网关未实际串联 |
| services/collector | 中 | `SimpleCollector`：任务 CRUD + SSE + 多语言脚本执行；`/streams/quotes/publish` 用 Rust `EastmoneySource` 抓行情并写 `quotes.raw`；大量**未挂载的死模块**（见 §4） |
| services/data-engine | 中高 | Axum REST + DataFusion `/query` + ClickHouse Parquet 导出 + gRPC；内置 Redis Streams normalizer（`quotes.raw`→`quotes.normalized`） |
| services/real-time-feed | 中 | 消费 `quotes.normalized`（回退 `quotes.raw`）→ WebSocket 广播；无效消息进 `quotes.dlq`；Redis 不可用回退模拟数据 |
| services/api-gateway | 低 | **纯 stub**：health 返回硬编码健康状态，proxy 返回 mock，无真实转发/鉴权 |
| wasm-analyzer + web | 中 | WASM 引擎 + 前端自洽（demo 数据 + Canvas），**未接任何后端 API/WS** |
| desktop (Tauri) | 低 | 骨架；本机编译需 libsoup-2.4 等系统库（CI 在 macOS 单独跑，Linux 本地需 `--exclude alpha-desktop`） |
| 部署（docker-compose/CI） | 中低 | compose 服务齐全但环境变量/端口多处错位（见 §3.3） |

## 2. 端到端管线真实状态

```
crawler(python, 手工) ──(不进管线)──┐
collector /streams/quotes/publish ──┴→ quotes.raw → data-engine(normalize) → quotes.normalized → real-time-feed → WS
                                                ↘ 内存 TimeSeriesStorage（重启即失）        ↘ quotes.dlq
```

- **可跑通的部分**：collector 用 Rust `EastmoneySource` 抓 Eastmoney 行情（payload 含 `symbol/price/volume/change/change_percent/bid1/ask1/open/high/low`）→ data-engine normalizer 消费（要求 `symbol/price/volume`）→ 写内存时序 + 转发 `quotes.normalized` + ack。
- **断点 1（bug）**：`real-time-feed` 的 `envelope_to_realtime` 把 `change`/`change_percent` 当必填；而 data-engine 转发的 normalized payload 是 `MarketData` 序列化（**没有** change/change_percent）→ **所有 normalized 行情都被打进 `quotes.dlq`，WebSocket 端拿不到任何来自管线的行情**。这正是「管线看似通、实际断」的核心点。
- **断点 2（丢失语义）**：data-engine 转发时只保留 `MarketData` 字段，collector 原始 payload 的 `name/pre_close/amount/source/change*` 全部丢失，normalized stream 无法回溯审计。
- **断点 3（不落地）**：data-engine `storage.persistence_enabled`/`timescale_url` 配置项存在但**从未被使用**——`AppState` 永远用内存 `TimeSeriesStorage`；TimescaleDB/ClickHouse 只在导出/演示层出现，处理结果不落库。
- **旁路**：`crawlers/python/eastmoney_quote.py` 只被 collector 的通用任务执行器（`/tasks/:id/execute`）调用且**结果不进 stream**；真正进管线的只有 Rust 直采路径。

## 3. 测试与部署现状

### 3.1 测试
- 基线（本机，`cargo test --workspace --all-targets --exclude alpha-desktop`，与 CI test job 同口径）：**编译即失败** —— `desktop` 依赖 `soup2-sys` 需要 libsoup-2.4（已按 CI 口径排除后重验）。
- 各 crate 自带单元测试；storage 层集成测试门控于 `REDIS_TEST_URL`/`TIMESCALE_TEST_URL`，本机 Redis 可用（redis-server PONG + redis-test 容器）→ **管线级集成测试有条件做真实 Redis 验证**。
- `tests/clickhouse_test.rs`、`src/main.rs`（仓库根）是**孤儿文件**：根 Cargo.toml 是纯 virtual workspace，无 `[package]`，这两个文件从不参与编译。
- 没有任何跨服务的端到端测试（raw→normalized→DLQ 语义无人守护——断点 1 正是因此漏网）。

### 3.2 CI
- `ci.yml`：clippy + test（`--exclude alpha-desktop`）+ desktop(macOS) + wasm 构建 + cargo audit。口径基本自洽。

### 3.3 部署错位（会直接导致「起了但不对」）
1. collector 二进制绑 `0.0.0.0:3000`，compose/Dockerfile/dev-start 全按 **8083**（EXPOSE、healthcheck、端口映射、文档）→ 容器健康检查必挂。
2. compose 给 real-time-feed / collector **没传 `REDIS_URL`** → 容器内默认 `localhost:6379`，real-time-feed 静默回退到模拟行情、collector 发布必失败。
3. compose 给 data-engine 的 `CLICKHOUSE_URL` / `ALPHA__STORAGE__*` 与 settings 的 `ALPHA__CLICKHOUSE__*` 前缀不匹配 → ClickHouse 实际始终 disabled（默认 false 且无 ALPHA__CLICKHOUSE__ENABLED）。
4. collector Dockerfile 的 `COPY packages/ services/collector` 不含其余 workspace 成员目录，workspace 解析会失败（镜像实际构建不出来）。

## 4. 代码卫生
- collector 内 `crawler_discovery.rs`、`data_sources.rs`、`distributed_crawler.rs`、`integrated_crawler_demo.rs`、`multilang.rs`、`multilang_simple_old.rs` **未被 lib.rs 声明**（约 5000 行死代码，参与不了编译也测不到）。
- `main_simple.rs` 内 `/tasks/:id/execute` 走 Python 子进程路径，与 `/streams/quotes/publish` 的 Rust 直采路径**并存且语义分裂**（前者结果丢弃）。
- README/PROJECT_SUMMARY 与实际（端口、Python 爬虫地位、「已完成」表述）有多处出入。

## 5. 行动清单（按价值排序，已同步 TODO.md）

### P0 打通并守住端到端管线（本轮实施）
| # | 事项 | 验收标准 |
|---|---|---|
| 1 | 修断点 1：data-engine normalized payload 保留原始字段（合并而非替换）；real-time-feed 对 `change/change_percent` 缺省容错 | 集成测试：真实 Redis 上 publish → normalize → realtime 解析成功，`quotes.dlq` 无新增 |
| 2 | 新增管线集成测试（`REDIS_TEST_URL` 门控，沿用 storage 层约定）：RedisStreamQueue publish/read_group/ack + raw→normalized→realtime 全链路断言 | 本机 Redis 下测试绿；无 Redis 时自动 skip |
| 3 | `normalize_quote` 抽为可测纯函数并补单测（非法 payload→None，bid1/ask1 映射） | 单测绿 |

### P1 修部署口径（下一轮）
| # | 事项 | 验收标准 |
|---|---|---|
| 4 | collector 端口统一为 8083（改为 env 可配，默认 8083） | 二进制默认绑 8083，compose/Dockerfile/文档一致 |
| 5 | compose 补 `REDIS_URL`（real-time-feed、collector）、`ALPHA__CLICKHOUSE__ENABLED/URL`（data-engine） | compose config 检查通过；服务日志显示真实消费 |
| 6 | 修 collector Dockerfile workspace 拷贝（整仓 COPY 或仅拷所需成员） | `docker build services/collector` 成功 |

### P2 数据落地（后续）
| # | 事项 | 验收标准 |
|---|---|---|
| 7 | data-engine 按 `storage.persistence_enabled`/`timescale_url` 装配 `TimescaleTimeSeriesStorage`，normalizer 双写（内存 + 落库失败降级告警） | TIMESCALE_TEST_URL 集成测试：normalized 行情可从 Timescale 查回 |
| 8 | normalized payload 补 `ingest_ts→bar` 归档与去重（payload_hash） | 重复发布同 payload 不产生重复点 |

### P3 补对外链路（后续）
| # | 事项 | 验收标准 |
|---|---|---|
| 9 | api-gateway 真实反代 data-engine（8081）/real-time-feed（8082），去掉 mock | 集成测试经网关取 `/stocks/:symbol/history` 与 WS |
| 10 | web 前端接 real-time-feed WS + data-engine REST（可配置 base URL） | 浏览器展示管线行情而非 demo 数据 |
| 11 | 清理 collector 死模块与根目录孤儿文件（`src/`、`tests/`） | `cargo build` 覆盖率与 lib.rs 声明一致 |

## 6. 风险提示
- `patches/arrow-arith` 本地补丁 + datafusion 35/arrow 50 的版本耦合较脆，升级需整体验证。
- normalizer 每条消息 `refresh_query_tables()` 全量重建 MemTable，行情放大后是 O(N²) 热点（记入 P2 优化项）。
- api-gateway health 返回假健康状态，会对运维掩盖真实故障——P3 落地前建议先改为聚合真实 /health。
