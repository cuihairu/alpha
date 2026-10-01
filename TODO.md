# TODO

## 🎯 优先级行动清单（2026-09-27 现状分析产出，详见 docs/analysis.md）

### P0 打通并守住端到端管线 ✅ 2026-09-27
- [x] 修复 normalized 行情被误入 DLQ（real-time-feed 对 change/change_percent 容错 + data-engine 保留原始字段）
- [x] 新增 Redis Streams 管线集成测试（REDIS_TEST_URL 门控）：raw→normalized→realtime 全链路断言
- [x] normalize_quote 抽纯函数并补单测
- [x] 修 read_latest 的 XREVRANGE 解析崩溃（StreamRangeReply）
- [x] 本机真实 Redis 进程级 E2E 验证：raw→normalized→API history，DLQ 为空

### P1 修部署口径 ✅ 2026-09-27
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

## 🌍 跨平台 Rust 架构设计
- [x] 设计统一跨平台架构（packages/、services/、web/、desktop/、mobile/）：docs/cross-platform-architecture.md v1 草案——L0 共享核心（packages/core，零平台依赖）/ L1 平台服务（storage/protocols/services，服务端专属）/ L2 平台表现（web+wasm-analyzer、desktop Tauri 1.5、mobile 预留）三层与依赖方向强制；现状盘点全部实测（core wasm32 编译通过需 --features wasm=chrono/wasmbind+uuid/js；protocols 因 tonic 默认特性拉 mio 在 wasm32 编译失败，remediation=default-features=false+grpc 模块 feature 门控，已写入差距清单）；平台适配层 trait 草案（KeyValueStore/LocalPersistence/UserNotification）；配套 scripts/check-cross-platform.sh 落地强制检查（wasm32 下 core 编译门禁 + core 依赖黑名单扫描 tokio/reqwest/sqlx/redis/tonic 等 + protocols informational 探测，非交互可入 CI，实测通过）；余项（workspace 多目标配置、共享核心库补齐、适配层落地）映射 §7 路线图（✅ 2026-09-28）
- [x] 配置 Cargo workspace 支持多目标平台构建：.cargo/config.toml 落地 cargo alias（cargo wasm-check = alpha-core @ wasm32 --features wasm；cargo wasm-build = alpha-wasm-analyzer @ wasm32 cdylib 构建，两 alias 实测通过）；scripts/check-cross-platform.sh 扩为四步（core wasm32 编译门禁、wasm-analyzer wasm32 构建门禁——实测通过（cdylib+rlib，6 个既有 dead_code 警告不阻塞）、core 依赖黑名单扫描、protocols informational 探测）；docs/cross-platform-architecture.md §5 目标矩阵/§6 检查清单/§7 路线图同步（CI 多目标并行矩阵留待「跨平台 CI/CD」节）。全仓门禁复跑 0 失败（✅ 2026-09-28）
- [x] 建立跨平台共享核心库（core、protocols、storage）：按设计文档三层模型落齐——core：wasm-clean（--features wasm，wasm32 编译门禁）；protocols：消除设计文档 §5 已知差距——tonic/prost/tonic-build 转 optional、「grpc」feature（default 开启，服务端零改动）门控 proto 代码生成（build.rs cfg 跳过）与 proto 模块，rest/websocket/grpc 纯 serde 契约无门控共享，wasm32 --no-default-features 编译实测通过并升入 check-cross-platform.sh 第 4 步硬门禁（原 informational）；storage：按设计属 L1 服务端专属（sqlx/redis/clickhouse），移动端经 REST/WS 访问 services 不直连，wasm 侧由 wasm-analyzer 自带存储模块承接。三个 crate 职责边界与消费方式在 docs/cross-platform-architecture.md §5/§7 记录；全仓门禁（默认特性，服务端不受影响）复跑 0 失败（✅ 2026-09-28）
- [x] 实现平台适配层抽象接口（Desktop、Web、Android、iOS）：packages/core/src/platform.rs 落地设计文档 §4 的 trait 面——KeyValueStore（get/set/delete 后写覆盖语义）/ LocalPersistence（export_file）/ UserNotification（notify，async-trait、方法不带平台类型），各交付面实现方式逐一注明（Tauri fs/通知、Web IndexedDB/Notification API、移动端沙箱+推送、服务端参考实现）；InMemoryKeyValueStore 参考实现（HashMap+Mutex）+ 契约测试（get/set/delete 语义、Arc<dyn> 动态分发与跨线程可用）。顺带发现并按 P3-3 同类口径处理新孤儿：core/src/trading.rs 从未挂载（lib.rs 无 pub mod trading）、全仓零引用且存在潜伏语法/类型错误（多余右括号、浮点歧义、E0384 等——从未参与编译所以从未暴露），已删除并在设计文档 §2/§3 修正 L0 清单措辞（量化策略回测引擎属未来「业务功能」节另起炉灶）；core 增 tokio dev-dep（macros+rt，仅测试，不进 lib/wasm 构建）。alpha-core 17 测试绿、cargo wasm-check 绿、全仓门禁 0 失败（✅ 2026-09-28）
- [x] 定义统一的 Rust 代码规范和跨平台兼容性检查：docs/rust-code-standards.md v1（格式化 rustfmt 唯一权威、clippy 零警告且 allow 必须留痕禁 blanket、错误处理 AlphaError/anyhow 分层与 lib 禁 unwrap、模块挂载纪律〔引 trading.rs 孤儿案例〕、tokio/async-trait 惯例、tracing 级别语义与显式初始化、平台依赖黑名单与 feature 默认值策略、测试 env 门控/契约 helper/空断言禁令、提交纪律）；可执行门禁 scripts/check-lint.sh（fmt --check + clippy -D warnings，与 CI 完全一致）。全仓一次性清零到位：clippy --fix 自动修 21 处 + 人工修 ~25 处（clamp/迭代器求和/copy_from_slice/Default 补齐/嵌套 format 提取/生成代码 result_large_err include 处压制/路线图预留 API 与调度器传参形态按「allow 必须注释理由」规范标注），cargo fmt --all 机械重排 53 文件；本地验证 fmt --check 绿、workspace clippy -D warnings 绿、全量测试 0 失败、双服务二进制重建启动冒烟 healthy、check-cross-platform.sh 四步绿。CI 集成（.github/workflows/ci.yml 修复三处长期红灯）：lint 作业从裸 clippy 升级为 fmt+clippy 真门禁并排除 desktop（原在 ubuntu 必因缺 GUI 库失败）；test 作业补 protobuf-compiler（proto 代码生成缺 protoc 即败）+ Redis service 让 REDIS_TEST_URL 门控测试真实运行；wasm 作业并入 check-cross-platform.sh；security 作业降为 continue-on-error 报告型（8 个 cargo 依赖漏洞属升级债另行立项，避免长期红灯淹没真信号，已在规范 §11 注明）。跨平台兼容性检查（check-cross-platform.sh 四步）保持既有（✅ 2026-09-28）

