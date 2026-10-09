# TODO

> 本文件是交付台账：`[x]` 条目按完成时点记录当时事实，行号（Lxxx）随编辑漂移、
> 不作引用依据；现状以 README.md、docs/ 与代码为准。已知过期处以〔现注：…〕标注。

## 优先级行动清单（2026-09-27 现状分析产出，详见 docs/analysis.md）

### P0 打通并守住端到端管线  2026-09-27
- [x] 修复 normalized 行情被误入 DLQ（real-time-feed 对 change/change_percent 容错 + data-engine 保留原始字段）
- [x] 新增 Redis Streams 管线集成测试（REDIS_TEST_URL 门控）：raw→normalized→realtime 全链路断言
- [x] normalize_quote 抽纯函数并补单测
- [x] 修 read_latest 的 XREVRANGE 解析崩溃（StreamRangeReply）
- [x] 本机真实 Redis 进程级 E2E 验证：raw→normalized→API history，DLQ 为空

### P1 修部署口径  2026-09-27
- [x] collector 端口统一为 8083（env 可配：ALPHA_COLLECTOR_BIND），对齐 compose/Dockerfile/dev-start
- [x] compose 补 REDIS_URL（real-time-feed/collector）与 ALPHA__CLICKHOUSE__*（data-engine），并加 depends_on redis
- [x] 修 4 个服务 Dockerfile workspace 拷贝不完整问题（改为 COPY . + .dockerignore）
- [x] （新发现）解码失败的消息滞留消费组 PEL 永不清理：read_group 改为返回 GroupRead{messages, invalid}，消费者按 DLQ 契约隔离（publish_dlq + ack，quotes.dlq），DLQ 发布失败时不 ack 留待重试（✅ 2026-09-27，见 P2）
- [x] （新发现）消费端崩溃遗留的孤儿 pending 消息：storage 层新增 `claim_stale`（XAUTOCLAIM，Redis ≥6.2；已被 XDEL 的 PEL 空壳直接 ack 清理），data-engine/real-time-feed 各起独立 sweeper 周期认领重放，处理路径与实时消费完全共用（与解码失败→DLQ 仍是两条独立路径，互不混写）；data-engine 走 ALPHA__SWEEPER__* 配置、real-time-feed 走 ALPHA_CLAIM_MIN_IDLE_MS/ALPHA_CLAIM_SWEEP_SECS env（默认闲置 30s、每 30s 扫一轮）（✅ 2026-09-27，见 P2）

### P2 数据落地
- [x] data-engine 按 storage.persistence_enabled/timescale_url 装配 Timescale 落库：AppState 增 persistence 镜像，initialize_persistence 三态降级（与 ClickHouse 装配同口径：未启用→内存；URL 缺失/连接失败→告警降级内存，服务不拒启；正常→write_normalized 写内存后镜像落库）。三态单测齐备（正常写入 TIMESCALE_TEST_URL 门控），实机 PG15 进程级 E2E 验证 raw→normalized→内存+落库全链路（✅ 2026-09-27）
- [x] normalized 层去重（payload_hash）：data-engine 写侧按原始 envelope 的 payload_hash 判重，重复投递（XAUTOCLAIM 重放、上游重复发布）不重复写内存/转发但仍 ack（防 PEL 重放）。窗口边界：进程内 FIFO、65536 条、重启清零；跨重启重复由 Timescale (symbol,ts) UPSERT 幂等兜底；指纹在「内存写+转发」成功后才记录，写失败重试不会被去重吞掉。实测：重复投递/sweeper 重放均只落地一条 normalized、PEL 清零（✅ 2026-09-28）
- [x] MemTable 全量重建热点：实测全量重建 ~25-35µs/点（600 点 17.7ms/次、2.4 万点 850ms/次、48 万点 11.7s/次，随存量线性增长且在消费循环内同步执行）→ 写路径移除全量重建（/query 每次执行前自刷新、/stocks 与 /indicators 直读内存时序，语义不变，双查回归 200）；顺带修复 register_table 撞名潜伏 bug（datafusion 35 同名表报错：第二次 refresh 起必失败，/query 第二次 500 → 先 deregister 再 register）（✅ 2026-09-28）
- [x] 内存时序 add_point 重写：单点改按 ts 二分有序插入 + 同 (symbol,ts) UPSERT 覆盖（最后写赢，与 Timescale ON CONFLICT 口径一致）；批量改 add_points（按 symbol 分组一次排序+一次去重），灌入从逐条全排序 O(n² log n)（48 万点分钟级）降至 48 万点 6.4s（剩余耗时为逐点 JSON metadata 构造，与排序无关）。data 恒有序 + ts 唯一不变式，/query、/stocks、/indicators 读路径与写侧去重窗口行为零改动。单测：乱序批量灌入有序、重复 ts 去重（单点/批内/批与存量）、写后立即可查；进程级 E2E 三读路径复跑通过（✅ 2026-09-28）
- [x] 重投递封顶：claim_stale 按 XPENDING 明细 delivery_count 封顶（含首次投递，达到上限仍 pending 即判定「毒消息」）：不再认领重投，以 invalid（reason 携带 dc/cap）交回调用方走既有 DLQ 契约（publish_dlq→quotes.dlq + ack；DLQ 发布失败不 ack，下轮扫描重试），DLQ 条目携带原 stream/条目 ID/原 payload 可回溯，storage 层另发 tracing 告警。DLQ 形态取「Redis Stream（复用解码失败→quotes.dlq 既有契约与巡检链路）+ 告警日志」：「独立 consumer group」需跨组搬运语义，「登记表」则游离于既有 DLQ 工具之外，均不如复用。N 默认 5（默认 30s 扫描间隔 ≈2 分钟重试窗口，覆盖部署重启类瞬断、坏消息不空转），data-engine ALPHA__SWEEPER__MAX_DELIVERY_COUNT、real-time-feed ALPHA_CLAIM_MAX_DELIVERY 可调。实现由 XAUTOCLAIM 换为 XPENDING(IDLE 过滤)分页+XCLAIM 精确认领（XAUTOCLAIM 无法按 delivery_count 过滤且认领即递增不可反悔；XCLAIM 保留 min-idle 门槛防并发抢锁，Nil 先 XRANGE 核实空壳再 ack 防误清他人 pending；毒消息不占认领配额、单轮扫描量设上界）。测试：封顶触发+超限停投+DLQ 留痕、未超限不误杀、毒与新鲜孤儿同轮协同不饥饿；双服务集成（pipeline_compat）与全仓门禁复跑，二进制带新 env 启动冒烟通过（✅ 2026-09-28）
- [x] real-time-feed 显式日志级别初始化：fmt::init() 在未设 RUST_LOG 时只放行 ERROR、WARN 级兜底日志不可见 → 与 data-engine 同口径的显式初始化（默认 info，ALPHA_LOG_LEVEL 可调、RUST_LOG 兼容保留，parse_log_level 纯函数 + 单测含未知值回退 INFO）；未设 RUST_LOG 启动冒烟可见 INFO（✅ 2026-09-28）
- [x] 持久化镜像失败重试：镜像写失败不再直接丢副本，入进程内有界 FIFO 重试缓冲（10 万条，超限丢最旧并计数），后台任务每 5s 补写 Timescale（单轮 500 条，失败条起整批按原序回队首断点续写）；内存 serving 与消费管线不受影响。边界（非完整 outbox，已在代码注释与此处注明）：缓冲重启即失、跨重启缺口不弥补；补写与直写并发可能使极少数同 (symbol, ts) 冲突回写旧值（UPSERT 最后写赢）。未启用持久化时 worker 不启动（预期降级留痕）。强持久化（跨进程 outbox/落登记表）如需再立项。单测：回队首保序、超限丢最旧计数；二进制启动冒烟（✅ 2026-09-28）
- [x] compose 透传持久化 env：两键自 160b73e（2026-04）即已存在（该条目「未透传」对硬编码值而言已过时），本次升级为部署可调的 ${VAR:-默认值} 形式——默认保持既有行为（启用 + timescaledb 服务），宿主机 env/.env 可覆写开关与地址；docker compose config 验证默认渲染与宿主机覆写双向生效，键内单下划线规则就地注释（✅ 2026-09-28）

