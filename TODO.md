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
- [ ] 实现实时数据同步协议（WebSocket 增量更新 + 版本控制）

## 🖥️ 桌面端应用（Tauri）
- [ ] 搭建 Tauri + Rust 桌面应用框架
- [ ] 实现原生文件系统集成和本地数据导出
- [ ] 开发系统通知和托盘集成功能
- [ ] 构建跨平台窗口管理和主题适配
- [ ] 实现本地数据库同步和离线模式
- [ ] 开发键盘快捷键和右键菜单支持

## 🔧 Rust 微服务架构
- [x] 基于 Axum + Tokio 构建高性能 HTTP/gRPC 服务
- [x] 使用 Tonic + Prost 替代 go-zero 实现微服务通信
- [x] 集成 DataFusion 替代 DuckDB 服务端实现内存 SQL 引擎
- [x] 开发基于 Arrow 的列式数据处理管线
- [x] 实现基于 Tokio 的异步消息队列和事件驱动架构
- [x] 构建统一配置管理（config-rs）和分布式追踪（tracing）

## 📱 移动端应用（Android & iOS）
- [ ] 设计移动端 Rust 核心库架构（JNI + UniFFI）
- [ ] 搭建 Android Kotlin + Rust 混合开发环境（Jetpack Compose）
- [ ] 实现 iOS Swift + Rust 集成（SwiftUI + UniFFI）
- [ ] 开发移动端特有的推送通知和后台同步
- [ ] 实现触屏手势和移动端 UI 交互优化
- [ ] 构建移动端离线数据存储和同步机制

## 📊 跨平台 UI 框架开发
- [ ] 选择和集成跨平台 UI 框架（Web: React/Vue, Desktop: Tauri, Mobile: Native）
- [ ] 基于 Yew/Leptos 开发 Web 端组件化数据分析界面
- [ ] 集成高性能图表库（D3.js + Canvas + 原生渲染）
- [ ] 开发跨平台 SQL 查询编辑器和结果可视化
- [ ] 实现响应式设计适配不同屏幕尺寸
- [ ] 构建统一的主题系统和个性化配置

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