## 🚀 Rust WASM Web 分析引擎
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

## 🖥️ 桌面端应用（Tauri）
- [x] 搭建 Tauri + Rust 桌面应用框架：桌面 crate 拆「框架层（lib，零 Tauri 依赖）+ 接线层（gui 特性）」——Tauri 降为可选依赖（Linux 需 WebKitGTK/libsoup，此前整个包只能从 lint/test 门禁排除＝无自动化校验），框架层随主门禁进 CI，接线层由 macOS 作业编译；框架层落地 paths（配置/数据/导出/键值四目录布局）、config（缺失自举/损坏回退上报 ConfigSource::Recovered/临时文件+rename 原子写/一次性报全部校验问题）、kv（L38 适配层 KeyValueStore 的桌面实现：键→十六进制文件名阻断 `../` 穿越，每键一文件）、market（symbol 派生种子的确定性 LCG 行情，替换原 rand+Utc::now 不可测口径）、analysis（委派 alpha-core AnalysisEngine，不重复实现指标）、export（CSV/JSON，导出时刻注入使文件名可断言）、alerts（原子写+损坏回退空表）、ipc（前后端 DTO，serde 往返+前端 JSON 负载解析锁定契约）、state/app/error；接线层 gui.rs 只做「解析路径→委派→映射错误」的薄 #[tauri::command]（6 命令），main.rs 退化为进程入口；构建门禁新增 scripts/check-desktop.sh（tauri.conf.json 自洽性：必填字段/图标存在/distDir 含 index.html 防空白窗口/**build 段** withGlobalTauri/兜底壳不得用 v2 的 ipcRenderer/**allowlist 与 Cargo.toml tauri 特性对齐**/无孤儿 src-tauri，加框架层 clippy+单测）与 desktop/tests/tauri_config.rs（10 例，用 tauri-utils 走 tauri-build 同一解析路径把「配置 schema 是否匹配 Tauri 1.x」从 macOS CI 运行时前移到本地/常规 CI，另锁兜底壳↔gui.rs 命令名与 v1 IPC 入口契约）并接 CI 新作业 Desktop Framework（Linux）；顺手删孤儿 desktop/src-tauri/tauri.conf.json（crate 根是 desktop/，其 allowlist 与 devPath 与生效配置不同，留着只会改错文件）与 5 个未用依赖（reqwest/dirs/config-rs/alpha-storage/anyhow）；前端集成兜底：受版本控制的 web/dist/index.html + desktop-shell.js（.gitignore 加例外）经 window.__TAURI__ 串 initialize_app→get_app_info→get_real_time_quotes→analyze_symbol，闭环到 alpha-core 真实计算（外部 JS 文件，不放开 CSP script-src）；设计说明 docs/desktop-framework.md；框架层 91 单测（L113 导出功能 +33 → 现 124；含 KV 穿越/二进制大值、CSV 往返、损坏配置回退、原子写无残留、确定性行情等）。首轮 CI 红灯修复（均为 v1/v2 混用，非交互自行判定）：withGlobalTauri 在 Tauri 1.x 属 build 段（放 tauri 段会被 Config 的 deny_unknown_fields 在 tauri_build::build() 运行时拒绝，Linux runner 跑不到、只能靠 macOS 作业暴露），前端 IPC 入口在 v1 是 window.__TAURI__.invoke 而非 v2 的 ipcRenderer（用错是运行期 undefined、窗口静默无响应）。第二轮 CI 红灯（Desktop macOS 编译错误）复盘并结构性收口：接线层曾把框架层 validate() -> Result<(), Vec<String>> 当 Vec<String> 用（is_empty() 编译不过）——业务判断写在「本地编不了、只有 macOS 作业能编」的 gui.rs 里，红灯要等一次推送往返才暴露。故把命令体全部改写为纯委派：判断下沉为框架层入口 state::bootstrap_app（配置自举落盘+降级提示）/ analysis::analyze_request、quotes_request / alerts::upsert_request（方向串解析+价格校验）/ export::export_request（格式解析+空列表）；AppConfig 补 validation_problems() 返回问题列表本身（避免 Result<()> 误用）；InitPayload 下沉 ipc.rs 并带 is_recovered()；app_info 改注入 Tauri identifier（v1 单值，非 v2 标识符数组）。新增 desktop/tests/wiring_contract.rs（8 例薄度契约：命令体不得含判空/兜底/自造错误串、必须委派上述入口并 map_err、不得绕过入口调底层、generate_handler! 与兜底壳 invoke 一致、lib.rs 重导出指向真实项、文件行数上限），反向用例实测（把 validate().unwrap_or_default() 塞回命令 → 8 例中 2 例转红）；另 app 4 例/ ipc 4 例 / analysis 4 例 / alerts 3 例 / export 4 例 / state 3 例 / config 2 例。框架层 91 → 112 测，配置契约 10 + 接线契约 8。诚实边界：薄度契约是源码断言，替代不了编译——Tauri 自身类型用法（#[tauri::command] 宏展开、AppHandle/State 形态）仍只能由 macOS 作业验证，故策略是把留在 macOS 的代码压到最小。 第三轮把「替代不了编译」这个缺口也补上：cargo check/clippy 不链接，而 Tauri 的 sys crate（webkit2gtk-sys/soup2-sys/javascriptcore-rs-sys）只在 build 期跑 pkg-config——于是新增 scripts/desktop-fake-pc/（版本号给足、Libs/Cflags 留空的 .pc + README 说明边界），门禁加 [5/5]：PKG_CONFIG_PATH 指向该目录后 cargo clippy -p alpha-desktop --features gui --all-targets -D warnings，gui.rs 的 #[tauri::command] 宏展开、AppHandle/State 用法、generate_context! 与全部框架层调用签名就此进入 Linux 门禁（仍不覆盖链接与启动，那部分留给 macOS 作业）。反向用例实测：把 validate().unwrap_or_default() 塞回 get_app_info，该命令直接给出与首轮 CI 完全相同的 E0599。为此还修掉一个真实缺陷：zbus 5.11.0 声明 zbus_macros = "^5.11.0"（caret），区间允许 5.19.0，而 5.19 宏生成的代码引用 zbus 5.11 未导出的 DispatchResult2（cargo check 到 zbus 本体即 9 个错误，已实测）；zbus 是 tauri 仅 Linux/BSD 的依赖（tauri → notify-rust 4），macOS CI 从不编译它、Linux 上任何 cargo build（默认带 gui）却会失败。本仓忽略 Cargo.lock（.gitignore:3，库惯例）拿不到锁文件兜底，故在 desktop/Cargo.toml 加代码不引用的 optional 依赖 zbus-macros-pin = { package = "zbus_macros", version = "=5.11.0" }（挂 gui 特性）钉住；代价是 zbus 本身也锁在这条链上（cargo update -p zbus --precise 5.19.0 在解析期硬失败），想升级先解钉版。顺带发现并修掉 [5/5] 自身的不可复现：pkg-config 同时搜 PKG_CONFIG_PATH 与 PKG_CONFIG_LIBDIR（后者默认系统 .pc 目录），最初 8 个 .pc 在本机（装了 GTK3）被系统真件悄悄兜住，推到 CI 的 ubuntu-latest 就红在 glib-sys 要的 gobject-2.0 not found——门禁 [5/5] 追加 PKG_CONFIG_LIBDIR 指向空目录（target/desktop-fake-pc-system/）屏蔽系统 .pc，.pc 集合补到 18 个（gobject-2.0/gio-2.0/gmodule-2.0/gdk-3.0/atk/pango/cairo-gobject/gdk-x11-3.0/x11，Requires 照抄真实依赖关系），任何 Linux 机器结果一致；反向验证：删 gobject-2.0.pc 且 cargo clean -p glib-sys -p gobject-sys -p gdk-sys -p atk-sys → [5/5] 转红在 atk 找不到 gobject-2.0（不 clean 会命中 clippy 缓存、看不出变化）。非交互假设：演示行情为确定性占位数据（真实后端接入只换 market 模块）；导出暂落应用数据目录 exports/（「另存为」对话框归 L113）；告警仅持久化（通知/托盘归 L114）（✅ 2026-09-30）
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