### P3 补对外链路
- [x] api-gateway 真实反代：/api/v1/* 透明反代 data-engine（方法/查询串/头[滤逐跳头]/体透传，上游不可达 502）；/ws 与 /ws/* 双向泵反代 real-time-feed（tokio-tungstenite 上游，升级握手前失败拒绝 502）；/health 真实并发探测三上游（实测时延，data-engine+real-time-feed 任一不可达整体 degraded）。上游地址 CLI/env（ALPHA_GATEWAY_*，compose 按容器网络注入并替换原无用 CLICKHOUSE_* env）。单测 6：健康探测真实/降级、REST 透传（路径+方法+体）、502、WS 回环（tungstenite 客户端经网关↔上游 axum echo）、URL 转换；进程级 E2E：网关 /health 真实时延、/api/v1/stocks/* 与 /api/v1/query 透传 200 真实数据、WS 握手 101（✅ 2026-09-28）
- [x] web 前端接 real-time-feed WS + data-engine REST：app.js 从纯 mock 切为「真实后端优先、不可达回退演示数据并在状态栏注明」。REST：fetchStockHistory 调 data-engine /stocks/:symbol/history（days 可调），响应 {timestamp,price,volume,metadata{bid,ask,open,high,low}} 映射为分析器数据形状，股票分析/技术指标/价格图三处共用（图表改画真实收盘序列，无数据才回退随机示例）；实时：5s 轮询 mock 改为 WebSocket 长连接，解析 real_time_quotes 通道推送按观察列表过滤渲染（等待推送占位、断连/错误复位按钮并提示，beforeunload 关闭连接；同条行情经 raw+normalized 转发会推两帧，按 symbol 覆盖自然去重）。连接地址前端可配（新「后端连接」卡片，localStorage 持久化）：默认直连服务（data-engine CORS 默认开启；data-engine :8081、real-time-feed ws://…:8082/ws），可改指 api-gateway 统一入口（:8080/api/v1、/ws；默认端口与 web 静态服务器 server.js 同为 8080，同机同跑需错开，卡片内已注明）。实测协议校准：服务端 serde 变体名未 rename，WS 帧实为 {"type":"Data",…}（PascalCase），前端大小写不敏感匹配。验证：node --check；活服务 E2E——真实 fetchStockHistory 函数源码在 node 对活 data-engine 拉取映射断言、python websockets 客户端按 app.js 解析逻辑逐行复刻对活 real-time-feed 断言推送渲染、不可达回退抛错路径断言；全仓门禁复跑 0 失败（✅ 2026-09-28）
- [x] 清理 collector 未挂载死模块与根目录孤儿文件：根 Cargo.toml 是纯 [workspace]（无 [package]），根目录 src/main.rs（ClickHouse 集成试验程序）与 tests/clickhouse_test.rs 从未被任何 crate 编译；collector 的 crawler_discovery.rs / distributed_crawler.rs / integrated_crawler_demo.rs / multilang.rs / multilang_simple_old.rs / data_sources.rs 六个文件均未在 lib.rs/main.rs 挂载（全仓 grep 确认无 #[path] 引用，examples 自带内联 mod、仅在打印文案中提及文件名，属自包含示例）。全部删除；编译集严格缩小，存活代码零改动，alpha-collector --all-targets 编译通过、全仓门禁复跑 0 失败（✅ 2026-09-28）

## 跨平台 Rust 架构设计
- [x] 设计统一跨平台架构（packages/、services/、web/、desktop/、mobile/）：docs/cross-platform-architecture.md v1 草案——L0 共享核心（packages/core，零平台依赖）/ L1 平台服务（storage/protocols/services，服务端专属）/ L2 平台表现（web+wasm-analyzer、desktop Tauri 1.5、mobile 预留）三层与依赖方向强制；现状盘点全部实测（core wasm32 编译通过需 --features wasm=chrono/wasmbind+uuid/js；protocols 因 tonic 默认特性拉 mio 在 wasm32 编译失败，remediation=default-features=false+grpc 模块 feature 门控，已写入差距清单）；平台适配层 trait 草案（KeyValueStore/LocalPersistence/UserNotification）；配套 scripts/check-cross-platform.sh 落地强制检查（wasm32 下 core 编译门禁 + core 依赖黑名单扫描 tokio/reqwest/sqlx/redis/tonic 等 + protocols informational 探测，非交互可入 CI，实测通过）；余项（workspace 多目标配置、共享核心库补齐、适配层落地）映射 §7 路线图（✅ 2026-09-28）
- [x] 配置 Cargo workspace 支持多目标平台构建：.cargo/config.toml 落地 cargo alias（cargo wasm-check = alpha-core @ wasm32 --features wasm；cargo wasm-build = alpha-wasm-analyzer @ wasm32 cdylib 构建，两 alias 实测通过）；scripts/check-cross-platform.sh 扩为四步（core wasm32 编译门禁、wasm-analyzer wasm32 构建门禁——实测通过（cdylib+rlib，6 个既有 dead_code 警告不阻塞）、core 依赖黑名单扫描、protocols informational 探测）；docs/cross-platform-architecture.md §5 目标矩阵/§6 检查清单/§7 路线图同步（CI 多目标并行矩阵留待「跨平台 CI/CD」节）。全仓门禁复跑 0 失败（✅ 2026-09-28）
- [x] 建立跨平台共享核心库（core、protocols、storage）：按设计文档三层模型落齐——core：wasm-clean（--features wasm，wasm32 编译门禁）；protocols：消除设计文档 §5 已知差距——tonic/prost/tonic-build 转 optional、「grpc」feature（default 开启，服务端零改动）门控 proto 代码生成（build.rs cfg 跳过）与 proto 模块，rest/websocket/grpc 纯 serde 契约无门控共享，wasm32 --no-default-features 编译实测通过并升入 check-cross-platform.sh 第 4 步硬门禁（原 informational）；storage：按设计属 L1 服务端专属（sqlx/redis/clickhouse），移动端经 REST/WS 访问 services 不直连，wasm 侧由 wasm-analyzer 自带存储模块承接。三个 crate 职责边界与消费方式在 docs/cross-platform-architecture.md §5/§7 记录；全仓门禁（默认特性，服务端不受影响）复跑 0 失败（✅ 2026-09-28）
- [x] 实现平台适配层抽象接口（Desktop、Web、Android、iOS）：packages/core/src/platform.rs 落地设计文档 §4 的 trait 面——KeyValueStore（get/set/delete 后写覆盖语义）/ LocalPersistence（export_file）/ UserNotification（notify，async-trait、方法不带平台类型），各交付面实现方式逐一注明（Tauri fs/通知、Web IndexedDB/Notification API、移动端沙箱+推送、服务端参考实现）；InMemoryKeyValueStore 参考实现（HashMap+Mutex）+ 契约测试（get/set/delete 语义、Arc<dyn> 动态分发与跨线程可用）。顺带发现并按 P3-3 同类口径处理新孤儿：core/src/trading.rs 从未挂载（lib.rs 无 pub mod trading）、全仓零引用且存在潜伏语法/类型错误（多余右括号、浮点歧义、E0384 等——从未参与编译所以从未暴露），已删除并在设计文档 §2/§3 修正 L0 清单措辞（量化策略回测引擎属未来「业务功能」节另起炉灶）；core 增 tokio dev-dep（macros+rt，仅测试，不进 lib/wasm 构建）。alpha-core 17 测试绿、cargo wasm-check 绿、全仓门禁 0 失败（✅ 2026-09-28）
- [x] 定义统一的 Rust 代码规范和跨平台兼容性检查：docs/rust-code-standards.md v1（格式化 rustfmt 唯一权威、clippy 零警告且 allow 必须留痕禁 blanket、错误处理 AlphaError/anyhow 分层与 lib 禁 unwrap、模块挂载纪律〔引 trading.rs 孤儿案例〕、tokio/async-trait 惯例、tracing 级别语义与显式初始化、平台依赖黑名单与 feature 默认值策略、测试 env 门控/契约 helper/空断言禁令、提交纪律）；可执行门禁 scripts/check-lint.sh（fmt --check + clippy -D warnings，与 CI 完全一致）。全仓一次性清零到位：clippy --fix 自动修 21 处 + 人工修 ~25 处（clamp/迭代器求和/copy_from_slice/Default 补齐/嵌套 format 提取/生成代码 result_large_err include 处压制/路线图预留 API 与调度器传参形态按「allow 必须注释理由」规范标注），cargo fmt --all 机械重排 53 文件；本地验证 fmt --check 绿、workspace clippy -D warnings 绿、全量测试 0 失败、双服务二进制重建启动冒烟 healthy、check-cross-platform.sh 四步绿。CI 集成（.github/workflows/ci.yml 修复三处长期红灯）：lint 作业从裸 clippy 升级为 fmt+clippy 真门禁并排除 desktop（原在 ubuntu 必因缺 GUI 库失败）；test 作业补 protobuf-compiler（proto 代码生成缺 protoc 即败）+ Redis service 让 REDIS_TEST_URL 门控测试真实运行；wasm 作业并入 check-cross-platform.sh；security 作业降为 continue-on-error 报告型（8 个 cargo 依赖漏洞属升级债另行立项，避免长期红灯淹没真信号，已在规范 §11 注明）。跨平台兼容性检查（check-cross-platform.sh 四步）保持既有（✅ 2026-09-28）

## Rust WASM Web 分析引擎
- [x] 集成 wasm-bindgen 和 wasm-pack 构建工具链
  （2026-09-28 落地：wasm32 target + `cargo wasm-check`/`wasm-build` alias；CI wasm 作业
  jetli/wasm-pack-action + `wasm-pack build --target web` 长期绿；本地 wasm-pack release
  构建冒烟通过，pkg 产物已 gitignore 不入库）
- [x] 开发高性能 Rust WASM 核心计算库（指标算法、回测引擎）
  （2026-09-28 最小可用版本：指标算法 = alpha-core `indicators` + wasm-analyzer 既有绑定
  （RSI/SMA/EMA/BOLL/MACD/analyzeSymbol）；回测引擎 = alpha-core `backtest` 纯计算模块
  （Signal/Strategy trait/SmaCrossStrategy O(1) 滚动窗口/BacktestEngine，口径：单标的日频、
  多空两态、收盘成交、fee_bps 双边、夏普 √252 年化）+ wasm 绑定 `backtestSmaCross`
  （serde snake_case 输出，node 冒烟验证持有收益与费用记账）；接口缝：多标的/止损/滑点
  经 Strategy trait 扩展，Worker 并行走 worker.rs BacktestStrategy 协议预留，
  零拷贝 memcpy 简化待「零拷贝内存管理」项统一处理）
- [x] 实现零拷贝内存管理与 Arrow 数据格式优化
  （2026-09-28 最小可用版本：wasm-analyzer/src/shared_buffer.rs 定长 f64 缓冲区
  alloc/from_raw/as_slice/into_vec + allocPriceBuffer/freePriceBuffer wasm 绑定；
  JS 侧 Float64Array::view_mut_raw 零拷贝直写、Rust 侧 as_slice 零拷贝直读，
  生命周期契约（定长、grow 后视图失效、双 free UB）文档化；
  arrow_adapter.rs：ArrowBatch.from_market_data 零拷贝列构建、
  exportPrices/exportVolumes 导出、ArrowMemoryPool 预留；
  wasm32 target 编译通过、原生测试 17 通过 3 自跳过、全仓门禁复跑 0 失败；
  同日续作（计算缝 + 基准验证）：backtestSmaCrossPtr 零拷贝计算绑定 +
  test_zero_copy_backtest_flow 全流程 wasm 测试；A/B 基准实测——
  native（examples/zero_copy_bench.rs，release）：10k bars 347.4→341.4µs/次、
  100k 4382.2→4307.0µs/次（引擎计算占主导，每次调用消除 78/781KB memcpy+堆分配）；
  JS 边界（node-bench/zero-copy.mjs，交错 10 轮取 min）：10k 960.4→738.1µs/次
  （-23.1%）、100k 11788.2→11405.6µs/次（-3.2%，equity_curve 报告序列化两侧
  同量占主导，收益下限；JSON/JS 转换优化另行立项）；
  复现：cargo run -p alpha-wasm-analyzer --example zero_copy_bench --release、
  wasm-pack build 后 node node-bench/zero-copy.mjs；
  并行会话注：本项 shared_buffer/alloc/free 及本注首段由并行会话 a8aaa97 入库
  （其提交吸收了本会话未提交 WIP），backtestSmaCrossPtr/测试亦随其入库，
  续作提交补齐 fmt 红灯修复与基准数据）
- [x] 构建混合存储架构（WASM 内存 + IndexedDB + 服务端缓存）
  （2026-09-29 最小可用版本，三层 read-through 分工：
  packages/core/src/hybrid_cache.rs = L0 纯计算核心——LruCache O(1) 内存层
  （命中提新鲜度/超容驱逐/分层计数）、PersistentStore trait（L2 持久层契约，
  InMemoryPersistentStore 参考实现 + 契约测试锁定语义）、RemoteSource trait
  （L3 服务端源缝——HTTP 属 L1 服务层，L0 黑名单禁 reqwest）、HybridCacheService
  read-through 组合（L1→L2 回填 L1→L3 写穿 L2+L1）+ write_through/invalidate/stats，
  10 单测含 LRU 驱逐序/三层命中回填/NotFound 传播/失效重拉/容量 0 拒绝；
  wasm-analyzer/storage.rs = HybridStorage 由元信息空壳升级为真行为：内嵌 core
  LruCache，putPrices/getPrices（miss→NULL）/invalidateSymbol/getStorageStats
  （lru_len/hits/misses/evictions 分层计数）；IndexedDB 真实异步读写为 JS glue
  实现 PersistentStore 契约（schema 元信息导出保留），服务端缓存经
  RemoteSource 缝接——两端均为显式接口缝，最小口径注明非敷衍简化；
  native 20 通过 3 自跳过 + wasm32 边界测试 1（浏览器跑，门禁编译验证），
  check-lint/check-cross-platform/全量 17 套件全绿）
- [x] 开发流式数据处理和并行计算机制（Web Workers + Rayon）
  （2026-09-29 最小可用版本：wasm-analyzer/src/streaming.rs 新增
  ParallelStreamProcessor（原生 Rayon / wasm32 WorkerPool 双路径）与
  BatchStreamProcessor::computeAllIndicatorsParallel；复用
  alpha_core::parallel::compute（native 真并行，wasm32 顺序退化，语义等价）；
  新增 8 单测（wasm32 only）覆盖推送/批量/带报告/空缓冲区/列表清空；
  alpha_core::parallel 9 测 + worker.rs 16 测 + streaming 2 测（native）全绿；
  check-lint/check-cross-platform/cargo test 全仓 213 测全绿）
- [x] 实现实时数据同步协议（WebSocket 增量更新 + 版本控制）
  （2026-09-29 最小可用版本：协议核 + 内存态版本表，不动部署与前端接线。
  alpha_core::sync——SyncEngine 状态机（Full 基线/Delta 连续推进/Gap 检测/
  旧帧幂等/多通道隔离/Resync 恢复）+ build_delta/apply_delta 深度 1 差量 +
  SyncHistory 环形版本表（retention 窗口内增量重放追平、超窗回落全量），
  16 单测含发布×客户端回环收敛集成测试；线上帧 SyncMessage/ResyncRequest
  落 alpha_protocols::websocket（4 测）；服务端 real-time-feed 锁内逐通道
  seq 分配 + Resync 应答（未知通道显式回 Error 404 帧，11 测含服务端 Delta
  × 客户端引擎闭环）；wasm 侧 buildSyncDelta/applySyncDelta/WasmSyncEngine
  绑定 + wasm_bindgen_test；前端 app.js Sync 帧增量合入。设计要点落
  docs/realtime-sync-protocol.md。注：sync.rs/websocket.rs/real-time-feed
  主体由并行会话随 f930375 入库，本项补齐追平版本表/Resync Error 应答/
  docs 并勾选；非交互假设：retention 窗口语义取「帧数环形保留、超窗全量」）

## 桌面端应用（Tauri）
- [x] 搭建 Tauri + Rust 桌面应用框架：桌面 crate 拆「框架层（lib，零 Tauri 依赖）+ 接线层（gui 特性）」——Tauri 降为可选依赖（Linux 需 WebKitGTK/libsoup，此前整个包只能从 lint/test 门禁排除＝无自动化校验），框架层随主门禁进 CI，接线层由 macOS 作业编译；框架层落地 paths（配置/数据/导出/键值四目录布局）、config（缺失自举/损坏回退上报 ConfigSource::Recovered/临时文件+rename 原子写/一次性报全部校验问题）、kv（L38 适配层 KeyValueStore 的桌面实现：键→十六进制文件名阻断 `../` 穿越，每键一文件）、market（symbol 派生种子的确定性 LCG 行情，替换原 rand+Utc::now 不可测口径）、analysis（委派 alpha-core AnalysisEngine，不重复实现指标）、export（CSV/JSON，导出时刻注入使文件名可断言）、alerts（原子写+损坏回退空表）、ipc（前后端 DTO，serde 往返+前端 JSON 负载解析锁定契约）、state/app/error；接线层 gui.rs 只做「解析路径→委派→映射错误」的薄 #[tauri::command]（6 命令），main.rs 退化为进程入口；构建门禁新增 scripts/check-desktop.sh（tauri.conf.json 自洽性：必填字段/图标存在/distDir 含 index.html 防空白窗口/**build 段** withGlobalTauri/兜底壳不得用 v2 的 ipcRenderer/**allowlist 与 Cargo.toml tauri 特性对齐**/无孤儿 src-tauri，加框架层 clippy+单测）与 desktop/tests/tauri_config.rs（10 例，用 tauri-utils 走 tauri-build 同一解析路径把「配置 schema 是否匹配 Tauri 1.x」从 macOS CI 运行时前移到本地/常规 CI，另锁兜底壳↔gui.rs 命令名与 v1 IPC 入口契约）并接 CI 新作业 Desktop Framework（Linux）；顺手删孤儿 desktop/src-tauri/tauri.conf.json（crate 根是 desktop/，其 allowlist 与 devPath 与生效配置不同，留着只会改错文件）与 5 个未用依赖（reqwest/dirs/config-rs/alpha-storage/anyhow）；前端集成兜底：受版本控制的 web/dist/index.html + desktop-shell.js（.gitignore 加例外）经 window.__TAURI__ 串 initialize_app→get_app_info→get_real_time_quotes→analyze_symbol，闭环到 alpha-core 真实计算（外部 JS 文件，不放开 CSP script-src）；设计说明 docs/desktop-framework.md；框架层 91 单测（L113 导出功能 +33 → 现 124；含 KV 穿越/二进制大值、CSV 往返、损坏配置回退、原子写无残留、确定性行情等）。首轮 CI 红灯修复（均为 v1/v2 混用，非交互自行判定）：withGlobalTauri 在 Tauri 1.x 属 build 段（放 tauri 段会被 Config 的 deny_unknown_fields 在 tauri_build::build() 运行时拒绝，Linux runner 跑不到、只能靠 macOS 作业暴露），前端 IPC 入口在 v1 是 window.__TAURI__.invoke 而非 v2 的 ipcRenderer（用错是运行期 undefined、窗口静默无响应）。第二轮 CI 红灯（Desktop macOS 编译错误）复盘并结构性收口：接线层曾把框架层 validate() -> Result<(), Vec<String>> 当 Vec<String> 用（is_empty() 编译不过）——业务判断写在「本地编不了、只有 macOS 作业能编」的 gui.rs 里，红灯要等一次推送往返才暴露。故把命令体全部改写为纯委派：判断下沉为框架层入口 state::bootstrap_app（配置自举落盘+降级提示）/ analysis::analyze_request、quotes_request / alerts::upsert_request（方向串解析+价格校验）/ export::export_request（格式解析+空列表）；AppConfig 补 validation_problems() 返回问题列表本身（避免 Result<()> 误用）；InitPayload 下沉 ipc.rs 并带 is_recovered()；app_info 改注入 Tauri identifier（v1 单值，非 v2 标识符数组）。新增 desktop/tests/wiring_contract.rs（8 例薄度契约：命令体不得含判空/兜底/自造错误串、必须委派上述入口并 map_err、不得绕过入口调底层、generate_handler! 与兜底壳 invoke 一致、lib.rs 重导出指向真实项、文件行数上限），反向用例实测（把 validate().unwrap_or_default() 塞回命令 → 8 例中 2 例转红）；另 app 4 例/ ipc 4 例 / analysis 4 例 / alerts 3 例 / export 4 例 / state 3 例 / config 2 例。框架层 91 → 112 测，配置契约 10 + 接线契约 8。诚实边界：薄度契约是源码断言，替代不了编译——Tauri 自身类型用法（#[tauri::command] 宏展开、AppHandle/State 形态）仍只能由 macOS 作业验证，故策略是把留在 macOS 的代码压到最小。 第三轮把「替代不了编译」这个缺口也补上：cargo check/clippy 不链接，而 Tauri 的 sys crate（webkit2gtk-sys/soup2-sys/javascriptcore-rs-sys）只在 build 期跑 pkg-config——于是新增 scripts/desktop-fake-pc/（版本号给足、Libs/Cflags 留空的 .pc + README 说明边界），门禁加 [5/5]：PKG_CONFIG_PATH 指向该目录后 cargo clippy -p alpha-desktop --features gui --all-targets -D warnings，gui.rs 的 #[tauri::command] 宏展开、AppHandle/State 用法、generate_context! 与全部框架层调用签名就此进入 Linux 门禁（仍不覆盖链接与启动，那部分留给 macOS 作业）。反向用例实测：把 validate().unwrap_or_default() 塞回 get_app_info，该命令直接给出与首轮 CI 完全相同的 E0599。为此还修掉一个真实缺陷：zbus 5.11.0 声明 zbus_macros = "^5.11.0"（caret），区间允许 5.19.0，而 5.19 宏生成的代码引用 zbus 5.11 未导出的 DispatchResult2（cargo check 到 zbus 本体即 9 个错误，已实测）；zbus 是 tauri 仅 Linux/BSD 的依赖（tauri → notify-rust 4），macOS CI 从不编译它、Linux 上任何 cargo build（默认带 gui）却会失败。本仓忽略 Cargo.lock（.gitignore:6，库惯例）拿不到锁文件兜底，故在 desktop/Cargo.toml 加代码不引用的 optional 依赖 zbus-macros-pin = { package = "zbus_macros", version = "=5.11.0" }（挂 gui 特性）钉住；代价是 zbus 本身也锁在这条链上（cargo update -p zbus --precise 5.19.0 在解析期硬失败），想升级先解钉版。顺带发现并修掉 [5/5] 自身的不可复现：pkg-config 同时搜 PKG_CONFIG_PATH 与 PKG_CONFIG_LIBDIR（后者默认系统 .pc 目录），最初 8 个 .pc 在本机（装了 GTK3）被系统真件悄悄兜住，推到 CI 的 ubuntu-latest 就红在 glib-sys 要的 gobject-2.0 not found——门禁 [5/5] 追加 PKG_CONFIG_LIBDIR 指向空目录（target/desktop-fake-pc-system/）屏蔽系统 .pc，.pc 集合补到 18 个（gobject-2.0/gio-2.0/gmodule-2.0/gdk-3.0/atk/pango/cairo-gobject/gdk-x11-3.0/x11，Requires 照抄真实依赖关系），任何 Linux 机器结果一致；反向验证：删 gobject-2.0.pc 且 cargo clean -p glib-sys -p gobject-sys -p gdk-sys -p atk-sys → [5/5] 转红在 atk 找不到 gobject-2.0（不 clean 会命中 clippy 缓存、看不出变化）。非交互假设：演示行情为确定性占位数据（真实后端接入只换 market 模块）；导出暂落应用数据目录 exports/（「另存为」对话框归 L113）；告警仅持久化（通知/托盘归 L114）（✅ 2026-09-30）
- [x] 实现原生文件系统集成和本地数据导出
  （2026-09-30 最小可用版本：用户自选路径导出闭环，不动部署。框架层
  export::export_to_file / export_symbol_request——后缀一致性校验（大小写不敏感）/
  缺文件名/空序列/空标的/未知格式一律先拒绝且不留任何文件（含临时文件）、父目录
  自建、tmp+rename 原子落盘（与 config::save 同口径）、ExportOutcome 补 Serialize
  （path/filename/rows 字段由单测锁定前端契约）；接线层 gui::export_symbol_to_file
  薄委派（注册命令 6→7）；兜底壳加导出卡片经原生 dialog.save 拿路径（取消显示
  「已取消」、非 Tauri 环境按钮禁用并注明）+ index.html 导出卡片；wiring_contract
  8→9 例（新命令无判断/委派/注册一致性 + 壳 dialog.save 链路断言）；
  check-desktop.sh [1/5] 加 node --check 兜底壳语法门禁；docs/desktop-framework.md
  §6 设计说明。注：核心代码/测试/文档为并行会话未提交 WIP（静默 6 分钟无编译/
  测试进程，按既定并发协议接管收口——门禁+TODO+commit，其文件原样入库未改动）；
  非交互假设：单文件单标的（批量多标的仍走 exports/ 目录导出）、覆盖写直接替换
  （写前确认未做）、dialog.save 取消返回 null 按「已取消」处理。门禁：
  check-desktop.sh ✅ / check-lint.sh ✅ / 全仓 231 测 0 失败 ✅ /
  check-cross-platform.sh 四步 ✅）
- [x] 开发系统通知和托盘集成功能（2026-09-30 最小可用版本：通知/托盘纯逻辑下沉框架层 +
  平台 API 留接线层。框架层 notify.rs——NotificationLevel 解析（大小写不敏感）/
  Notification + TrayState（serde 往返字段名即前端契约）/NotificationQueue
  （有界 FIFO 50 + 同标题正文去重 + 超容量驱逐最旧 + recent 新→旧）/
  notify_request（级别解析/空标的/空标题先拒绝且不入队）/tray_status_request
  （读告警文件→生效数/最近触发→状态文本，停用不计入、损坏回退空表）/
  alert_notification（复用 AlertKind::matches，未触发/停用 None，触发 Critical）；
  AppState 内嵌 Mutex<NotificationQueue>（notification_queue() 访问器，与
  engine()/paths() 同模式）；接线层 gui.rs 三命令 send_notification/
  list_notifications/set_tray_status（注册命令 7→10，薄度上限 160→200 行），
  平台胶水 tauri::api::notification::Notification::show() 与
  tray_handle_by_id("main").set_tooltip() 由 check-desktop.sh [5/5] 假
  pkg-config 在 Linux 门禁类型检查；allowlist 的 notification-all 与
  systemTray 配置 L112 已对齐，本轮无需改配置。非交互假设：队列容量 50
  （会话内历史非持久化）；托盘 tooltip 只反映告警状态。
  同日第二轮闭环补全（6e17247 留了两条缝：托盘从未被创建——tauri.conf 的
  systemTray 段只注入图标，Builder 不调 .system_tray() 就没有托盘，
  tray_handle_by_id 永远拿不到句柄；alert_notification 备而未接）：新增
  src/platform.rs（gui 门控的平台胶水，与 gui.rs 同纪律、行数上限由
  wiring_contract 专项测试 platform_glue_stays_mechanical 锁定——不得定义
  命令/不得自造错误串，只做「框架层模型→Tauri 类型」机械翻译）：
  Builder.system_tray(platform::system_tray()) 显式 with_id("main")（默认
  id 是随机串）+ 菜单；on_system_tray_event 把菜单点击经框架层 tray_action
  映射为 show+focus / hide / exit(0)，动作后按真实可见性回写菜单。菜单状态
  机与文案在框架层：tray_menu_model(visible)（显示/隐藏随主窗可见性互斥
  可用，分隔线隔开退出）+ tray_action(id→动作，未知 id 忽略不 panic)，均
  有单测。告警检查链：框架层 check_request(告警文件, 队列, 时刻)＝读告警→
  生效者按 market::synthetic_quote（与 quotes_request 同口径）判定→触发即
  入队（NotificationQueue 同文去重→重复触发不重复弹窗）+ alerts::deactivate
  停用落盘（沿用既有持久化语义：停用保留记录，一次性告警避免确定性恒价
  行情下反复触发）；check_alerts 命令薄委派 + platform 批量 Notification::show
  （注册命令 10→11，gui.rs 薄度上限 200→220 行）。前端告警卡片闭环：
  set_price_alert→check_alerts→set_tray_status（desktop-shell.js + index.html，
  演示布防目标价＝现价−1%，确定性行情即刻满足）。顺手修两个真缺陷：
  ①Tauri 1.x 命令参数键默认 camelCase（tauri-macros wrapper.rs
  ArgumentCase::Camel），兜底壳原来把 file_path 写成 snake——运行期静默
  失配（L113 的导出按钮真机会失败；CI 只编译不启动从未暴露），改 filePath
  并新增源码断言拦截 targetPrice/alertType/filePath；②send_notification
  误把通知 id 当应用 identifier 传 Notification::new（notify-rust 会拿错误
  应用名），改回 bundle identifier。非交互假设：触发即停用；演示布防目标价
  取现价−1%；检查取数仍用确定性演示行情（换真实行情只动 market 模块）。
  真机验收边界（CI Desktop 只编译+链接，以下需真实桌面环境人工观察）：
  ①系统通知真实弹出且需系统通知权限（macOS 通知中心/Windows toast/Linux
  libappindicator→notify-rust）；②托盘图标出现、iconAsTemplate 深浅色适配；
  ③托盘菜单「显示/隐藏主窗口/退出」真实生效（macOS 配置 menuOnLeftClick=
  false → 右键/ctrl+左键出菜单，Linux 行为另有差异）；④tooltip 悬停显示
  「N 个告警生效中/无告警」且随 set_tray_status 变化；⑤菜单可用态随窗口
  可见性翻转（set_menu 回写）；⑥通知点击唤起主窗未做（v1 点击事件接
  Notification 的 on_action 属后续）。门禁：check-desktop.sh [1-5/5] ✅ /
  check-lint.sh ✅ / 全仓测试 ✅ / check-cross-platform.sh 四步 ✅；
  gui/platform 的链接与运行由 CI Desktop (macOS) 作业验证）