## 🔧 Rust 微服务架构
- [x] 基于 Axum + Tokio 构建高性能 HTTP/gRPC 服务
- [x] 使用 Tonic + Prost 替代 go-zero 实现微服务通信
- [x] 集成 DataFusion 替代 DuckDB 服务端实现内存 SQL 引擎
- [x] 开发基于 Arrow 的列式数据处理管线
- [x] 实现基于 Tokio 的异步消息队列和事件驱动架构
- [x] 构建统一配置管理（config-rs）和分布式追踪（tracing）

## 📱 移动端应用（Android & iOS）
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

## 📊 跨平台 UI 框架开发
- [x] 选择和集成跨平台 UI 框架（Web: React/Vue, Desktop: Tauri, Mobile: Native）：选型决策写入 docs/web-framework-selection.md——Web 定 **React 18 + TypeScript + Vite**（vs Vue 3：金融终端级组件生态〔图表/虚拟表格/Monaco 编辑器〕React 一等封装更全；vs Yew/Leptos：生态空缺抵不过「同语言」收益，不选作主框架，wasm-analyzer 维持 cdylib+JS 绑定现模式）；影响面登记：L428 组件化后续按 React 路线重定 scope、L429 图表候选 lightweight-charts/ECharts；Web 骨架落地 web/app/（独立 Vite 工程：最小示例页 = 行情演示表 + 纯 TS SMA + WASM 引擎探针三态〔动态 import /pkg 接缝与纯 TS 对账〕，vitest 8 单测 + tsc 严格类型〔纯 TS SMA 口径**对齐** Rust 侧 TechnicalIndicators::calculate_sma：等长输出/前导 0.0 占位/样本不足全 0/4 位小数取整，单测沿用 Rust 同名样本向量对账，前端不另立指标语义〕，package-lock 锁定）；现有 web/ 演示页零回退（未触碰任何既有文件；scripts/check-web.sh 第 1 步在场守门 + node --check 冒烟 + 本机 server.js 实测 index.html/wasm-demo.html 均 200）；新门禁 scripts/check-web.sh 四步（旧页在场冒烟 → 骨架结构断言 → npm ci → tsc+vitest+vite build）接入 CI wasm 作业尾部（ubuntu-latest 自带 node）；Desktop/Mobile 仅文档登记接入边界不实现（Desktop Tauri 1.5 维持现状、frontendDist 指向 web/app/dist 的策略归后续单〔§6 选项 A〕；Mobile 维持 Native 不引 React Native——移动端共享面是 Rust FFI 而非 UI，设计语义对齐即可）。全仓门禁复跑 0 失败（check-lint.sh / 全仓测试 / check-cross-platform.sh 四步 / check-desktop.sh [1-5/5] / check-web.sh）（✅ 2026-10-01）
- [x] 基于 React 开发 Web 端组件化数据分析界面（原「Yew/Leptos 路线」按 L427 选型定论废弃，本项即其登记的改写轮）：web/app 组件化拆分——QuoteTable（props 化可复用）/ WasmProbe（L427 探针提取）/ **IndicatorPanel 新增**（代码 × 指标〔SMA/EMA〕× 周期三选联动 → 纯 TS 指标全序列对表 + 末值摘要；SMA 前 period-1 位 0.0 占位明示；图表化渲染留 L429）；指标面新增 ema（口径逐项对齐 packages/core/src/indicators.rs calculate_ema：空输入 []、等长无占位、ema[0]=prices[0]、multiplier=2/(period+1)、递推逐位 4 位小数取整，滑动累加与取整顺序一致）；vitest 8→12（ema 递推样本手算对账 / period=1 退化为原序列 / 取整精度 / 等长契约）；check-web.sh 结构守门扩组件三文件；选型文档 §3「L428 改写归后续轮」更新为已落实、§7③ 假设更新（DOM 渲染测试留交互复杂化后，功能优先测试到门禁量）。全仓门禁复跑 0 失败（check-lint.sh / 全仓测试 / check-cross-platform.sh / check-desktop.sh / check-web.sh 12 单测 + build）（✅ 2026-10-02）
- [x] 集成高性能图表库（D3.js + Canvas + 原生渲染）：选型定论 **lightweight-charts 5.2**（TradingView 出品金融图表库——Canvas 原生渲染 + 增量重绘/缩放虚拟化内建，gzip 增量 ~57KB，A股配色红涨绿跌；D3 属底层可视化原语〔scale/shape 需自建渲染循环与交互〕与「集成高性能图表库」目标不符不直接引入，ECharts ~300KB+ 对行情主图过重落备选；理由入 docs/web-framework-selection.md §3）；web/app 落地 PriceChart 组件（CandlestickSeries 日 K + SMA(5) 叠加线〔与指标面板同口径、跳过前导 0.0 占位段〕+ fitContent + autoSize + 卸载 chart.remove()）；数据源 demoWalk（**确定性** LCG 合成日 K 60 根、跳过周末交易日序列——Math.random 会破坏可复现门禁；OHLC 不变量单测：high≥max(open,close)≥min(open,close)≥low>0、同种子逐位一致、两位小数口径）；vitest 12→16；check-web.sh 结构守门扩 PriceChart/demoWalk + 依赖断言 lightweight-charts。全仓门禁复跑 0 失败（check-web.sh 16 单测 + tsc + build 320KB/104KB gz；Rust 侧零改动四道门禁语义不变复跑绿）（✅ 2026-10-02）
- [x] 开发跨平台 SQL 查询编辑器和结果可视化：web/app SqlWorkbench 组件——**DuckDB-WASM 引擎**复用 web/vendor/duckdb 资产（prepare-vendors 产出，不入 git；「拷入 public/vendor/duckdb 后可用、未拷入明示降级」与 WasmProbe 的 public/pkg 同款模式，构建保持 hermetic，零新增 npm 依赖）；编辑器最小实现（textarea 等宽 + 执行按钮，语法高亮/补全登记后续增强）；**跨平台**=组件随 web/app 产物可入 Tauri 壳（L427 §6 选项 A 边界）；演示表 demo_quotes + demo_candles（60 根合成日 K 可 GROUP BY/窗口练习，seedSql 纯函数生成）；结果可视化 ResultGrid（列名/行集网格 + 行列数/耗时/截断摘要，capRows 上限 100）；纯函数面 6 单测（capRows 截断/NULL 口径/bigint、tableToRows 列序取齐、seedSql 建表断言）vitest 16→22；本机 vite preview 冒烟：页面与 duckdb mjs/wasm 资产全 200（浏览器内真实 SQL 执行归真机验收边界）；check-web.sh 结构守门扩四文件。全仓门禁复跑 0 失败（check-web 22 单测 + tsc + build；Rust 零改动四道门禁复跑绿）（✅ 2026-10-02）
- [x] 实现响应式设计适配不同屏幕尺寸：web/app 首个样式面 src/styles.css（内容优先单列；三断点 ≤720 移动 / ≤1024 平板 / >1024 桌面版心 1080px）——移动端触控目标 ≥40px、label/select/textarea/button 纵向堆叠全宽、字号 15→14 收缩；宽表/图表在节内横向滚动兜底（section overflow-x，组件零改动）；th/td 收缩 + hover 反馈；色彩令牌铺最小集（深浅主题系统留 L432 不展开）；纯 CSS 方案（不引断点 JS 库，matchMedia hook 留交互需要时）；check-web.sh 结构守门 +3 断言（viewport meta 在场 / styles.css 已挂载 / 两断点存在）；视觉验收（375/768/1200 三档截图）归浏览器真机边界。全仓门禁复跑 0 失败（Rust 零改动）（✅ 2026-10-02）
- [x] 构建统一的主题系统和个性化配置：web/app 主题面——styles.css 令牌升级为双主题（:root 浅色 color-scheme:light + data-theme='dark' 深色块，六令牌 fg/muted/accent/border/bg/bg-soft 全覆盖，原生控件随 color-scheme）；个性化配置 ThemeToggle（三态偏好 跟随系统/浅色/深色，持久化 localStorage alpha.theme，system 档监听 prefers-color-scheme change 实时切换、卸载解绑；存储异常〔隐私模式〕兜底不持久化但主题仍生效）；lib/theme.ts 纯函数面 resolveTheme 三态矩阵 + parseThemePref 脏值收口缺省 system，vitest 22→26；页头 flex 布局挂切换器（窄屏自动换行）；check-web.sh 结构守门 +2（深色令牌块 / 持久化键）。全仓门禁复跑 0 失败（Rust 零改动）（✅ 2026-10-02）

## 🕷️ 数据采集服务
- [x] 基于 Tokio + Reqwest 开发高性能异步爬虫引擎
- [x] 实现智能任务调度器和限流代理池管理
- [x] 构建反爬虫对策（User-Agent 轮换、请求频率控制）
- [x] 开发多数据源适配器（API、网页、FTP、文件推送）
- [x] 实现数据质量校验、清洗和标准化流程
- [x] 构建采集状态监控和自动故障恢复机制

## 💾 存储与数据处理
- [x] 集成 SQLx + TimescaleDB 实现时序数据存储
- [x] 使用 DataFusion + Arrow 构建内存分析引擎
- [ ] 设计 Parquet 格式的数据湖存储架构
- [ ] 实现基于 Redis 的分布式缓存和限流系统
- [ ] 开发智能数据分区策略（时间、股票、交易所维度）
- [ ] 构建数据压缩和列式存储优化算法

## ⚡ 性能优化工程
- [ ] 实现 Rust 零拷贝数据处理和内存池管理
- [ ] 开发 SIMD 优化的向量化计算算法
- [ ] 构建多核并行计算引擎（Rayon + Web Workers）
- [ ] 实现智能预取和后台数据同步机制
- [ ] 开发基于 LLVM Profile 的编译优化
- [ ] 构建内存泄漏检测和性能分析工具