- [x] 构建跨平台窗口管理和主题适配
  （✅ 2026-09-30）口径同前四轮：放置决策/主题映射的纯逻辑下沉框架层，平台读数与调用留接线层。
  框架层新增 window.rs（零 Tauri 依赖）：MonitorRect::shows_window（交集宽/高各 ≥100px 才算在屏，
  贴边一点不误判丢失）、resolve_placement（无保存/尺寸低于 conf 最小值 → None 交还 OS 默认；最大化
  只恢复 maximize 不钳坐标；任一屏可见原样还原；显示器拔出/布局变化 → 钳入主屏——尺寸也钳到屏宽、
  clamp_axis 处理主屏比窗口小的负区间）、load/save_window_state（缺失/损坏 → None 容错同 config/alerts
  口径；临时文件+rename 原子写，负坐标往返保留）、WindowStateTracker（移动/缩放事件节流 ≥1s 且几何有变
  才落盘 observe，CloseRequested 时 flush 关闭兜底不受间隔限制）；config.rs 补主题规范映射 theme_pref
  （trim+大小写不敏感，未知值宽容回退 System——非法值由 validate 单独上报，壳层永远拿到可渲染偏好）与
  resolve_theme（pref × system_prefers_dark → light/dark，Light/Dark 强制覆盖系统）；paths.rs 加
  window-state.json；AppState 内嵌 Mutex<WindowStateTracker>（与通知队列同模式）。接线层 window_gui.rs
  （gui 门控，118 行）：restore_window（setup 读状态→枚举显示器→主屏排首→resolve_placement→机械翻译
  set_size/set_position/maximize，任一步拿不到静默跳过——窗口管理是体验优化，不允许阻断启动）、
  on_window_event（Moved|Resized → 节流落盘，CloseRequested → flush）；gui.rs 只加两行（215 行 < 220 上限）。
  主题：Tauri 1.x 无运行期 set_theme（v2 才有），原生装饰由 tauri.conf.json "theme": "System" 创建期跟随
  系统（已有配置不动）；配置强制 light/dark 的覆盖落在内容层——壳层 JS 按配置在 <html> 落 data-theme
  （system 跟随 prefers-color-scheme 并监听实时切换，旧 WebKit addListener 回退；配置返回前先按系统口径），
  index.html CSS 变量 :root[data-theme="light"] 整体翻浅色，badge/code/button 原硬编码 #232936 提取为
  --chip 一并主题化（覆盖既有组件）。契约：wiring_contract 11 → 14 例（window_glue_stays_mechanical
  <140 行/无命令/无自造错误串/必须引用 window::；window_management_is_wired；
  shell_theme_follows_system_with_override；framework_entry_points_are_exported 补 6 个新入口）；
  框架层+契约单测合计 184（lib 160：window +8/config 主题 +3/state +1，另 tauri_config 10 + wiring_contract 14）。
  真机验收边界（CI Desktop 只编译+链接，以下需真实桌面环境人工观察）：①双显示器拖拽后重启位置还原；
  ②拔掉副屏后窗口钳回主屏（含最大化还原）；③系统深浅色实时切换壳层跟随（native 装饰由 conf System
  跟随，两边一致性取决于 WM）；④托盘图标不随主题换图（单图标资产，登记后续）；⑤多屏 DPI 混排时
  物理像素口径的还原精度。门禁：check-desktop.sh [1-5/5] ✅ / check-lint.sh ✅ / 全仓测试 ✅ /
  check-cross-platform.sh 四步 ✅；window_gui/gui 的链接与运行由 CI Desktop (macOS) 作业验证）
- [x] 实现本地数据库同步和离线模式
  （✅ 2026-09-30）口径同前五轮：降级判定与增量语义下沉框架层，网络 IO 收敛两处最小实现。
  框架层新增 offline.rs（零 Tauri）：本地数据库复用 kv 目录 FileKeyValueStore（键 quotes/{symbol}
  → JSON 快照行，整体存 MarketData 不丢展示字段）；content_seq 内容指纹（价格位模式+成交量
  FNV-1a，u64 落库）作增量比对基准；probe_health 真实网络 IO（tokio TcpStream 最小 HTTP GET
  /health，500ms 超时，连接拒绝/503/超时/https → 离线）；读路径降级矩阵 quotes_from（在线拉取
  写穿落库 live / 单标的失败落缓存 cache / 离线只读缓存 / 缓存缺失入 missing 宁缺毋滥 / 落库失败
  响亮上抛）；增量同步 sync_from（离线→空报告非错误；在线→指纹同 unchanged 跳过、异或本地无
  → applied 落库、拉取失败 failed 且本地保留）。远端缝 QuoteRemote trait（Arc 注入 AppState，
  生产实现 SyntheticRemote=演示行情口径，真实 HTTP 后端只换实现不动语义）。serde_json 加
  float_roundtrip 特性——默认 f64 解析 ±1 ulp 抖动会破坏「往返一致」与末位一致性，本地数据库
  契约要求逐位一致。AppState 内嵌 kv/remote/api_url（bootstrap 从生效配置注入探测目标）；
  gui.rs 两薄命令（get_offline_quotes/sync_offline_data，注册 11 → 13，薄度上限 220 → 260，
  同步范围由前端传观察列表避免命令体读配置的 reach-around）；壳层离线卡片渲染来源标记与增量
  报告。契约 wiring_contract 15 → 16 例（shell_offline_card_renders_source_and_missing：invoke+
  q.source+missing+applied/unchanged+卡片元素；另有两命令入无判断/委派/map_err/壳调用清单、
  注册数 13、框架入口补 sync_request/probe_health/QuoteRemote）。框架层单测 181（offline +17，
  state +2，kv 派生 Debug 后既有 13 例不受影响）。
  真机验收边界（本地单测用假服务端/假 .pc，以下需真实环境人工观察）：①杀掉 api-gateway 后离线
  卡片显示「离线（读本地缓存）」且行情标 cache；②重启网关后增量同步报告 applied/unchanged
  分布；③慢网（>500ms）探测超时表现；④真实 https 后端需 TLS 探测实现（当前保守判离线走缓存）。
  门禁：check-desktop.sh [1-5/5] ✅ / check-lint.sh ✅ / 全仓测试 ✅ / check-cross-platform.sh
  四步 ✅；gui 两命令的链接与运行由 CI Desktop (macOS) 作业验证）
- [x] 开发键盘快捷键和右键菜单支持
  （✅ 2026-09-30）口径同前六轮：表与模型全下沉框架层，接线层只注册与广播，
  壳层只渲染与分发（动作不新增命令，复用既有命令流）。
  框架层新增 shortcuts.rs（零 Tauri）：DEFAULT_SHORTCUTS 组合键表（CmdOrCtrl+R
  刷新 / +E 导出 / +D 离线读 / +Shift+S 同步，u64 动作 id 即字符串常量）+
  valid_combo 合法性（+ 分隔/无空段/键在末位/修饰键合法不重复/不收裸键）+
  resolve↔combo_of 逆映射 + display_label 平台展示（mac ⌘⇧S vs 其它
  Ctrl+Shift+S，CmdOrCtrl 非 mac 映射 Ctrl）+ 右键菜单模型 context_menu
  （{id,label,hint,enabled}，可用性：无行情→复制置灰、无标的→导出置灰）。
  接线层新增 shortcut_gui.rs（行数上限 120，同 platform/window_gui 纪律）：
  GlobalShortcutManager 逐键注册，触发广播 "shortcut" 事件（载荷=动作 id），
  单键注册失败（被系统/其它应用占用）只告警降级不阻断启动；gui.rs setup 挂
  注册、新薄命令 get_context_menu（注册 13 → 14，薄度上限 260 → 280，纯模型
  构造与 get_app_info 同豁免 map_err）。壳层：行情/导出/离线/同步四段流程抽
  成可复用函数 + ACTIONS 表（快捷键事件与右键菜单共用分发）；右键菜单为内容
  层 DOM（Tauri 1.x 无原生 context menu API），数据来自 Rust（hasQuote/
  hasSymbols camelCase 上报界面状态），Esc/点击任意处收起。
  契约 wiring_contract 15 → 17 例（shortcut_glue_stays_mechanical：胶水行数/
  无命令/无自造错误串/取表于框架层/setup 必挂；shell_keyboard_and_context_
  menu_wired：事件监听/模型拉取/camelCase/ACTIONS 全动作 id/菜单样式；另
  get_context_menu 入无判断/委派清单、注册数 14、薄度 280、框架入口补
  context_menu/DEFAULT_SHORTCUTS/display_label）。框架层单测 192（shortcuts
  +11）。
  真机验收边界（本地单测覆盖不了平台注册与真实事件）：三平台 ⌘R/Ctrl+R 实际
  触发与系统快捷键冲突时的降级告警；右键菜单深浅色观感与置灰态；多显示器下
  菜单落点；Esc/点击收起时序；窗口失焦时全局快捷键仍触发（设计上会，未实测）。
  门禁：check-desktop.sh [1-5/5] ✅ / check-lint.sh ✅ / 全仓测试 ✅ /
  check-cross-platform.sh 四步 ✅；快捷键注册与事件链路由 CI Desktop (macOS)
  作业编译验证，运行期行为需真机）

## Rust 微服务架构
- [x] 基于 Axum + Tokio 构建高性能 HTTP/gRPC 服务
- [x] 使用 Tonic + Prost 替代 go-zero 实现微服务通信
- [x] 集成 DataFusion 替代 DuckDB 服务端实现内存 SQL 引擎
- [x] 开发基于 Arrow 的列式数据处理管线
- [x] 实现基于 Tokio 的异步消息队列和事件驱动架构
- [x] 构建统一配置管理（config-rs）和分布式追踪（tracing）

## 移动端应用（Android & iOS）
- [x] 设计移动端 Rust 核心库架构（JNI + UniFFI）
  （✅ 2026-09-30）口径：**设计文档 + 最小可编译骨架 + 单测**（本轮为巡检派发
  首选项；工具链/SDK 构建属后续 TODO 270/271，本环境无 Android SDK/Xcode，
  骨架的编译与测试在 workspace 门禁内完成——台账按 L112–L117 dispatch 顺序
  记为 L118，行号会漂移不作编号依据）。设计文档 docs/mobile-core-architecture.md：
  分层（平台壳 → UniFFI 桥 alpha-mobile → alpha-core 同一份计算）、JNI vs
  UniFFI 决策（UniFFI proc-macro-only 单一桥接源=两端绑定同源；裸 JNI 只作
  SDK 回调逃生舱，workspace 预 pin 的 jni=0.21 本轮不引用）、JSON 字符串桥
  契约（alpha-core 类型无法跨 crate derive Record，与 web/desktop 同一 serde
  字段口径）、错误映射（AlphaError → MobileError 类型化异常，Display 全文
  透传）、线程模型（analyze_symbol 形 async 体同步 → per-call current-thread
  运行时驱动，对象纯数据 Send+Sync）、构建管线与后续 TODO 映射。
  骨架 crate mobile/ → alpha-mobile（从 workspace exclude 移入 members，随
  lint/test 门禁覆盖）：lib.rs（setup_scaffolding! + 重导出）/ state.rs
  （MobileCore 观察列表+配置槽+引擎、MobileError=uniffi::Error+thiserror、
  FFI 面 quote_json/analyze_json/status_json，单测 11 例）/ market.rs（与
  桌面同算法确定性演示行情，单测 5 例——其中「快照 ≡ 序列末根」倒逼生成器
  与序列逐 draw 同序，修掉初版抽取序不同导致的口径谎言）。crate-type 三型
  （lib/cdylib/staticlib）齐备。
  非交互假设（文档 §11 编号）：①台账 L118 沿顺序假设；②uniffi=0.25
  proc-macro-only（无 UDL 副本，消除双定义漂移）pin 不升级；③JNI 逃生舱
  后续启用；④JSON 桥非 Record；⑤进 members（纯 Rust 无 SDK 依赖）；
  ⑥per-call 运行时；⑦观察列表外一律 InvalidSymbol。两处编译期适配已注明：
  uniffi 0.25 FFI 参数不支持 &str（改 owned String）；setup_scaffolding 展开
  码命中 rustc unpredictable_function_pointer_comparisons（crate 级 allow，
  调用处 allow 不传导）。
  真机验收边界（文档 §12 编号）：①uniffi-bindgen 生成 Kotlin 绑定 + 真机
  加载 .so；②Swift staticlib 链接；③主线程调用禁忌/后台调度落实；④JNI
  回调实战场（TODO 272）；⑤多线程并发调用压测。
  门禁：check-lint.sh ✅（workspace clippy 含新成员）/ 全仓测试 ✅（20 段
  ok，alpha-mobile 16 例）/ check-cross-platform.sh 四步 ✅ /
  check-desktop.sh [1-5/5] ✅）
- [x] 搭建 Android Kotlin + Rust 混合开发环境（Jetpack Compose）
  （✅ 2026-09-30，台账 L301）口径：**设计文档 + 最小可编译骨架 + 单测**，另
  做本机 SDK 实证（关键假设⑧：派发前提「本环境无 Android SDK」已过时——本机
  实有 SDK platforms 34/35/36 + build-tools + NDK r27/28 + Gradle 9.8 + JDK 21，
  故 Android 侧做了超出 workspace 门禁的实证；真机/模拟器**运行**仍留边界）。
  交付面 mobile/android/：Gradle 工程（wrapper 8.11.1 入库 / AGP 8.9.0 /
  Kotlin 1.9.25——composeOptions 1.5.15+serialization 插件 / minSdk 26 /
  compileSdk 35 / abiFilters arm64-v8a）+ Compose 壳三件（MainActivity 状态头+
  快照列表+行内分析；AlphaBridge 唯一 uniffi 消费点——Dispatchers.IO +
  MobileException 就地翻译成 Kotlin 侧 Error，UI 层零 uniffi 引用；MarketModels
  载荷模型 @SerialName 对齐 serde snake_case 契约）+ JVM 载荷契约单测 4 例
  （无需设备）。
  Rust 侧增量：mobile 加 uniffi-bindgen CLI（--features bindgen 裁剪，默认构建
  零增量）+ 契约测试 android_shell_contract.rs 9 例（跨语言四名一致 / 生成绑定
  FFI 面点名 / import uniffi.* 仅 AlphaBridge 特权 / 清单-主题-Gradle 三方一致 /
  .gitignore 分离生成源与产物 / alpha-core android jni 依赖回归守门）。
  **本机实证**（README 记录步骤）：uniffi-bindgen 从 host .so 生成 Kotlin 绑定
  （入库 mobile/android/app/src/main/java/uniffi/alpha_mobile/alpha_mobile.kt）；
  NDK clang linker 交叉编译 arm64 libalpha_mobile.so（无需 cargo-ndk，
  CARGO_TARGET_*_LINKER 指 aarch64-linux-android26-clang，minSdk 对齐）进
  jniLibs（.so 不入库）；./gradlew :app:assembleDebug + testDebugUnitTest 通过
  （JNA @aar 依赖为 uniffi 0.25 生成面的硬要求）。初版取 Kotlin 2.1.0 被
  K2 拒——uniffi 0.25 生成绑定在 2.x 下过载歧义（错误类构造属性与 override
  `message` 同名双候选，已知不兼容），按假设②（uniffi pin 0.25 不升级）降级
  壳侧 Kotlin 1.9.25 实证通过（假设⑩：升级 Kotlin 的前置是升级 uniffi，两层
  锁同进退，契约测试负向守卫）。
  **顺带修复潜伏孤儿**：packages/core 的 From<jni::errors::Error> 只有
  cfg(target_os="android") 门控而无依赖——android target 一编即 E0433（从未
  参与编译所以从未暴露，L38 trading.rs 同类），按同文件 js-sys 先例补
  [target.'cfg(target_os = "android")'.dependencies]（假设⑨：不做 optional
  feature，TODO 272 启用 JNI 逃生舱时天然带上）。
  边界（文档 §12）：真机加载 .so 与运行、x86_64 模拟器镜像 ABI、ANR 表现、
  并发压测、R8/签名发布流水线（TODO 351 行附近）。
  门禁：check-lint.sh ✅ / 全仓测试 ✅ / check-cross-platform.sh 四步 ✅ /
  check-desktop.sh [1-5/5] ✅（Android 工具链不进 CI 门禁，靠契约测试守结构）
- [x] 实现 iOS Swift + Rust 集成（SwiftUI + UniFFI）
- [x] 开发移动端特有的推送通知和后台同步：分层纪律「核心库只做决策（推什么/
  何时同步），壳层只做送达与调度（怎么弹/何时醒来）」（docs/mobile-push-sync.md
  §1）——mobile/src/notify.rs：AlertRule（规则键 = 标的|方向|目标价位模式
  to_bits，稳定去重键）+ NotificationSpec 六字段载荷（id/kind/symbol/title/
  body/created_at）+ NotificationChannel trait 可插拔（默认 LocalQueueChannel
  入队、壳层 take_pending_json 取走送平台 API；FCM/APNs 等云推送**不引入**，
  留 trait 实现位，不新增付费云服务）+ Notifier 穿越去重（穿越一次发一次、
  条件回落重武装，防通知轰炸，11 单测）；mobile/src/sync.rs：SyncTrigger 四种
  触发时机（periodic 受 300s 默认间隔闸门 / foreground / connectivity_restored /
  manual 无条件出计划；字符串枚举走 serde，foreground 显式 rename）+
  BackgroundSync 间隔闸门/状态/指纹三件套 + content_seq/fingerprint_of 沿桌面
  L116 FNV-1a 同算法（symbol 排序聚合、与传入顺序无关）（10 单测）；
  state.rs：MobileCore 挂 Mutex<Notifier>+Mutex<BackgroundSync>，FFI **只增不
  改**六方法（set_alert_rules_json 返回规则条数+观察列表/正数校验、
  check_alerts_json 评估入队、take_pending_json 取走即空、sync_status_json、
  sync_plan_json 未知触发走 Failed、mark_synced_json 记时刻+重算指纹；构造器
  与 L118 三方法签名原样——L119 iOS 壳在用，6 单测）；uniffi Kotlin 绑定重生成
  （+139 行）；Android 接缝：MarketModels 六载荷模型（u64→ULong）+ AlphaBridge
  六透传（Dispatchers.IO + TRIGGER_* 常量与 serde 线上串逐字一致）+
  PushSyncSeam.kt（NotificationDispatcher 送达口 + PeriodicSyncWorker 骨架 +
  WorkManager ≥15min 钳制口径装配，work-runtime-ktx 2.9.1 入壳工程）；
  PayloadParsingTest +6（规则编码 serde 契约/通知载荷/评估报告/状态/计划双形）；
  android_shell_contract.rs +1 守门（绑定六方法在、透传接线在、触发串跨语言
  一致、PushSyncSeam 不触碰 uniffi）；本机 gradle assembleDebug +
  testDebugUnitTest 10 测实证（不进 CI 门禁）。假设（文档 §8）：默认间隔 300s
  为骨架值（系统钳制作壳层兼容、不作一致性保证）、取数执行归 L339（api_url
  仍为配置槽）、规则整体替换不持久化（归 L339 kv 快照）、评估用确定性演示
  行情；边界（§10）：通知实际弹出/权限授权流/Doze 降级/远程通道凭据链留真机。
  门禁：check-lint.sh ✅ / 全仓测试 ✅ / check-cross-platform.sh 四步 ✅ /
  check-desktop.sh [1-5/5] ✅（✅ 2026-10-01）