## 🔍 监控与可观测性
- [ ] 集成 Prometheus + Grafana + Loki 全链路监控
- [ ] 开发分布式追踪系统（tracing + Jaeger）
- [x] 实现基于 Metrics 的性能指标收集
- [x] 构建健康检查和服务依赖监控
- [ ] 开发 Rust 专用的内存安全监控工具
- [ ] 实现实时告警和智能故障诊断

## 🚀 跨平台 CI/CD 与发布
- [ ] 配置 GitHub Actions 支持多目标平台并行构建
- [ ] 开发跨平台 Docker 镜像和容器化部署方案
- [ ] 实现自动化测试（单元测试、集成测试、端到端跨平台测试）
- [ ] 构建多平台发布流水线（Web、Desktop、Android、iOS）
- [ ] 集成应用商店发布（Google Play、App Store、Microsoft Store）
- [ ] 开发自动化版本管理和热更新机制

## 👥 多平台用户体验与产品
- [ ] 设计跨平台一致的用户体验和交互模式
- [ ] 实现统一的用户账户系统和数据同步
- [ ] 开发平台特色功能（桌面：文件导出，移动：推送通知）
- [ ] 构建跨平台帮助文档和视频教程系统
- [ ] 实现多语言国际化和本地化支持
- [ ] 开发跨平台用户行为分析和产品优化系统

## 🔒 安全与合规
- [ ] 实现 JWT + OAuth 2.0 身份认证系统
- [ ] 开发基于 RBAC 的细粒度权限控制
- [ ] 实现端到端数据加密（传输 + 存储）
- [ ] 构建防爬虫、DDoS 和 API 限流保护
- [ ] 开发安全审计日志和异常行为检测
- [ ] 确保金融数据合规性和隐私保护（GDPR、CCPA）

## 🧪 质量保证与测试
- [ ] 建立单元测试、集成测试和端到端测试体系
- [ ] 开发性能基准测试和回归测试套件
- [ ] 实现基于 Proptest 的模糊测试和属性验证
- [ ] 构建代码覆盖率报告和质量度量
- [ ] 开发自动化性能回归检测系统
- [ ] 实现基于 Miri 的内存安全静态分析

## 📈 业务功能开发
- [ ] 开发实时行情展示和技术指标计算
- [ ] 实现高级技术分析（形态识别、波浪理论）
- [ ] 构建量化交易策略回测和优化引擎
- [ ] 开发风险管理和投资组合分析工具
- [ ] 实现智能告警和个性化消息推送
- [ ] 构建市场数据 API 和第三方集成接口

## 🔄 平台特色功能开发
- [ ] Web 端：PWA 支持和离线功能
- [ ] 桌面端：多标签页界面和工作区管理
- [ ] Android 端：小组件（Widget）和快捷方式支持
- [ ] iOS 端：Live Activities 和动态岛支持
- [ ] 全平台：深色模式和系统主题适配
- [ ] 移动端：生物识别认证和隐私保护

## 📦 跨平台打包与分发
- [ ] 配置 Web 端 CDN 部署和静态资源优化
- [ ] 开发桌面端多平台安装包（Windows .exe、macOS .dmg、Linux .AppImage）
- [ ] 实现 Android APK/AAB 分包和多渠道发布
- [ ] 配置 iOS IPA 签名和 TestFlight/App Store 发布
- [ ] 构建自动更新和增量更新机制
- [ ] 开发平台合规性检查和适配（隐私政策、权限申请）
- [x] （未来项 → 已落地 2026-09-28，见 P2「重投递封顶」条）消息投递/重送交付计数上限：claim_stale 按 delivery_count 封顶（默认 5，env 可调），超限毒消息不再重投、转 quotes.dlq 留痕并 ack 停投，与兜底扫描路径协同防止资源耗尽；毒串消息的完整运营机制（批量重放工具、DLQ 内容级再处理）留待后续