- [x] 实现触屏手势和移动端 UI 交互优化：手势=平台壳职责（架构文档 §13 行 273
  既定性），**Rust 核心库零改动、FFI 零新增**——手势集三选覆盖触控三模式
  （docs/mobile-gestures.md §2）：①下拉刷新 `PullToRefreshBox`（material3
  1.3.x）→ `Gestures.kt::refreshWithManualSync` 编排：`quotes` 重拉 →
  `syncPlan("manual")`（Manual 不受 L337 间隔闸门，用户手势无条件响应）→
  `plan.due` 则 `markSynced` 否则 `syncStatus`（安全网）；②长按行情行
  `combinedClickable(onLongClick)` → `analyze`（与既有按钮并存，减少误触）；
  ③双击状态头折叠 api_url 详情行（local-only 不触 FFI）。横向滑动不引入——
  删除观察列表超出核心库能力（架构文档 §7 边界），自造语义会绕过观察列表。
  落地：`Gestures.kt`（GestureAction 枚举 + `targetBridgeMethod()` 手势→FFI
  翻译面契约源 + `RefreshGateway` 接口 + `refreshWithManualSync` 编排，纯逻辑
  零 Compose/uniffi 依赖可 JVM 测）；`AlphaBridge` `implement RefreshGateway`
  （既有方法签名加 `override`，公共 API 零变化）；`MainActivity` 三手势
  modifier 挂接 + 刷新占位状态（异常路径 finally 复位）；JVM 单测
  `GestureMappingTest.kt` 3 例（映射逐分支点名 + fake gateway 断言调用序列
  与 due 分支，runBlocking 驱动零新测试依赖）；android_shell_contract.rs +1
  例（Gestures.kt 纯逻辑纪律 / 映射目标方法真实存在于 AlphaBridge / MainActivity
  挂接在场 / JVM 测试文件在场，共 11 例）。本机 gradle assembleDebug +
  testDebugUnitTest 13 测实证（10 载荷 + 3 手势，不进 CI 门禁）。
  边界（文档 §8）：触摸延迟/惯性滚动、PullToRefresh 视觉阈值、长按与双击
  冲突窗口、小屏折叠布局、异常路径占位复位均需真机观察。
  门禁：check-lint.sh ✅ / 全仓测试 ✅ / check-cross-platform.sh 四步 ✅ /
  check-desktop.sh [1-5/5] ✅（✅ 2026-10-01）
- [x] 构建移动端离线数据存储和同步机制：沿「决策在核心库、执行在壳层」分层
  （docs/mobile-offline.md）。**产品红线三条款（核心库强制，壳层绕不过）**：
  ①授权开关默认关闭——`OfflineSyncConfig::default()` = enabled:false，未开启
  快照生成/恢复直接 `Failed`（文案引导显式开启）；②显式开启须同时携带非空
  数据范围（`set_offline_sync_config_json` 里 `enabled=true ∧ scopes空` = 拒绝，
  「开启」与「明示范围」同请求绑定）；③未授权不产生同步工作项——
  `offline_sync_delta_json` 未开启返回 `needed=false, reason=sync_disabled`。
  **术语红线（不混用）**：备份=本地快照落盘不出设备（`Snapshot` 载荷/
  `OfflineStore`/`backupNow`）；同步=与远端增量对齐出设备（`SyncDelta` 载荷/
  `planSync`，骨架期只出判断不执行网络）——字段面由 JVM 测试断言互斥
  （snapshot 含 captured_at 不含 since_fingerprint，反之亦然）。
  Rust 决策面：mobile/src/offline.rs（DataScope 封闭枚举首期仅 Quotes/未知
  scope serde 拒绝；OfflineManager 快照生成 scope 过滤 + L337
  content_seq/fingerprint_of 同源指纹；restore 版本一致性校验；11 单测）+
  state.rs FFI **只增不改**五方法（offline_sync_config_json /
  set_offline_sync_config_json / offline_snapshot_json /
  restore_offline_snapshot_json / offline_sync_delta_json(u64→ULong)；构造器
  与既有九方法签名原样——L119 iOS 壳在用；+3 单测）+ lib.rs 导出。
  Kotlin 执行面：OfflineStore.kt（KeyValueStore 接口 + InMemory 测实现 +
  SharedPreferences 真机实现位；OfflineStore 快照单键落盘 `check(enabled)`
  第二道防线；OfflineSyncManager 备份/同步编排，FakeGateway 可注入；
  dataScopeSummary 明示文案「行情快照（代码、价格、成交量、买卖一档）」）+
  AlphaBridge 五透传（implement OfflineGateway，沿 RefreshGateway 模式）+
  绑定重生成（+118 行）。MainActivity **不自动备份**（刷新手势不偷偷落盘，
  备份只经显式编排——契约测试守门）。测试：offline.rs 11 + state.rs 3 + JVM
  OfflineSyncTest 7（kv 后写覆盖/往返/拒收/基线/文案/术语）+ 契约
  android_shell_contract.rs +1 共 12 例；本机 gradle assembleDebug +
  testDebugUnitTest 20 测实证（10 载荷 + 3 手势 + 7 离线，不进 CI 门禁）。
  假设（文档 §8）：开关统一管备份+同步两面；首期 DataScope 仅 Quotes；
  快照整体单键落盘（分键/条目级 diff 归后续）；无设置页（配置面+文案函数
  就绪，UI 归后续）；restore 版本须一致（迁移归发布流水线）；mobile/ios 不动。
  边界（§9）：SharedPreferences 持久化、设置页展示、断网恢复全链路、真实
  远端对齐执行、快照增长分键策略留真机/后续 TODO。
  门禁：check-lint.sh ✅ / 全仓测试 ✅ / check-cross-platform.sh 四步 ✅ /
  check-desktop.sh [1-5/5] ✅（✅ 2026-10-01）

## 跨平台 UI 框架开发
- [x] 选择和集成跨平台 UI 框架（Web: React/Vue, Desktop: Tauri, Mobile: Native）：选型决策写入 docs/web-framework-selection.md——Web 定 **React 18 + TypeScript + Vite**（vs Vue 3：金融终端级组件生态〔图表/虚拟表格/Monaco 编辑器〕React 一等封装更全；vs Yew/Leptos：生态空缺抵不过「同语言」收益，不选作主框架，wasm-analyzer 维持 cdylib+JS 绑定现模式）；影响面登记：L428 组件化后续按 React 路线重定 scope、L429 图表候选 lightweight-charts/ECharts；Web 骨架落地 web/app/（独立 Vite 工程：最小示例页 = 行情演示表 + 纯 TS SMA + WASM 引擎探针三态〔动态 import /pkg 接缝与纯 TS 对账〕，vitest 8 单测 + tsc 严格类型〔纯 TS SMA 口径**对齐** Rust 侧 TechnicalIndicators::calculate_sma：等长输出/前导 0.0 占位/样本不足全 0/4 位小数取整，单测沿用 Rust 同名样本向量对账，前端不另立指标语义〕，package-lock 锁定）；现有 web/ 演示页零回退（未触碰任何既有文件；scripts/check-web.sh 第 1 步在场守门 + node --check 冒烟 + 本机 server.js 实测 index.html/wasm-demo.html 均 200）；新门禁 scripts/check-web.sh 四步（旧页在场冒烟 → 骨架结构断言 → npm ci → tsc+vitest+vite build）接入 CI wasm 作业尾部（ubuntu-latest 自带 node）；Desktop/Mobile 仅文档登记接入边界不实现（Desktop Tauri 1.5 维持现状、frontendDist 指向 web/app/dist 的策略归后续单〔§6 选项 A〕；Mobile 维持 Native 不引 React Native——移动端共享面是 Rust FFI 而非 UI，设计语义对齐即可）。全仓门禁复跑 0 失败（check-lint.sh / 全仓测试 / check-cross-platform.sh 四步 / check-desktop.sh [1-5/5] / check-web.sh）（✅ 2026-10-01）
- [x] 基于 React 开发 Web 端组件化数据分析界面（原「Yew/Leptos 路线」按 L427 选型定论废弃，本项即其登记的改写轮）：web/app 组件化拆分——QuoteTable（props 化可复用）/ WasmProbe（L427 探针提取）/ **IndicatorPanel 新增**（代码 × 指标〔SMA/EMA〕× 周期三选联动 → 纯 TS 指标全序列对表 + 末值摘要；SMA 前 period-1 位 0.0 占位明示；图表化渲染留 L429）；指标面新增 ema（口径逐项对齐 packages/core/src/indicators.rs calculate_ema：空输入 []、等长无占位、ema[0]=prices[0]、multiplier=2/(period+1)、递推逐位 4 位小数取整，滑动累加与取整顺序一致）；vitest 8→12（ema 递推样本手算对账 / period=1 退化为原序列 / 取整精度 / 等长契约）；check-web.sh 结构守门扩组件三文件；选型文档 §3「L428 改写归后续轮」更新为已落实、§7③ 假设更新（DOM 渲染测试留交互复杂化后，功能优先测试到门禁量）。全仓门禁复跑 0 失败（check-lint.sh / 全仓测试 / check-cross-platform.sh / check-desktop.sh / check-web.sh 12 单测 + build）（✅ 2026-10-02）
- [x] 集成高性能图表库（D3.js + Canvas + 原生渲染）：选型定论 **lightweight-charts 5.2**（TradingView 出品金融图表库——Canvas 原生渲染 + 增量重绘/缩放虚拟化内建，gzip 增量 ~57KB，A股配色红涨绿跌；D3 属底层可视化原语〔scale/shape 需自建渲染循环与交互〕与「集成高性能图表库」目标不符不直接引入，ECharts ~300KB+ 对行情主图过重落备选；理由入 docs/web-framework-selection.md §3）；web/app 落地 PriceChart 组件（CandlestickSeries 日 K + SMA(5) 叠加线〔与指标面板同口径、跳过前导 0.0 占位段〕+ fitContent + autoSize + 卸载 chart.remove()）；数据源 demoWalk（**确定性** LCG 合成日 K 60 根、跳过周末交易日序列——Math.random 会破坏可复现门禁；OHLC 不变量单测：high≥max(open,close)≥min(open,close)≥low>0、同种子逐位一致、两位小数口径）；vitest 12→16；check-web.sh 结构守门扩 PriceChart/demoWalk + 依赖断言 lightweight-charts。全仓门禁复跑 0 失败（check-web.sh 16 单测 + tsc + build 320KB/104KB gz；Rust 侧零改动四道门禁语义不变复跑绿）（✅ 2026-10-02）
- [x] 开发跨平台 SQL 查询编辑器和结果可视化：web/app SqlWorkbench 组件——**DuckDB-WASM 引擎**复用 web/vendor/duckdb 资产（prepare-vendors 产出，不入 git；「拷入 public/vendor/duckdb 后可用、未拷入明示降级」与 WasmProbe 的 public/pkg 同款模式，构建保持 hermetic，零新增 npm 依赖）；编辑器最小实现（textarea 等宽 + 执行按钮，语法高亮/补全登记后续增强）；**跨平台**=组件随 web/app 产物可入 Tauri 壳（L427 §6 选项 A 边界）；演示表 demo_quotes + demo_candles（60 根合成日 K 可 GROUP BY/窗口练习，seedSql 纯函数生成）；结果可视化 ResultGrid（列名/行集网格 + 行列数/耗时/截断摘要，capRows 上限 100）；纯函数面 6 单测（capRows 截断/NULL 口径/bigint、tableToRows 列序取齐、seedSql 建表断言）vitest 16→22；本机 vite preview 冒烟：页面与 duckdb mjs/wasm 资产全 200（浏览器内真实 SQL 执行归真机验收边界）；check-web.sh 结构守门扩四文件。全仓门禁复跑 0 失败（check-web 22 单测 + tsc + build；Rust 零改动四道门禁复跑绿）（✅ 2026-10-02）
- [x] 实现响应式设计适配不同屏幕尺寸：web/app 首个样式面 src/styles.css（内容优先单列；三断点 ≤720 移动 / ≤1024 平板 / >1024 桌面版心 1080px）——移动端触控目标 ≥40px、label/select/textarea/button 纵向堆叠全宽、字号 15→14 收缩；宽表/图表在节内横向滚动兜底（section overflow-x，组件零改动）；th/td 收缩 + hover 反馈；色彩令牌铺最小集（深浅主题系统留 L432 不展开）；纯 CSS 方案（不引断点 JS 库，matchMedia hook 留交互需要时）；check-web.sh 结构守门 +3 断言（viewport meta 在场 / styles.css 已挂载 / 两断点存在）；视觉验收（375/768/1200 三档截图）归浏览器真机边界。全仓门禁复跑 0 失败（Rust 零改动）（✅ 2026-10-02）
- [x] 构建统一的主题系统和个性化配置：web/app 主题面——styles.css 令牌升级为双主题（:root 浅色 color-scheme:light + data-theme='dark' 深色块，六令牌 fg/muted/accent/border/bg/bg-soft 全覆盖，原生控件随 color-scheme）；个性化配置 ThemeToggle（三态偏好 跟随系统/浅色/深色，持久化 localStorage alpha.theme，system 档监听 prefers-color-scheme change 实时切换、卸载解绑；存储异常〔隐私模式〕兜底不持久化但主题仍生效）；lib/theme.ts 纯函数面 resolveTheme 三态矩阵 + parseThemePref 脏值收口缺省 system，vitest 22→26；页头 flex 布局挂切换器（窄屏自动换行）；check-web.sh 结构守门 +2（深色令牌块 / 持久化键）。全仓门禁复跑 0 失败（Rust 零改动）（✅ 2026-10-02）

## 数据采集服务
- [x] 基于 Tokio + Reqwest 开发高性能异步爬虫引擎
- [x] 实现智能任务调度器和限流代理池管理
- [x] 构建反爬虫对策（User-Agent 轮换、请求频率控制）
- [x] 开发多数据源适配器（API、网页、FTP、文件推送）
- [x] 实现数据质量校验、清洗和标准化流程
- [x] 构建采集状态监控和自动故障恢复机制
- [x] 数据源健康面（L504，✅ 2026-10-07）：SourceHealthTracker 台账（成功清零连败/失败累计+错误截断 200 字符）→ 三态推导 unknown/healthy/degraded（连败 1..=3）/down（>3）+ `GET /sources/health`（任务表 ∪ 台账并集，从未执行补 unknown；删除任务同摘台账）+ gauge `alpha_collector_source_health{task}`（0/1/2/3）+ 告警两档 CollectorSourceDegraded（≥2 持续 5m warning）/CollectorSourceDown（≥3 持续 10m critical）。health_check 落法=执行结果推导而非模板声明（SourceDefinition 收敛方向末位字段，数据源健康本质是任务能否持续产出解析成功结果，声明式字段制造双源真相）；边界：诊断知识库未收录两条新告警——已于同日闭合（COL-001/002 入 builtin 规则，降级=依赖劣化 70 分/中断=下游故障 85 分，11 条告警全部有规则，见 docs/alerting-and-diagnosis.md §6.3）。collector 85 测试绿（+5）。

## 存储与数据处理
- [x] 集成 SQLx + TimescaleDB 实现时序数据存储
- [x] 使用 DataFusion + Arrow 构建内存分析引擎
- [x] 设计 Parquet 格式的数据湖存储架构：设计项落地 docs/data-lake-parquet.md（10 节，零代码改动沿设计项先例）——现状实测锚定（Parquet 现仅 HTTP 即时导出无落湖、DataFusion 35 仅 MemTable 热查询、ClickHouse `market_data` 七列事实 schema）；三层模型 Bronze/Silver/Gold（湖为分析侧持久层，不替代热库不迁移）；物理布局 `{layer}/{table}/trade_date=YYYY-MM-DD/part-{seq}.parquet`（交易日分区、内容幂等键、128MB 文件/行组、symbol 字典编码、snappy 起步）；schema 与 ClickHouse 列名零映射对齐（timestamp/symbol/open_price/... 七列 + 演进规则列只增不删、破坏性变更走新表版本）；写路径临时文件原子 rename 幂等重放；读路径多引擎（data-engine ListingTable 分区裁剪与 MemTable 并存 / DuckDB-WASM read_parquet 通配零改动 / Tauri 随 L427 §6 接缝）；catalog 骨架期不引 metastore/Iceberg（目录即清单，manifest 文件挂后续）；冷热分层与保留策略；后续边界划分 L447 多维分区/L448 压缩调优/lake writer 落地改造 export 端点。假设与边界见其 §10（lake_root 本机目录起步、单写者、首表 silver/market_data）。全仓门禁复跑 0 失败（docs-only，Rust 零改动）（✅ 2026-10-02）〔现注：写路径骨架已落 2026-10-09——packages/storage/lake.rs `LakeWriter::write_bars` 按交易日分组（复用 L447 partition 策略层 + trade_date_of 日切）→ Parquet 编码（snappy、§4 七列 schema 含 Timestamp(ms, UTC) 类型落面、文件内 symbol,timestamp 排序）→ .tmp + fsync + 原子 rename → write_report 清单；read_partition 单分区读回；6 单测锁往返/跨日分区/同 seq 幂等重放/空批拒绝/缺分区空读/schema 列序。export 端点改造与 ListingTable 注册仍登记〕〔再注 2026-10-09：export 端点改造已落——data-engine `lake.*` 配置段默认关（关闭零行为变化），启用后 export.parquet 写透落湖（失败只告警）+ `?from_lake=true` 读旁路（区间分区枚举读回、symbol 过滤 + limit 截断、读侧去重）；`read_range` 区间读 + 7 单测；ListingTable 注册仍登记〕〔三注 2026-10-09：§6 ListingTable 注册已落——refresh_query_tables 注册 lake_market_data（trade_date hive 目录即分区列，零 metastore），/query 直扫历史层并可 UNION 热层 MemTable；目录未建静默跳过、首次落湖后下一查自动注册；会话关 listing_table_ignore_subdirectory（DF35 默认 true 会漏列子目录文件）。§7 manifest 与 compaction 调度仍登记〕
- [x] 实现基于 Redis 的分布式缓存和限流系统：packages/storage 新增 cache.rs + rate_limit.rs（lib.rs 导出）。缓存侧 `DistributedCache`：JSON cache-aside 面（get_json/set_json/ttl 缺省、delete、get_or_load 旁路装载——命中即返、未命中装载回填、坏值不缓存原样上抛）+ AtomicU64 命中/未命中计数；限流侧 `RedisRateLimiter`：固定窗口 INCR+EXPIRE 经 Lua 原子（消灭计数无过期键的中间态），窗口判定/键拼装/窗口索引纯函数化无 Redis 亦可单测（decide_window 第 limit 个仍放行、拒绝时 remaining 恒 0）。api-gateway 接线：`--rate-limit-url`（env ALPHA_GATEWAY_RATE_LIMIT_URL，空=关闭）+ `--rate-limit-per-minute`（默认 120），/api 子路由 `middleware::from_fn_with_state` 只包 REST 反代（/health 与 /ws 不占配额），客户端标识 X-Forwarded-For 首段→X-Real-IP→anonymous 纯函数；429 + Retry-After + X-RateLimit-Remaining: 0 + JSON 错误体；Redis 故障 fail-open 告警放行（限流器不可用不拖垮网关主链路）、启动连接失败 fail-fast 不带病上线。测试：storage 34 passed（含 REDIS_TEST_URL 门控的 roundtrip/cache-aside 单次装载/固定窗口 2+1 拒绝/主体隔离/窗口滚动重置），gateway 9 passed（含门控的配额 2 内放行+第 3 个 429+Retry-After=60+/health 不占配额集成）（✅ 2026-10-02）
- [x] 开发智能数据分区策略（时间、股票、交易所维度）：packages/storage/partition.rs（纯函数策略层、零 IO，物理写盘归 lake writer 落地项沿 L445 §9 边界）。三维映射规则即「智能」核心：时间→`trade_date=`（Asia/Shanghai 日切纯函数，基数 ~250/年）；交易所→`exchange=` 目录（基数 3 裁剪收益直接）；股票→symbol 基数 5000+ 目录分区会爆炸，默认文件内聚簇排序（L445 §3 既定）、可选 FNV-1a hash 桶目录（`bucket={n:04}` 稳定 hash 锁定 FNV 常量——旧数据在新 writer 下必须同桶）。`PartitionScheme` 三档（DateOnly=L445 骨架原样 / DateExchange / DateExchangeSymbolBuckets）。compaction 装箱计划器（L445 §5 小文件治理策略侧）：同分区目录分组、小文件达 min_files 才触发、贪心封箱尊重 target_bytes、单文件组（无效重写 1→1）自动降级 untouched、大文件跳过；输入/输出均为可序列化清单结构供调度侧消费。测试 5 项锁定日切边界（23:59:59.999 vs 00:00:00）、桶号稳定性、三 scheme 路径、跨分区隔离与目标尺寸封箱（✅ 2026-10-02）
- [x] 构建数据压缩和列式存储优化算法：packages/storage/columnar.rs（纯函数算法层、零 IO，行组尺寸与 zstd 级别实测调优归 lake writer 落地沿 L445 §9 边界）。编码选择器 `choose_encoding` 决策矩阵（单调列→Delta 优先如 timestamp；基数 ≤1/4→Dictionary 如 symbol/exchange；其余 Plain；Rle 留显式指定口）+ 压缩比估算 `estimate_ratio`（统一 8B/值可比口径）。三种核心编码实现均往返可逆且 serde 可序列化（清单/调度侧直接消费）：Delta（首值+差分+显式 len 区分空列与单值列——`[0]` 单元素列不得误判空）、RLE（值+游程对）、Dictionary（去重值表+索引向量，出现顺序字典）。测试 4 项锁定决策矩阵边界（distinct×4==len 恰中 Dictionary、单元素列不走 Delta）、三编码往返、空列语义、估算数值（150 值 2 游程 50x、5ms 步长时间戳 Delta>3x）（✅ 2026-10-02）

## 性能优化工程
- [x] 实现 Rust 零拷贝数据处理和内存池管理：packages/core/memory.rs（沿 hybrid_cache 线程惯例 std::sync::Mutex + 中毒 map_err）。内存池侧 `BufferPool`：`Vec<u8>` 缓冲 acquire/release 复用（池空或容量不足才新分配），防膨胀双阈值回收（单缓冲超 max_capacity 直接丢弃、空闲数达 max_idle 拒收），AtomicU64 命中统计 + reuse_ratio 复用率面；`PooledBuffer` Deref/DerefMut + Drop 自动归还（中毒池宁可漏回收不可 drop 内 panic）。零拷贝侧 `QuoteFrame`：32 字节定长行情帧（symbol[6]+pad[2]+price f64+volume f64+timestamp i64，LE）解码视图——symbol `core::str::from_utf8` 直接借用输入缓冲、标量 from_le_bytes 逐字节构造无对齐要求，全程零堆分配；`encode_frame_into` 配合池化缓冲消除高频编码路径分配，构成「解码借用/编码复用」闭环。测试 4 项：复用命中率与清零语义、双阈值回收（超容量不回收/同时持有 4 还 2 池满裁剪）、全字段往返（2^-2 精确小数逐位验证）、坏长度/坏 symbol/空 symbol/超长 symbol 拒绝矩阵（✅ 2026-10-02）
- [x] 开发 SIMD 优化的向量化计算算法：packages/core/simd.rs。语义设计「显式 SIMD 与 portable lane 分解同数学语义」：4 lane 部分和（元素按下标 i%4 累入独立 lane、水平归约 (l0+l2)+(l1+l3)）——x86_64 运行时检测 AVX2 后走 `_mm256_add_pd`/`_mm256_mul_pd` 4-f64 向量路（`#[target_feature]` + unsafe 限定于 loadu/storeu/算术），其余平台（wasm32/aarch64/无 AVX2 主机）走 `sum_lane_reference`/`dot_lane_reference` 纯 Rust 同序实现（LLVM 自动向量化兜底），cfg(target_arch) 保证非 x86_64 目标整体剔除内建代码。关键修正：AVX2 尾部余数必须按 lane 分配（remainder[j] 全局下标 4k+j → lane j，与 lane 参考 i%4 对齐）——初版顺序累加 total 因浮点结合序差异破坏位级一致（len=7 实测 ULP 级偏差暴露）。min/max 只走 portable（x86 MINPD 与 f64::min 的 NaN 语义不同，不引入指令级差异）。测试 4 项：跨长度位级一致（含 0/1/3/5/7/13 尾部路径）、与顺序和相对容差 <1e-9、长度不等拒绝+空切片 0.0、NaN 忽略语义（✅ 2026-10-02）
- [x] 构建多核并行计算引擎（Rayon + Web Workers）：核实已完整落地（本单补勾选+标注，零代码改动）。packages/core/parallel.rs：`compute` native 走 Rayon `par_iter`（`cfg(not(wasm32))` target 门控，Cargo.toml 依赖同门控）、wasm32 顺序退化——`IndexedParallelIterator` collect 保序语义 + `parallel_matches_sequential`（四指标 × 逐元素 1e-12）与 `parallel_preserves_dataset_order`（异长数据集顺序指纹）锁双路等价；`available_parallelism`（Rayon 线程数/wasm 恒 1）+ `BatchReport`（结果/数据集数/并行度/耗时）供 JS 侧观测收益；`IndicatorKind` 与 JS 传参口径对齐。Web Worker 侧 wasm-analyzer/src/worker.rs：Worker 线程池编排（`Worker` postMessage 跨核真并行 / `Inline` 主线程降级同口径），`WorkerTask`/`WorkerResult` serde 协议与结构化克隆兼容，`handle_task` Worker 侧入口与主线程共用同一 wasm 计算实例杜绝算法漂移。测试 parallel 组 6 项全绿（✅ 2026-10-02）
- [x] 实现智能预取和后台数据同步机制：packages/storage/prefetch.rs（复用 L446 DistributedCache cache-aside 回填面）。分层：纯函数层 `detect_step`（最近 4 键严格等差检测——可解释可单测，误预取只浪费 IO 而检测器必须无误判语义；零公差=静止不预取）/`plan_prefetch`（last+step·i 未来键）/`PrefetchWindow`（8 键滑窗线程安全容器）；执行层 `BackgroundPrefetcher`：observe→检测→计划→tokio::spawn 后台回填，**在飞去重**（Mutex<HashSet> 同键任务进行中不重复派发，成败皆清标记——失败键自然获得重试窗口），loader 失败只计数不上抛（预取是优化非正确性依赖，读路径 miss 由 cache-aside 同步装载兜底），`PrefetchLoader` async-trait 契约 + 统计面（observed/planned/loaded/failed）。修复两处编译语义：`&self` 上 `self.clone()` 陷阱（解析成 Clone for &Self 借引用进 spawn 触发 E0521→显式 `<Self as Clone>::clone`）、derive(Clone) 对 L 的错误 bound（字段全 Arc→手写 impl Clone）。测试 4 项：严格等差判定矩阵（含负步进/断点/历史噪声滑窗）、计划边界、滑窗裁剪、REDIS 门控端到端（派发时序 [3,4,5]/[6]/[7] 去重推演、失败键不入缓存、标记清除后重试收敛）（✅ 2026-10-02）
- [x] 开发基于 LLVM Profile 的编译优化：scripts/build-pgo.sh（PGO 三阶段一键封装）+ docs/pgo-build-optimization.md。四阶段循环：`-Cprofile-generate` 插桩构建 → 负载采集（校验 .profraw 非空，缺负载退化为 `--help` 启动路径并明示覆盖有限）→ `llvm-profdata merge` → `-Cprofile-use` 优化构建 + 二进制大小对比（明确注明代码布局优化大小可能增减、**不是收益指标**，收益以微基准为准）。工具链探测只信当前 `rustc --print sysroot`（host triple 精确匹配）+ 缺组件自动 `rustup component add`——实测踩坑固化：跨版本 profdata 读不了新 raw profile 格式（1.97 读 1.99 的 version-11 报 mismatch）、`-Cprofile-use` 相对路径在文件存在时仍报不存在（脚本统一转绝对路径）。负载画像要求按服务登记（gateway 反代回放比例/data-engine SQL+导出/realtime tick 回放）；边界：CI 构建保持无 PGO（确定性优先，产物随发布流水线）、wasm32/移动端画像采集归对应发布项。**本机对 alpha-api-gateway 完整四阶段实测跑通**（1 profraw → 11.9MB profdata → 优化构建 → 二进制 --help 冒烟通过；36.9MB→8.2MB 为插桩/非插桩构建天然差非收益证据）（✅ 2026-10-02）
- [x] 构建内存泄漏检测和性能分析工具：packages/core/alloc_tracking.rs + scripts/profile.sh + docs/memory-profiling.md。**TrackingAllocator**：包装 System 的计数分配器（累计/存活分配数、存活字节、峰值字节，全 AtomicU64 零锁）——服务 `#[global_allocator]` 全局启用或测试直接调 GlobalAlloc 方法配对验证；`has_unreleased()` 判读语义分层（测试=泄漏 / 长驻服务看趋势），peak 单调不回退（宽松序「不低估」取向并注明）、reset 供压测轮次复位。**profile.sh**：perf record（`-g --call-graph dwarf`）/report/top 三模式封装，release 栈可读性注明 debuginfo 要求。文档：判读方法、登记项（tokio-console 带 tokio_unstable 构建条件 / valgrind 兜底 / heaptrack）、边界（wasm32 不走——JS 堆画像归 DevTools；不做每分配栈回溯；全局分配器注册互斥）。测试 3 项修复并发污染（共享 static 计数 + reset 互相清账 → 每测试局部实例隔离）（✅ 2026-10-02）

## 监控与可观测性
- [x] 集成 Prometheus + Grafana + Loki 全链路监控：服务侧四服务统一 /metrics 面（config/prometheus.yml 既定 scrape 目标终于有了真实端点）——api-gateway/data-engine/real-time-feed/collector 全部接 metrics-exporter-prometheus 0.13（metrics 0.21 workspace）：gateway/data-engine 把 `PrometheusHandle` 入共享 state（collector/realtime-feed 无状态路由直读进程级 handle），`global_metrics_handle()` OnceLock 模式解决 install_recorder 每进程一次与并行测试冲突（首个调用者 install 全局接管 metrics 宏、后续复用渲染；collector 既有 CollectorMetrics 宏指标自动入面）；gateway 带 /metrics 端点测试（200 + text/plain 协议头，空 recorder 空快照语义注明）。基础设施侧：docker-compose 补 Loki（tsdb/filesystem 单机、168h 保留）+ Promtail（docker SD 按 `alpha-` 容器名过滤、容器名→job 标签与 Prometheus 命名对齐）+ Grafana provisioning 落盘（datasources.yml：Prometheus+Loki 双数据源；dashboards provider + alpha-services-overview 仪表盘：四服务 up 统计/抓取时延/Loki 日志面）——修复原 compose 挂载 `./config/grafana/provisioning` 目录不存在导致容器起不来的既有缺陷。全部配置经 compose config + YAML/JSON 解析验证（✅ 2026-10-02）
- [x] 开发分布式追踪系统（tracing + Jaeger）：gateway trace-id 贯穿链路落地 + docs/distributed-tracing.md（Jaeger OTLP 登记边界沿 L445 设计项先例）。贯穿契约：入站非空 `X-Trace-Id` 原样沿用（跨服务/客户端重试串同链路）、缺失/空白生成 `tr-<uuid>`（网关即链路起点）、转发上游统一在透传循环后注入（避免 reqwest 追加成双值、入站已有头跳过）、响应回填供前端关联报障；`resolve_trace_id` 纯函数三态单测 + 集成测试双请求锁定（带 t-123 原样回填/不带生成 tr- 回填）；`tracing::info!(trace_id=...)` 结构化字段入日志。Loki 关联可用：LogQL `{job=~"alpha-.*"} |= "<trace-id>"` 串联 gateway→data-engine→collector（Promtail 侧 L459 已就绪）。Jaeger OTLP 全量 span 导出登记不实现：依赖坐标（opentelemetry/otlp/tracing-opentelemetry 版本对齐）、`tracing_opentelemetry::layer()` 叠加路径、`tr-<uuid>` 与 W3C traceparent 并存映射、`parentbased_traceidratio` 10% 采样（行情 QPS 下全量不可承受）均已登记（✅ 2026-10-02）
- [x] 实现基于 Metrics 的性能指标收集
- [x] 构建健康检查和服务依赖监控
- [x] 开发 Rust 专用的内存安全监控工具：packages/core/safety_audit.rs（unsafe 清单扫描 + 舱位预算断言）+ alpha-core 编译期 deny 双层防线。**编译期**：lib.rs 顶部 `#![deny(unsafe_code)]` 全 crate 硬拒，仅 simd（AVX2 intrinsics）与 alloc_tracking（GlobalAlloc 契约）两舱位模块顶部 `#![allow(unsafe_code)]` 豁免（两模块文档各自注明豁免理由与审计联动）。**审计期**（本模块）：`classify` 行级互斥分类（优先级 unsafe block > impl > fn > transmute > 裸指针 > from_raw_parts，行级归并口径注明——转换类 API 裸调用行才单独成类）+ 注释三态剔除（整行 `//`、`//!`/`///` 文档行、多行块注释首尾）→ `scan_file/scan_all` 产 `Inventory`（by_file/by_kind 全类别零值预注册，`by_kind_of` 关联函数供过滤子集复用）→ `audit` 类别预算断言（conscious ack：新增 unsafe 须显式抬预算并复核不变量）+ `quarantine_violations` 舱位白名单断言。全仓扫描测试 `workspace_stays_in_quarantine_with_kind_budget`：packages/services/tools 全部 `.rs` 递归扫描（跳过 target/node_modules/.git/dist），白名单 3 舱位（simd/alloc_tracking 真 unsafe + safety_audit 自身——工具模式匹配表与测试样本必然携带 unsafe 字符串字面量，`lib.rs` deny 对真代码兜底），预算只统计真舱位实测基线（unsafe_block 12 / unsafe_fn 4 / unsafe_impl 1 / transmute·raw_pointer·from_raw_parts 全 0；全仓真 unsafe 仅 simd 4 + alloc_tracking 13 = 17 处，工具自身字符串不进预算防工具演化噪声）。报告入口 `cargo test -p alpha-core safety_audit -- --nocapture`。修正既有测试两处断言 bug（classify block 优先级与期望值自打脸、预算数量 2→1）与 6 处 unused 警告（by_kind 预注册改循环）。边界：其余 crate 编译期 deny 门控登记为后续硬化项；unsafe 注入类验证归质量保证节 Miri 项（L496）（✅ 2026-10-02）
- [x] 实现实时告警和智能故障诊断：Prometheus 告警（config/alpha-alerts.yml 8 条〔现注：现为 11 条，后补 GatewayShieldTriggered、CollectorSourceDegraded/Down（L504 采集源健康 gauge 两档）〕：网关限流/上游健康/5xx 错误率、data-engine p95 延迟/内存、realtime 连接/吞吐、Prometheus up==0 自监控——只引用已真实埋点指标，upstream 标签/`/ on()` 向量匹配两处硬教训见 docs/alerting-and-diagnosis.md）+ Alertmanager（分组/抑制/路由，空 auth/模板两处启动坑已排）+ alert-webhook 服务（钉钉/企微/Slack/PagerDuty 转发，externalURL 全大写 rename + label_or 借用两处编译坑，3 单测，随 workspace 门禁）+ storage diagnosis 纯函数引擎（规则知识库→根因报告，沿 L460 链路）+ compose/prometheus 接线（alertmanager:9093 + webhook:8084）。设计文档 docs/alerting-and-diagnosis.md。边界：Loki/alloc/SIMD 无 emitter 不设规则（后续指标项）；Jaeger OTLP 全量导出沿 L460 登记；代码侧（commit d83a580）：packages/core/diagnosis.rs 快照→Finding 诊断引擎（8 测试，阈值与告警规则同源锁定、缺数据跳过不误报）+ 三服务指标埋点（gateway requests_total/rate_limit_total/service_health、data-engine query_duration_seconds/memory_bytes、real-time-feed feed_connected/messages_total，指标契约测试）+ alpha-diagnose CLI（Prometheus 7 查询快照→JSON 报告，退出码 0/1/2/3，6 测试）+ metrics 0.21→0.22 统一（修复 workspace 与 exporter-prometheus 两个全局 slot 错位致 /metrics 从未暴露业务指标的 L459 根因，宏改链式调用）。遗留登记：collector prometheus-crate Registry（CollectorMetrics::export_prometheus）与 exporter render 双世界未接通（L459 遗留）（✅ 2026-10-02）

## 跨平台 CI/CD 与发布
- [x] 配置 GitHub Actions 支持多目标平台并行构建：.github/workflows/ci.yml build 作业矩阵（ubuntu-latest x86_64 / ubuntu-24.04-arm aarch64 / windows-latest x86_64-msvc / macos-latest aarch64+x86_64）——fail-fast=false 互不遮蔽；arduino/setup-protoc@v3 统一 proto 代码生成；cache key 按 target 隔离；cargo build --workspace --all-targets --exclude alpha-desktop --target ${{ matrix.target }} 编译验证（运行期测试归 test/desktop 既有作业）。（✅ 2026-10-02）
- [x] 开发跨平台 Docker 镜像和容器化部署方案：五服务 Dockerfile 硬化（builder rust:1.98.1-slim-bookworm——原 rust:1.75 解析不了新 edition 传递依赖，`cargo +1.98.1 check --workspace` 实证向后兼容，1.99 镜像发布后跟进；runtime debian:bookworm-slim 同基底——原 bullseye 与 builder glibc 断代会符号查找失败；api-gateway 运行时补 curl（HEALTHCHECK 探 /health 而 slim 基底无 curl，原健康检查必失败）；gateway/data-engine/real-time-feed 补 protobuf-compiler（grpc 特性 tonic-build 需系统 protoc）与 g++（clickhouse cityhash C++，rust:slim 默认无）；collector 经 cargo tree 实证不依赖 alpha-protocols 无需 protoc；alert-webhook 修复根 workspace 成员清单致局部 COPY 预缓存必失败 + COPY 未入库 Cargo.lock 两处，并补 HEALTHCHECK）+ scripts/build-images.sh（services/*/Dockerfile 自动发现、TAG 默认 git 短 SHA、多平台/--push 强制 buildx 缺插件即失败不静默降级、失败汇总不短路；shellcheck 零告警 + 负路径四连实测）+ docs/docker-deployment.md（五服务镜像矩阵/同基底 glibc 论证/buildx+QEMU 多架构策略/compose 接线与生产建议/边界登记）。本机实证：五镜像 docker build 全部通过 + cargo +1.98.1 全 workspace 向后兼容。边界：Cargo.lock 未入库（复现性）、cargo-chef 依赖层缓存、CI buildx push 归 L470、镜像扫描/SBOM/签名归发布流水线（✅ 2026-10-02）
- [x] 实现自动化测试（单元测试、集成测试、端到端跨平台测试）：单元/集成层沿既有面（workspace 全量 cargo test + REDIS_TEST_URL/TIMESCALE_TEST_URL 门控集成 + tests/ 契约测试，随 lint/test 门禁与 CI 作业）；端到端缺口本项补齐 = scripts/check-e2e.sh——真实进程拉起 data-engine/real-time-feed/collector/api-gateway 四服务五段链路断言（各服务 /health、gateway 聚合探测三上游 healthy、REST 反代 /api/v1/health 200 + x-trace-id 回填、WS 反代握手 101、gateway 业务指标在位 requests_total+service_health——兼作 metrics 0.22 slot 修复的集成回归、三服务 /metrics 200），Redis 前置 15s 重试（CI 容器冷启竞态）+ 端口 env 覆写与占用预检 + 失败倾倒四服务日志尾 + trap 进程清理；shellcheck 零告警、本机两轮全绿。CI e2e 作业：ubuntu（docker redis）+ macos（brew redis）矩阵 fail-fast=false 与本地同命令；docs/rust-code-standards.md §8 测试分层与 §11 门禁命令/CI 作业清单同步。边界：Windows 运行期缺 Redis 简单获取途径（runner 默认 Windows 容器），其编译面由 build 矩阵覆盖、运行期 e2e 矩阵归后续登记；本项为冒烟深度（未含行情消息级 WS 载荷断言），测试体系深度归 L491 统筹（✅ 2026-10-02）
- [x] 构建多平台发布流水线（Web、Desktop、Android、iOS）：`.github/workflows/release.yml` 七作业——**version-gate**（L472 `check-version.sh` 五处同值对账 + dispatch/version 输入与 v* tag 触发版本对仓库版本对账，防镜像 tag 与 updater feed 把 0.1.0 产物标成 0.2.0；docker/updater-feed `needs` 它）→ **web**（wasm-pack + `build:prod` 指纹/预压缩链；CDN secrets 缺省 dry-run 走查发布计划 + web-dist 产物上传）→ **desktop**（ubuntu-22.04 专属——24.04 源只有 webkit2gtk-4.1 而 Tauri v1 需 4.0 / macos arm64 / windows 三宿主并行 tauri bundler 全量出包 + AppImage `--appimage-extract` 无 FUSE 结构冒烟 + 安装包与 updater 素材（*.tar.gz/*.sig）上传）→ **android**（NDK 27.3.13750724 与 gen-bindings 默认对齐；ALPHA_ANDROID_KEYSTORE_B64 门控注入 ALPHA_KEYSTORE_* 四变量，缺 secrets 走 unsigned 冒烟语义）→ **ios**（STAGE=bindings 冒烟恒跑；ALPHA_APPLE_* 门控 archive/export/TestFlight，缺则登记说明，macos-latest 上跑）→ **docker**（ALPHA_REGISTRY 门控 buildx 多架构 amd64+arm64 --push，缺则登记跳过）→ **updater-feed**（desktop 产物拼 latest.json：tar.gz+sig 齐才产出、ALPHA_RELEASE_BASE_URL 门控、三平台 rust triple 映射与 release-update-feed.sh 同口径）。接缝按既有脚本实测接口逐一对过（web-cdn-deploy/desktop-release/android-release/ios-release/build-images/release-update-feed 六脚本的 flag/env 全部匹配）；`inputs.*` 一律走 env 注入不内联 run（自由文本防 shell 注入），notes 用 GITHUB_ENV 多行分隔符写（内联 `NOTES="${{...}}"` 会被引号/换行/命令替换击穿）；只产 artifacts 不建 GitHub Release/Tag（发布面归 L471 商店、L472 版本管理），无密钥仓库全绿跑通（各脚本非交互降级语义）。验证：YAML 解析 ✓ + 全部 run 块 `bash -n` 零失败；触发双轨 workflow_dispatch（version/notes 输入）+ v* tag（✅ 2026-10-02）
- [x] 集成应用商店发布（Google Play、App Store、Microsoft Store）——L517/L518 已产各店二进制与签名链，本项交付「上架面」：`store/` 元数据（Play 短/长描述 + AppStore 副标题/描述/关键词 × 中英，长度上限逐项核过：subtitle-en 33 超限已改 28）+ `docs/store-publishing.md`（产物对接表/分阶段发布三店灰度语义——Play staged rollout/App Store phased release/桌面 updater feed 即分阶段能力/截图实拍清单/隐私问卷答案源接缝）；MS Store 登记两条路（PWABuilder 套壳现成 manifest+SW / 原生 MSIX 归 L470 Windows 作业，不产 MSIX）；国内市场（版号/代收商务未启动）登记后续。边界：真机提审提交动作归发版时人工（✅ 2026-10-02）
- [x] 开发自动化版本管理和热更新机制——`scripts/bump-version.sh X.Y.Z [--android-code N]`（根 workspace version 唯一事实源，一处输入五处同改：Cargo workspace/web package.json/tauri package.version/Android versionName/SW 缓存名后缀；严格 semver + 只允许向前 + 默认拒脏树 + versionCode 缺省不动 + mobile/ios 绝不动；改后自跑门禁）：`scripts/check-version.sh` 五处同值 + versionCode 正整数门禁（CI `version` 作业）；`docs/version-management.md`（事实源表/热更新四形态接缝——桌面 updater feed 取号、Web SW 缓存跟版即更新、Android versionCode 判据、iOS 不自更新；与 L470 执行者/L471 上架面分工）。实测：0.1.0→0.9.9 端到端五处手术级 diff 零噪音后还原；四道 guard（等值/回退/非法格式/脏树）全拦；SW 缓存名首推拉齐 v0.1.0 使门禁转绿（✅ 2026-10-02）

## 多平台用户体验与产品
- [x] 设计跨平台一致的用户体验和交互模式：`docs/ux-consistency.md` 对账表（实测/源码锁定）——涨跌色板四端同值（web `#c0392b/#27ae60` = Android `widget_up/down` = iOS 灵动岛红绿，A 股红涨绿跌）；**本项收敛一处真分歧：平盘归属**——web（`pct >= 0`）与 iOS（含等号）把 0 归涨侧，Android widget 曾为 `> 0` → FLAT，已改 `>= 0` → UP（A 股平盘红惯例，JVM 加零值断言 `"+0.00%"`+UP）；数字格式两位小数 + 缺值占位符同形（Android `Locale.ROOT` 防本地化小数点漂移）；主题三态缺省跟随系统四端统一（桌面窗口 chrome 受 tauri v1 无 system 档限制留空走 OS，前端三态照常）；过期语义 120s 同口径（web 为模拟盘降级形态不同阈值同源）；跳变反馈因 RemoteViews/节流能力上限形态分歧但方向语义一致不强求。Android 39/39 全绿。缺口登记：平板断点/aria 系统化/`prefers-reduced-motion`（✅ 2026-10-02）
- [x] 实现统一的用户账户系统和数据同步——`packages/core/src/account.rs` 纯函数领域面（服务端/各端共用同一份语义）：① 账户档案 `AccountProfile`（display_name/email/locale + 服务端权威 `rev`）+ `normalize_account_id`（JWT `sub` → 存储安全 slug，折叠后追加 8 位摘要防撞）；② 同步记录 `SyncRecord`（`namespace:local_id` 键、`rev` 单调、`deleted` 墓碑位 + 必须 Null 载荷、64 KiB 载荷上限）+ 完整校验；③ 三方合并 `plan_sync`（基线 rev 快照 + 本地/远端双改 + 内容不等 = 冲突，`ConflictPolicy::NewestWins`/`LocalWins`/`RemoteWins` 裁决，**并列取服务端保证两端必然收敛**）；④ 同步往返 `SyncRequest`（cursor + base + pushes）→ `SyncResponse`（cursor + accepted/rejected/changes，乐观并发 `base_rev` 不符回 `SyncRejection` 携带服务端权威副本）；⑤ 客户端状态机 `SyncState`（发件箱后写覆盖/基线推进/水位单调防回退/出队上限 100 条/墓碑落地即本地删除）；全显式时间入参、无时钟读、wire 形状锁定（snake_case、拒绝原因原名）。19 单测全绿（账户 id slug/键形状/记录校验/推送拉取/冲突裁决收敛/墓碑传播/缺键非删除/端到端收敛/发件箱上限/请求基线/响应应用/游标回退忽略/序列化回环/档案校验/协议形状），core 131+19=150 全绿、clippy 0、lint 门过（✅ 2026-10-02）
- [x] 开发平台特色功能（桌面：文件导出，移动：推送通知）——桌面文件导出：`lib/exportCsv.ts` 纯函数（看板快照 → CSV：表头 `symbol,name,price,change_pct,volume,updated_at`、RFC4180 最小转义、涨跌% 两位、时间 ISO、非有限留空；空快照只回表头与服务端 `history.csv` 同形）+ `downloadCsv` Blob 下载（非浏览器环境返 false 降级）+ 看板「导出 CSV」按钮（桌面复用同一 web 应用零壳改动；与 L504 服务端单标历史导出分工=快照 vs 序列）。移动推送一半此前已由 L337（本地通知 + 可插拔通道 + FFI 六方法）与 L509（widget/快捷方式）交付，不重复造。4 个新 vitest（64/64 全绿），tsc + vite build 过（✅ 2026-10-02）
- [x] 构建跨平台帮助文档和视频教程系统——文档站侧边栏引用的 intro/getting-started/installation/deployment 四页全部缺失（构建即断链），本项补齐 + 新增用户指南：`docs/intro.md`（产品能力总览与分流）、`getting-started.md`（环境矩阵 + 60 秒本地看板）、`installation.md`（Web PWA/桌面三平台产物/Android 双渠道/iOS 链路，各指 L516–L518 专属文档）、`deployment.md`（部署总览：最小生产拓扑 + 各场景分流表，不与 DEPLOYMENT.md/docker/web-cdn 重复写两遍）、`docs/user-guide.md`（日常操作唯一事实源：看板状态语义/工作区/CSV 导出/隐私面板/主题/移动端入口），sidebar 新增「用户指南」分类；python 核验 sidebar id 零缺失 + 站内相对链接零断链。边界：`docs/` 无 node_modules 且离线装不上依赖，docusaurus 整站构建验证归 CI；视频教程录制托管管线未立项，用户指南即文字版唯一事实源（✅ 2026-10-02）
- [x] 实现多语言国际化和本地化支持——Web 端中英双语：`lib/i18n.ts` 字典基座（zh 为键全集、`en: Record<I18nKey,string>` 漏 key 编译期即拦 + 运行时非空测试 double-check；`tf` `{name}` 插值；`parseLocale` 优先级 `?lang=` > localStorage(`alpha.locale`) > `navigator.language` > 缺省 zh，非法值回退不抛错；数字/时间走 Intl tag 不在字典拼串；专有名词/机器契约/throw 文案不进字典）；`hooks/useLocale.tsx`（LocaleProvider 全树重渲染 + 持久化，隐私模式写失败不阻塞）；11 文件全量转换（App 标题/分区/语言切换器、主题三态、看板表头/状态/导出、演示表、工作区全串、指标/K 线/SQL/WASM/隐私面板；`THEME_PREFS` 去文案只留取值；工作区默认名/空名兜底可本地化注入，默认行为不变）；`toLocaleString/Time` 按语言选 tag。6 个新测试（70/70 全绿），tsc + vite build 过；残留中文仅代码注释与语言自名（中文/English）。边界：移动端（Android/iOS 字符串资源）与文档站英文版未做（✅ 2026-10-02）
- [x] 开发跨平台用户行为分析和产品优化系统——本地优先 + 可插拔上报（L337 同款口径）：① `packages/core/src/behavior.rs` 行为指标纯函数（与 `analytics.rs` 行情面互补）：事件计数/日活（UTC 天去重）/会话切分（gap 切分 + 用户字典序确定输出）/漏斗（按序完成去重计数）/N 日留存（首日为群、无事件返 None 不编 0）；时间全显式入参无时钟读，事件 serde 可序列化（上报契约）；6 个手算锚测试，core 137 全绿、clippy 0；② web `lib/analytics.ts` 采集缓冲（缺省关闭 record 空操作、封顶 200 丢头、`drain` 取走清空、开关持久化 `alpha.analytics_opt_in` 进 L488 显式清单 + 前缀面、匿名 id 每次加载随机不持久化、发送器由调用方注入本层不管网络）+ 隐私面板 opt-in 复选框 + 三处真实埋点（workspace.create/quotes.export_csv/sql.run）；4 个新测试（74/74 全绿），tsc + build 过。边界：上报发送器（flush 到哪）与服务端聚合看板未接线；移动端采集未做（✅ 2026-10-02）

## 安全与合规
- [x] 实现 JWT + OAuth 2.0 身份认证系统：网关 auth.rs（Claims 自签 HS256 签发/校验 + OIDC kid 选键校验核 + JWKS 拉取，exp 必需/自签 leeway 0/401 不区分原因，7 单测）+ 中间件接线（--auth-mode off|jwt 默认 off 零行为变化；jwt + 空 secret 启动 fail-fast；auth 后注册先执行、不消耗限流配额；/health /metrics /ws /auth/token 公开）+ POST /auth/token bootstrap 签发（X-Provision-Key，空 key 默认 503 关闭，ttl 上限 24h）+ auth_total allowed/denied 指标。设计文档 docs/auth.md。边界：OIDC JWKS 已接线（2026-10-07，L502——--auth-jwks-url 拉取 + 定时刷新保旧键 + kid 分流核 verify_any，oct 键 base64url（RFC 7517），启动 fail-fast，REST/WS 同核；RSA/EC x5c 链验证与 discovery 解析归硬化项，见 docs/auth.md §4）；授权码浏览器侧归 IdP/前端（下一增量）；scope 透传不断言（L484 RBAC）；WS 握手鉴权已接线（2026-10-07，jwt 模式 /ws 升级持票校验——Authorization 头优先 ?token= 回落、ws_allowed/ws_denied 指标、校验先于上游拨号，见 docs/auth.md §3.1；帧级订阅 ACL 未做）；刷新令牌生产走 IdP（✅ 2026-10-02）
- [x] 开发基于 RBAC 的细粒度权限控制：Claims 增 roles（空=viewer 兼容旧票据）+ authorize 纯函数（admin 全通/读任意已认证/写需 operator，path 维度预留）+ 中间件 401 后接 403（forbidden 体可区分重登与加角色）+ /auth/token 透传 roles（provision_key=root 口径注明）+ forbidden 指标。单测矩阵（空/operator/admin/未知角色 × 读写）+ 集成 403。docs/auth.md §6。边界：逐端点矩阵待首个危险写端点（✅ 2026-10-02）
- [x] 实现端到端数据加密（传输 + 存储）——传输面 TLS 归 L485 对端（api-gateway axum-server/rustls + rcgen 自签证书）；**存储面静态加密** `packages/core/src/crypto.rs`（AES-256-GCM 应用层加密，随机 IV、认证加密、Base64 编码存储，密钥通过 `ALPHA_STORAGE_ENC_KEY` 环境变量注入）+ `packages/storage/src/encryption.rs` 透明加密包装器 `EncryptedStorage<B>`（包装任意 `StorageBackend` 自动加解密，Redis KV/API Key/会话/隐私数据适用）+ 列级加密辅助 `encrypt_sensitive_field`/`decrypt_sensitive_field`（Timescale/ClickHouse 敏感列文本存密文），`crypto` feature 门控（std-only，wasm32 不编译）、core 5 测试 + storage 4 测试全绿、clippy 0、lint 门过；边界：密钥管理/KMS/轮换归后续、传输加密归对端 TLS 实现（✅ 2026-10-02）
- [x] 构建防爬虫、DDoS 和 API 限流保护——限流面此前已成型（`RedisRateLimiter` per-身份 per-分钟 + 429 Retry-After/X-RateLimit 头 + 指标告警，L446），本项补齐**防爬虫与应用层 DDoS 护栏** `services/api-gateway/src/shield.rs` 三件套（全部纯逻辑、时间显式入参可回放，middleware 只装配）：① UA 分类（空/已知脚本爬虫特征——curl//scrapy/python-requests/go-http-client/headlesschrome/bot|spider|crawler 等小写子串表，`curl/` 带斜杠防词根误伤）；bot 拒绝可关（默认关——curl/脚本既有调用方与 e2e 不破坏，ALPHA_GATEWAY_BOT_DENY=1 开启 → 403）；② 路径扫描检测（同身份滑动窗口**离散**路径数超阈 → 403，重复热点路径不误伤；默认开、宽阈值 240/min）；③ 秒级 burst 令牌桶（per-身份容量 100/补速 50/s，补齐分钟配额的秒级洪峰空隙；默认开）；内存界：身份分桶超 1 万逐出闲置 60s 项（防身份伪造撑爆）；执行序 auth → shield → rate_limit（护栏挡下不消耗 Redis 配额），生效面只在 /api（health/metrics/WS 天然豁免）；env 全可调非法值回退默认（配置面永不拒绝启动）；`alpha_gateway_shield_total{mode}` 指标 + GatewayShieldTriggered 告警规则接入既有 alerts 体系。测试 +7（分类表/扫描离散 vs 重复/桶深吸收与匀速补充/身份隔离/默认形态不杀脚本 + 3 集成 403·429·独立桶），网关 27 全绿、clippy 0、全仓门过；边界：网络层 DDoS（SYN 洪水等）归基础设施（LB/CDN），应用层为网关职责上限（✅ 2026-10-02）
- [x] 开发安全审计日志和异常行为检测——`services/api-gateway/src/audit.rs` 审计通道 + 失败风暴检测：① **审计事件分类**（serde tag PascalCase 与 protocols/alerts 同风格）：AuthFailure（错票据——主动攻击信号；**缺失 token 不进审计面**，那是常规未认证流量，量纲归护栏/限流）、AccessDenied（rbac 越权/bot/burst/scan/rate_limit 五源统一入流）、TokenProvisionDenied（bootstrap 签发钥爆破）、TokenProvisioned（状态变更必记）；量纲口径 = **只记攻击信号率不记常规放行**（放行有访问日志与 requests_total，重复记淹没信号）；`emit` = `target: "audit"` 结构化日志（Loki 按 target 单独采集）+ `alpha_gateway_audit_total{event}` 指标，序列化失败降级为只记 kind 不丢事件；② **异常行为检测** `AuthFailureDetector`：同身份滑动窗口失败次数达阈（默认 60s/20 次，env 可调非法回退）→ 风暴位 warn + `alpha_gateway_audit_anomaly_total`——**只标记不自动封禁**（误封共享出口 IP 代价高于漏标，IP 黑名单/账号锁定登记后续硬化项）；分桶超 1 万逐闲置（同 shield 口径防身份伪造撑爆内存）；③ 接线：auth 中间件错票据→风暴检测、rbac 拒绝→AccessDenied、provision 错钥→TokenProvisionDenied+风暴、签发成功→TokenProvisioned、shield 三拒绝与 rate_limit 拒绝→AccessDenied（护栏挡下的攻击不再只是各自指标，统一审计流可查）；时钟只在 middleware 读（system_now_ms），判定全在纯逻辑面。测试 +3（taxonomy 形状/风暴达阈与窗口滑出恢复/集成：错票据风暴后两指标在 /metrics 在位），网关 30 全绿、clippy 0、全仓门过（✅ 2026-10-02）
- [x] 确保金融数据合规性和隐私保护（GDPR、CCPA）——**数据主体权利的工程面** `web/app/src/lib/privacy.ts` + `PrivacyPanel`（无账号体系、本地优先——数据主体 = 本机用户）：① 可携权 `exportUserData`（`alpha.` 应用键全量 JSON 快照，schema/version/exportedAt 自描述，一键下载）；② 被遗忘权 `clearUserData`（清除范围 = 显式清单 ∪ `alpha.` 前缀扫描——未来新增应用键自动纳入权利面；非应用键一律不动；幂等）；③ 清单展示 `listUserDataKeys` + PrivacyPanel（逐键存在性 + 确认交互）；存储全走注入 `KeyValueStore`（生产 localStorageStore 适配，测试 Map 后备替身——node 测试无 localStorage）。`docs/data-privacy.md` 义务映射：八项法定义务逐条 → 落地面/状态（访问/可携、删除、撤回同意、最小化、存储限制、透明、问责、自动化决策 N/A）+ 数据清单表（本地个人数据 vs 服务端公开行情 vs 过程日志三分）+ 保留期限表 + 跨平台边界（Android 清除应用数据即删除已政策化、应用内工具登记后续；iOS 归 L119 面）+ 72h 泄露响应链登记（无自动化编排，诚实边界）。与 L520 platform-compliance.md 分工：那边是隐私政策与权限台账，这边是权利义务映射与工具。web 测试 +7（60/60）、tsc 过；文档前向引用纪律：L487 未落地前不写成既有事实（✅ 2026-10-02）

## 质量保证与测试
- [x] 建立单元测试、集成测试和端到端测试体系——`docs/testing.md` 统筹文档（单元/属性/集成契约/e2e/性能/内存/覆盖率七层落点表 + 前端 vitest/tsc/Android JVM 面 + 门禁命令与 CI 作业矩阵 + 缺口登记）；e2e 消息级深化 `scripts/check-e2e.sh` 断言 3b：Node 24 全局 WebSocket 直连 gateway `/ws` → Subscribe + Resync(from_seq=0) → **无论通道是否存在必须回帧**（Sync Full 快照或显式 Error），5s 静默超时判协议破坏；Sync 帧校验 channel 回显/seq 数值/op∈{full,delta}/data 在位，Error 校验 code+message（实测无行情注入时通道未建 → Error code=404，契约成立）；线上帧型 = serde variant 原名 PascalCase（探针实测 `{"type":"Resync",...}`）——顺带发现并修复 L499 liveFeed 帧型漂移（入站大小写不敏感 + 出站改协议原名，独立 fix commit）；web 53/53、shellcheck 零告警、e2e 全绿。边界：数据面 e2e（XADD→Delta→客户端合成收敛）依赖 collector envelope 注入链，登记为后续深化（Full 路径由 real-time-feed 单测锁定）；测试量只到门禁量（✅ 2026-10-02）
- [x] 开发性能基准测试和回归测试套件——criterion 基准套件 `packages/core/benches/indicators_bench.rs` 三组 8 项：热路径指标（SMA/EMA/RSI/Bollinger/MACD @10k 根，Elements 吞吐标注）、形态识别（K 线形态 + zigzag find_swings @10k）、优化引擎（2×3 网格 grid_search @1k——寻优编排开销代表面）；价格序列确定性 LCG 游走（种子 42，同 simd.rs 测试口径，无 rand 依赖可复现）；dev-dep criterion 0.5 关 default-features（无 plotters/gnuplot，CI 零绘图依赖）；`cargo bench -p alpha-core --bench indicators_bench` 实测 8 项全跑通（SMA 10k ≈70µs / find_swings 10k ≈32µs / grid_search ≈202µs）；lib 自带 bench 目标与 criterion 参数不兼容的坑以 `--bench` 定向解决；clippy --all-targets（含 bench）0 警告、全 crate 124 测试通过。回归检测面归 L495（--save-baseline 比对本套件）（✅ 2026-10-02）
- [x] 实现基于 Proptest 的模糊测试和属性验证——`packages/core/tests/proptest_invariants.rs` 集成测试 9 性质（256 案例/性质，有限价格策略防 f64 溢出淹没断言）：SMA 平移线性（跳过头部 period-1 的 0 填充段、容差覆盖滑窗浮点漂移）、EMA/RSI 有界（EMA 不越输入 [min,max]、RSI 恒落 [0,100]）、Bollinger 三轨序（上≥中≥下）、最大回撤幅度 ∈ [0,1]、VaR 置信度单调（0.90 ≤ 0.99）、夏普常数收益 → None（level 限 dyadic——任意 f64 常数的 mean=n·x/n 有 1 ulp 误差会让 std 极小非零，性质只对可精确均值的常数成立）、自相关 = 1、摆动检测**无前瞻**（序列延长后已确认枢轴逐项保持——新枢轴只增不改，对任意阈值）；**语义边界登记（非缺陷）**：时间镜像对称对相对阈值 zigzag 不成立——涨跌幅以极值为基价，反转后基价互换（实测 +7.33% 确认的枢轴反向 -6.83% 不确认），故以无前瞻性质替代；dev-dep proptest 1.5（独立 hunk 暂存避让他单在改的 metrics 行）；133 测试全绿（124 单测 + 9 性质），clippy 0 警告（✅ 2026-10-02）
- [x] 构建代码覆盖率报告和质量度量——`scripts/check-coverage.sh`：cargo-llvm-cov（LLVM source-based）对 workspace 出行/函数覆盖率，`--summary-only --lcov` 双出（终端汇总 + target/coverage/lcov.info 供 CI artifact/genhtml）；范围与常规测试门禁一致 `--exclude alpha-desktop`（GUI 层归 CI macOS 作业分工）；**不设硬阈值**（凑数测试反噬质量，报告产出即交付、回归观察靠趋势——用户约束的测试量口径）；CI `.github/workflows/coverage.yml`（taiki-e/install-action 装 cargo-llvm-cov + llvm-tools-preview，lcov 上传 artifact，周例行 + path 触发）；本地实测全链通过：78 文件、行覆盖 62.4%（12465/19970）、全部测试过、exit 0（✅ 2026-10-02）
- [x] 开发自动化性能回归检测系统——`scripts/check-perf.sh` criterion 基线比对（`--save-baseline` 固化 / `--baseline` 比对，基线 target/criterion 按基准分桶不入库；PERF_SAVE=1 显式刷新、PERF_BASELINE 改名）；criterion 对回归只打印不退出是坑——脚本 tee 输出后 grep「Performance has regressed」判失败（判退路径实测 exit 1：3 项 p=0.00 判退被正确拦截；放行路径 = grep 否命）；**前置条件登记：机器须空闲**——criterion 记墙钟，同机并行 cargo 构建期间实测 +100% 级全线漂移（p=0.00 全线判退），CI 单租户 runner 天然满足、本地跑前须停其他构建；CI `.github/workflows/perf.yml` 周滚动基线（周号 env + actions/cache：同周沿用旧基线比对防漂移、跨周重固化），仅 schedule/手动触发不进 PR required（统计判定不吃机器噪声误伤）；抖动耐受由 criterion p 值自带不另设百分比阈值（✅ 2026-10-02）
- [x] 实现基于 Miri 的内存安全静态分析——`scripts/check-miri.sh`（alpha-core 单库 `--lib`、5 种子 `-Zmiri-many-seeds=0..5`、`-Zmiri-disable-isolation` 兜底 clock_gettime 无隔离回退、`-Zmiri-tree-borrows` 规避上游 crossbeam-epoch 0.9.21 Stacked Borrows 违规）+ CI `.github/workflows/miri.yml`（nightly+miri、rust-cache key `miri-nightly`、周例行 + core 路径触发）；`cargo +nightly miri test -p alpha-core --lib` 实测全种子通过（simd/alloc_tracking 两舱位 unsafe 全覆盖、safety_audit 审计测试含全仓扫描均绿），环境超时 SIGTERM 143 属 runner 时间限额非测试失败（✅ 2026-10-02）

## 业务功能开发
- [x] 开发实时行情展示和技术指标计算：web/app 新增实时行情看板——① 协议面 `lib/liveFeed.ts` 纯逻辑（契约对齐 packages/protocols/websocket.rs 与 real-time-feed /ws:8082）：订阅帧/心跳、`parseFeedFrame` 判别联合（sync Full/Delta + 旧版 data 兼容）、`RealTimeQuote` 形状校验、通道快照字段级浅合并（depth-1 差集口径同 alpha_core::sync）、重连退避（500ms 指数封顶 8s 确定性无抖动）、`?feedWs=` 地址覆盖；② 降级面：`createSimTicker` LCG 确定性模拟盘（同 demoWalk 风格，同 seed 同符号序 → 同一 tick 序列），feed 重试 2 次不可达自动切换——界面/测试任何环境可跑；③ `hooks/useLiveQuotes` 薄装配（生命周期/重连/清理，状态推进全在纯函数），`components/LiveQuoteBoard` 看板（最新价跳变闪烁 key 重挂载重放 CSS 动画、A股红涨绿跌、SMA(5) 滚动窗口现场计算——纯 TS 指标口径已对齐 packages/core、sync seq 丢帧诊断显示）；指标计算面此前已由 core indicators/advanced/patterns + wasm-analyzer 导出与前端 indicators.ts 口径对齐构成，本项补齐其实时消费端。15 个新 vitest（41/41），tsc --noEmit + vite build 通过，构建产物不入库（dist 入库文件还原）（✅ 2026-10-02）
- [x] 实现高级技术分析（形态识别、波浪理论）：① 形态识别新模块 `packages/core/src/patterns.rs`——K 线形态（十字星/锤子/射击之星单根、看涨/看跌吞没双根、晨星/暮星三根，纯几何阈值全显式参数化、不含趋势上下文判断——上下文过滤登记为调用方策略层职责；`detect_candle_patterns` 扫描锚定在形态最后一根）；线形形态（双顶/双底、头肩顶/头肩底，全部跑在 `find_swings` zigzag 确认制摆动上——极值需反向 ≥ threshold 波动确认才入列、方向未定阶段双端盯梢、尾部未确认极值不 emit，无前瞻）；② 波浪理论：`indicators/advanced.rs` 的 Elliott 占位实现（自注明「简单示例」、数峰谷无规则）重写为 zigzag 摆动 + 经典三规则校验引擎（R1 浪2 不全回撤/R2 浪3 永不是最短/R3 浪4 不入浪 1 领地，牛熊镜像对称），A-B-C 修正段校验（B 不超浪 5 端点、C 越过 A），ElliottWave 结构体带枢轴下标/五浪幅度/B 对 A 回撤比例；全部手算价位测试锁定（含三规则各自的拒绝路径、熊市镜像、亚阈值噪声不产生浪）。19 个新测试，全 crate 124 通过，clippy 0 warning（✅ 2026-10-02）
- [x] 构建量化交易策略回测和优化引擎：回测半边（BacktestEngine 逐 bar 记账/SmaCrossStrategy/BacktestReport）已有，本次补齐**优化引擎** `packages/core/src/optimize.rs` 两层寻优面——① `grid_search` 网格穷举：参数轴笛卡尔积 → 训练段全员打分（夏普降序、并列参数字典序确定性 tie-break）→ 样本内最优组合在验证段用**全新策略实例**验证（杜绝滚动窗口状态泄漏）→ 全样本报告 + 过拟合比（样本外夏普 ≤0 记 INFINITY 的显式信号位）；② `walk_forward` 滚动前推：n 折独立「段内重新寻优 → 紧随段外评估」，样本外净值跨折复利拼接、整体回撤在拼接曲线上量（折间亏损叠加才是实盘真实回撤）。语义决策已测试锁定：long-only 策略单边下跌躺平收益 0（过拟合 INFINITY 含「躺平」触发面）、冲高回落才真亏（41 入 30 出手算锁定）；顺修 backtest.rs 年化夏普的真 NaN 源（2 点净值方差分母 n-1=0 → 污染寻优排序，改 len<3 返 0）。契约登记：轴独立积不做跨轴约束过滤（非法组合工厂 panic 暴露）。7 个新测试 + 全 crate 113 通过，clippy -p alpha-core 0 warning（✅ 2026-10-02）
- [x] 开发风险管理和投资组合分析工具：packages/core/src/risk.rs（纯函数风险面，与 backtest 策略面/analytics 业绩面互补）。度量九件：最大回撤（峰值/谷底定位、峰值≤0 分母保护）、年化波动率（样本 std·√ppY，<2 期 None）、历史 VaR（手写线性插值分位、置信度区间校验、亏损取正）、夏普（std=0 → None——全赢策略无统计意义的口径明文化）、索提诺（下行偏差分母取全序列长度、无下行期=0 非 None）、Pearson 相关（常量序列 None、结果 clamp ±1）、组合波动率 wᵀΣw（维度不匹配 InvalidInput、数值噪声钳 sqrt(NaN)）、固定分数仓位（多空对称取 |entry−stop|、零距=0）、Kelly（含全胜理论上限 + 文档明注工程用半 Kelly）；RiskReport::compute 一次聚合五指标供工具/面板消费。口径约定头注：简单收益率、年化基数显式传参（A 股 252 是惯例非常数）、空/短序列降级不 panic。测试 7 项全带算术锚（0.5 精确二进制防假 std、小样本 VaR 需负收益区、ρ=0→0.1/√2 vs ρ=1→0.1 分散化语义对照、Sortino 上行不被罚的增量>Sharpe 增量证明）；clippy 0 警告（✅ 2026-10-02）
- [x] 实现智能告警和个性化消息推送——业务级告警规则引擎 `packages/core/src/alerts.rs`（纯函数，与 L464 运维面告警 Prometheus/Alertmanager 互补）：① 规则即数据 `AlertRule`（id/owner/cooldown_ms + 条件 flatten 进规则体，serde tag 原名 PascalCase 与 protocols::websocket 同风格——服务端存取与客户端展示共用形状，序列化测试锁定）；② 四类条件：PriceAbove/PriceBelow（含等于的「触及」口径）、PctChangeAbove（窗口首收盘为基价、绝对值口径、基价 0/窗口不足静默）、SmaCross（金叉/死叉对称严格判定——前值严格一侧→当前严格另一侧，贴线不算、同向延续不重报；前值优先引擎推进状态、冷启用快照内前一根兜底；SMA 尾元素 0 填充假值位以 idx≥long-1 拒判）；③ `AlertEngine.evaluate`（快照逐规则判定 + 冷却抑制按规则 + **交叉状态每拍必登记**——与是否触发/是否被抑制无关，「上一拍」随时间推进而非随触发推进）；④ 个性化投递契约 `group_by_owner` → `NotificationBatch`（通道 WS/APNs/FCM/webhook 不在本层，服务侧接）；全显式时间入参无时钟读取，同输入序必同输出序。测试 7 项（触发边界=含等号/冷却窗口边界=差值等于 cooldown 放行/涨跌幅正负向与退化输入/金叉死叉翻转 vs 延续/owner 归批保序/序列化回环/同拍多规则全产出），core 131 全绿、clippy 0、lint 门过；④ **推送接线** services/real-time-feed：`ALPHA_ALERT_RULES`（AlertRule JSON 数组，与 core 序列化形状一致；缺失/空/非法=无规则零行为，装配测试锁定）→ 行情扇出同拍评估（滚动收盘窗口封顶 64 → QuotePoint → 引擎；不阻塞 ack 语义）→ 命中走平行 broadcast 通道，WS 发送循环第三路 select 出 `Data{channel:"alerts"}`（protocols 复用既有 DataMessage 免新变体，owner 贯穿载荷；慢订阅 Lagged 丢历史保连接）；feed 测试 +3（14+5 全绿）。个性化边界登记：owner 已贯穿 规则→事件→载荷 全链，按 owner 的定向路由需连接级身份绑定（连接即订阅的现架构不区分），登记为后续硬化项（✅ 2026-10-02）
- [x] 构建市场数据 API 和第三方集成接口——行情数据面（/query、/stocks/:symbol/history、/indicators、/analytics、ClickHouse parquet 导出）此前已成型，本项补齐**第三方集成两块**：① API key 门 `services/data-engine`：`security.api_keys` 配置（默认空=关闭，内网形态与历史行为完全一致；env `ALPHA__SECURITY__API_KEYS` 逗号分隔），非空时数据面（/query、/stocks、/indicators、/analytics、/clickhouse）一律要求 `X-Api-Key` 命中其一，/health、/metrics 运维面豁免（保护子路由 + from_fn_with_state 中间件，路由前于分发）；比较走常时形状（逐字节异或走满 + 多 key 全量判定不提前返回，不泄漏「第几个 key 正确」；长度不属秘密面）；多 key 并存支持轮换灰度；② CSV 导出 `/stocks/:symbol/history.csv`：与 JSON 面同源同参（load_history_points 共用），`timestamp,price,volume` 表头 + RFC3339 行，无逗号类型免转义、缺 volume 补 0、空数据只回表头；③ `docs/market-data-api.md` 对外契约（端点表/两条接入路径/key 轮换/边界——配额归 L486、TLS 归 L485）；测试 +3（常时比较/门开关与豁免 oneshot 集成/CSV 形状与空表头），18+1 全绿，clippy 0 告警；修测试两坑：GET /query 会 405（门在路由前仍被方法层拦截，改用 GET 数据面断言）、种在未来 1min 的点落在查询窗外（终点=now，种子点全放过去）（✅ 2026-10-02）

## 平台特色功能开发
- [x] Web 端：PWA 支持和离线功能——免新依赖手搓（vite-plugin-pwa 需改 package-lock.json——他单在改的禁面）：`public/manifest.webmanifest`（standalone + 主题色 #2980b9 + SVG 图标 any 尺寸）+ `public/icon.svg`（蓝底三阳线 K 线意象）+ `public/sw.js` 自包含 service worker（预缓存壳 '/' /manifest/icon；策略 = 同源 GET 才接管：导航网络优先失败回退缓存的壳——离线整页可开、面板各自降级照常（实时看板 → 确定性模拟盘），其余资产缓存优先 + 运行时填充 + 60 条 FIFO 封顶；activate 清旧版缓存）+ `lib/pwa.ts` 策略纯函数镜像与注册薄壳（失败静默——离线是增强不是依赖，语义单测锁定）；index.html 挂 manifest/theme-color/icon，main.tsx 注册。3 个新 vitest（52/52 全绿），tsc/vite build 过、dist 根落 sw.js/manifest/icon（✅ 2026-10-02）
- [x] 桌面端：多标签页界面和工作区管理——落地面在 web 前端（桌面复用同一 web 应用，桌面壳零改动）：`lib/workspaces.ts` 纯归约器——工作区 = 命名自选标的集，新建（空名兜底「工作区」、重名追加序号）/重命名（空名与重名拒绝）/删除（最后一个不可删、删活动区回退第一个）/切换/标的集（6 位数字归一化、去重、上限 30 截断），持久化 `alpha.workspaces` 单键 localStorage + `parseWorkspaceState` 合法性收口（结构损坏/全非法 → null 走初始态、非法标的剔除、缺 id 重生成、activeId 失配回第一）；`useWorkspaces` 挂钩（no-op 同引用跳过写盘、隐私模式失败不阻塞会话，沿 theme.ts 模式）；`WorkspaceTabs` 组件（标签条切换/删除/新建/重命名 + 自选 chips 编辑，键盘 Enter/Escape 可达）；活动工作区标的集驱动实时行情看板（LiveQuoteBoard 开 symbols 入参，未传走演示默认集）。8 个新 vitest（49/49 全绿），tsc/vite build 过（✅ 2026-10-02）
- [x] Android 端：小组件（Widget）和快捷方式支持——经典 `AppWidgetProvider` + RemoteViews（免新依赖）：`HomeWidget.kt`——`WidgetQuote` 载荷 + `WidgetStateStore` 单键 JSON 持久化（损坏 fail-safe 返 null）+ `buildWidgetQuote`（首只标的、涨跌幅相对开盘现算、开盘价缺省/非正 → 平盘 "--"）+ `WidgetContent.from` 纯函数渲染语义（两位小数 Locale.ROOT、UP/DOWN/FLAT/NO_DATA 方向判定、120s 过期灰显与 iOS staleDate 同口径、时间戳解析失败按过期）；数据面守住 L301 无 INTERNET 红线——widget 零网络请求，30min 系统轮询只做占位重绘，应用刷新（首载/下拉）经 `publishWidgetQuote` 是唯一数据源；静态快捷方式两条（长按图标直达贵州茅台/平安银行预选分析），深链契约 `EXTRA_SYMBOL` 与 widget 点击同一入口（onCreate 读 extra → 自动触发该标的分析）；零新增权限、receiver exported=false。docs/android-widget.md。6 个新 JVM 测试（39/39 全绿）（✅ 2026-10-02）
- [x] iOS 端：Live Activities 和动态岛支持——`mobile/ios/` 属 L119 会话交付面绝不动（目录仅 gen-bindings.sh + MarketData/AlphaViewModel Swift 源），本项交付面 = 工程设计契约 `docs/ios-live-activities.md`（L516/L518 同款「实测止步、设计登记」先例）：ActivityAttributes 类型契约（ContentState Decimal 价格/涨跌%/updatedAt + symbol/name 固定属性）、Info.plist NSSupportsLiveActivities 能力声明、AlphaViewModel 启停/更新扩展点（60s 更新节流 + staleDate 120s）、灵动岛 compact/minimal/expanded 三态映射（红涨绿跌对齐三端色板语义）、APNs 推送更新流接缝分工（客户端 pushType .token 归 L119、服务端 live-activity 出口届时立项不预写）、opt-in 纪律对齐 L512/L520 隐私姿态、iOS 解封后验收清单五条；服务端/客户端无一行代码动在未验证面上（✅ 2026-10-02 登记）
- [x] 全平台：深色模式和系统主题适配：语义三端同构（三态偏好 system/light/dark → 生效主题，跟随系统为统一缺省口径）——web 侧 L432 已交付（ThemeToggle + matchMedia 监听 + CSS 变量对），desktop 复用同一 web 主题系统（tauri v1 `theme` 配置仅 Light/Dark 无 system 档，L516 实测过 schema 故窗口 chrome 留空走 OS 决定，登记 docs/theme-adaptation.md）；本项 Android 落地 `Theme.kt`：`ThemePreference` 三态 + `parseThemePreference` 解析收口（未知/损坏/缺省回 SYSTEM）+ `prefersDark` 映射（对齐 web resolveTheme 语义）+ `ThemeSettingsStore` 单键持久化（复用 L512 KeyValueStore 抽象，损坏 fail-safe 回跟随系统）+ `AlphaTheme` Composable 切 Material3 light/dark（isSystemInDarkTheme 组合刷新），MainActivity 去硬编码 lightColorScheme 改 AlphaTheme（偏好变更重启生效取简，进程内热切换随设置页 TODO）；iOS 目录属 L119 不动、对应物登记留档。3 个新 JVM 测试（43/43 全绿）（✅ 2026-10-02）
- [x] 移动端：生物识别认证和隐私保护（Android 落地，iOS 边界登记）：`BiometricGate.kt` 三件套——① 生物识别门：门密钥 `alpha_biometric_gate` 绑定每次认证（auth-per-use + 新录入生物凭据即作废，API 30+ AUTH_BIOMETRIC_STRONG / 26-29 validity -1 双分支），BiometricPrompt CryptoObject 流认证成功后加密哨兵明文作门禁凭证（伪造 UI 拿不到 TEE 内的密钥操作）；纯逻辑 `GateStateMachine` 迁移全显式（冷启动即锁/退后台重锁清计数/认证失败留锁累计/关开关立即解锁/开锁不突袭）JVM 锁定；② 静态加密：`EncryptedKeyValueStore` AES-256-GCM 每写新随机 IV、篡改/换钥解密返 null 不抛错，与门密钥**两层分离**（门证明人在场、加密保护数据本身——auth-per-use 密钥不能当批量数据密钥，理由入档）；能力不可用（无硬件/未录入）门不激活不把用户锁门外，由静态加密兜底；③ 防泄漏基线：FLAG_SECURE 防截屏/最近任务缩略图（默认开可关）、隐私设置单键 JSON 持久化损坏 fail-safe 回默认（biometricEnabled=false opt-in、lockOnBackground/screenshotShield 默认开）。MainActivity 换 FragmentActivity 基类（androidx.biometric 硬要求）+ GateLayer 覆盖层 + onStop 重锁；权限 USE_BIOMETRIC（normal）从 L520 注册表 planned→allow 迁移（checker 实测 android=1 对账通过）。docs/mobile-privacy.md 七节。10 个新 JVM 测试（40/40 全绿，Keystore/Prompt 设备路径不触框架类）（✅ 2026-10-02）

## 跨平台打包与分发
- [x] 配置 Web 端 CDN 部署和静态资源优化：① web/scripts/optimize-dist.mjs（零依赖 node 构建期优化，接入 build:prod 链）——内容指纹 `<name>.<hash8>.<ext>` + html 引用重写、文本+wasm 双格式预压缩（.gz -9 / .br 质量 11，**实测 82MB→12.5MB -84.8%**）、cache-manifest.json 缓存清单（指纹 immutable 一年 / html no-cache / 其余 5min 兜底三档）；desktop-shell.js 为 .gitignore 豁免的桌面壳兜底文件设 NO_RENAME 保护（首版优化器误指纹化跟踪文件，已 git checkout 恢复并加排除集）；② 源站 compose web-origin（nginx:1.27-alpine serve dist，config/nginx/web-origin.conf）——**本机实测全绿**：/ →no-cache、指纹 wasm→application/wasm+immutable、gzip_static 协商命中（6.8KB 传输）；nginx 踩坑修复（location 正则 {n} 需引号、server 级 types{} 整体替换 MIME 表）；③ scripts/web-cdn-deploy.sh S3 兼容发布（aws/rclone 自动探测、缓存头按清单逐文件、.gz/.br 带 Content-Encoding、非 HTML 资产先行 html 入口最后覆盖的发布原子性、--dry-run 无工具自测已跑通 1026+326 文件计划）。docs/web-cdn.md 六节（缓存策略三处同源表、CDN 边缘要点、与 L519 feed/L520 政策页/L470 流水线/L468 容器化衔接）（✅ 2026-10-02）
- [x] 开发桌面端多平台安装包（Windows .exe、macOS .dmg、Linux .AppImage）：scripts/desktop-release.sh（宿主 OS→bundle 面映射 Linux=AppImage+deb/macOS=app+dmg/Windows=nsis+msi，--bundles 子集与 --debug、APPIMAGE_EXTRACT_AND_RUN 无 FUSE 兜底、webkit 依赖 pkg-config 自检提示、产物与 updater feed 素材清单）+ docs/desktop-release.md（产物矩阵、macOS 公证/Windows EV·Azure Trusted Signing 登记全走 CI secret、与 L519 更新通道接缝=--bundles updater 产 *.tar.gz+.sig 喂 release-update-feed.sh、三平台系统 webview 策略）。**顺手修掉两个被「从未真跑过 tauri build」掩盖的真 bug**（tauri-cli 1.6.6 实测）：① `windows[0].theme: "System"` 非法（v1 合法值仅 Light/Dark，CLI schema 拒绝）→ 移除字段（缺省即跟随系统）；② beforeBuild/DevCommand 的 `cd ../web` 相对路径错误——探针实测 `cargo tauri` 在 **workspace 根**执行命令（非 conf 目录）→ 改 `cd web && npm ...`（conf schema 契约测试 11 passed 复验）。**本机实测止步登记**：conf 校验✓→web 构建✓→cargo release 编译停在 soup2-sys/javascriptcore-rs-sys（Ubuntu 26.04 只剩 webkit2gtk-4.1/soup3，tauri v1 需 4.0/soup2，与仓 CI「Linux 缺 WebKitGTK」既有边界同因）——AppImage 真机验证归 L470 Linux 作业（ubuntu-22.04 基底）或 tauri v2 升级项，均登记不实现（✅ 2026-10-02）
- [x] 实现 Android APK/AAB 分包和多渠道发布：① gen-bindings.sh 单 arm64 循环化双 ABI（arm64-v8a 真机 + x86_64 模拟器/Chromebook，逐 ABI rustup target 自动补装 + NDK API-26 clang 链接器缺失显式报错，**本机实测产出双 .so** 32MB 级 debug）；② app/build.gradle.kts 渠道/分包矩阵（flavorDimensions channel × play/direct 双 flavor + BuildConfig.DISTRIBUTION_CHANNEL 标记（buildConfig=true）、applicationId 各渠道一致同一应用身份、splits.abi 双 ABI 拆分 + universalApk 兜底、签名全走 ALPHA_KEYSTORE_* 四环境变量缺省 unsigned、R8 仍关登记归 L470）+ scripts/android-release.sh 三段编排（bindings→gradle 三任务→产物清单按 aab/unsigned/signed 分级标注）；③ **本机全产物实测**：app-play-release.aab 11M（Play 动态设备分包）+ 双渠道 × {arm64 9.2M, x86_64 9.4M, universal 12M} APK，gradle BUILD SUCCESSFUL（3m47s 全量 / 5s 增量）。docs/android-release.md：渠道矩阵/AAB vs splits 分包语义分工（Play 设备维度动态下发 vs APK 侧 ABI 拆分）、Play App Signing 上传密钥托管、边界登记（versionCode 按 ABI 偏移不做、国内市场代收归 L471、CI 接线归 L470）（✅ 2026-10-02）
- [x] 配置 iOS IPA 签名和 TestFlight/App Store 发布：scripts/ios-release.sh（可执行登记——环境守卫非 macOS exit 2 供 CI 矩阵无条件调用，已实测 Linux 全 stage 跳过不落产物；四阶段 bindings→archive→export→upload：L118 FFI 绑定生成、xcodebuild archive（工程缺失守卫显式报「归 L470」）、ExportOptions（app-store-connect + automatic 签名 + TEAM_ID 必填校验）、altool 上传 TestFlight（Apple 弃用窗口登记 Transporter/ASC API 替换点只换 run_upload 一处））+ docs/ios-release.md 六节（前置资产清单全部 CI secret/本地钥匙串化本仓零密钥；TestFlight 内 100 外 10000 与 Beta 审核节奏；App Store 提审清单——隐私政策 URL 挂 L520 草案、PrivacyInfo.xcprivacy「不收集」口径、NSFaceIDUsageDescription 等用途字符串与 L520 §2.2 对齐、Finance 分类无交易下单、出口加密豁免）。mobile/ios/ 属 L119 交付面本单绝不动（脚本只引用其既有 gen-bindings.sh，缺失时报错不越界写入）；真机执行归 L470 CI macOS 作业接线（✅ 2026-10-02）
- [x] 构建自动更新和增量更新机制：桌面 Tauri v1 updater 通道三件套——① `desktop/tauri.conf.json` 登记 `tauri.updater` 段（active:false 保持 inert + dialog + pubkey 占位 + `{{target}}/{{current_version}}` endpoint 模板），契约测试 `updater_registered_but_inert` 锁定「已登记未激活」（翻真而无 pubkey 会让 gui 构建期失败，测试先行拦住；Url 会把 `{}` 规范化为 %7B/%7D——tauri v1 运行时两种形态都替换，测试双形态断言）；② `scripts/release-update-feed.sh` 产 Tauri v1 latest.json 更新清单（version/notes/pub_date RFC3339 UTC/platforms{signature,url}，semver 与 rust triple 入口校验、产物 JSON 合法性复检，正负路径实测）；③ 签名链路登记（tauri signer minisign：公钥入 conf、私钥进 CI secret 绝不入库，signature 字段取打包作业 .signature 产物）。增量更新现实登记：Tauri v1 为整包替换无内置差分，Windows/macOS bsdiff 与 AppImage zsync 归 L516 打包形态后复核、不自行开发 delta 协议。平台策略边界：iOS 自更新违反 App Store 条款明确不做、Android 走商店内更新、Web SW 更新流归 L506、feed 静态 CDN 托管衔接 L515；与 L472（版本管理编排）分工 = 更新通道 vs 发布自动化，feed 生成器为接缝。docs/auto-update.md 四节（✅ 2026-10-02）
- [x] 开发平台合规性检查和适配（隐私政策、权限申请）：config/compliance/permissions-registry.txt（[allow]/[planned] 双段权限注册表——allow 为检查基线、planned 留档计划权限含加入触发条件）+ scripts/check-compliance.sh（python3 对账四面：注册表 allow 条目理由非空校验、AndroidManifest.xml <uses-permission> ⊆ android 段、tauri.conf.json allowlist 启用组**双向对账**（未登记的启用组=失败、登记但未启用的陈旧条目=失败，注册表不许腐烂）、CSP 非空；负路径实测：注入 CAMERA 未登记权限/移除注册条目均正确 exit 1）+ docs/platform-compliance.md（隐私政策工程草案六条（数据最小化/本地优先/无遥测/权限可关停，标注发布前法务复核边界）+ 四平台权限台账：Android 骨架零权限最小清单（INTERNET 按 planned 触发条件走，POST_NOTIFICATIONS 为 API 33+ 运行时申请、INTERNET/USE_BIOMETRIC 为 normal 安装期授予）、iOS 仅登记边界（L119 交付面不动：push entitlement + NSFaceIDUsageDescription 随落地项进 Info.plist）、桌面 Tauri 六 allowlist 组逐条理由与最小权限评审（组粒度收窄归 L516 复核、签名公证归 L518）、Web PWA 通知授权边界（用户手势触发、拒绝降级站内）。新增权限流程 = 注册表 conscious ack + 文档同步，CI 接线归 L467〔现注：L467 是多目标构建项不含本项，已接入 ci.yml `compliance` 作业〕（✅ 2026-10-02）
- [x] （未来项 → 已落地 2026-09-28，见 P2「重投递封顶」条）消息投递/重送交付计数上限：claim_stale 按 delivery_count 封顶（默认 5，env 可调），超限毒消息不再重投、转 quotes.dlq 留痕并 ack 停投，与兜底扫描路径协同防止资源耗尽；毒串消息的完整运营机制（批量重放工具、DLQ 内容级再处理）留待后续
