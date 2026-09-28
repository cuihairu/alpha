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
- [ ] web 前端接 real-time-feed WS + data-engine REST
- [ ] 清理 collector 未挂载死模块与根目录孤儿文件（src/、tests/clickhouse_test.rs）

## 🌍 跨平台 Rust 架构设计
- [ ] 设计统一跨平台架构（packages/、services/、web/、desktop/、mobile/）
- [ ] 配置 Cargo workspace 支持多目标平台构建
- [ ] 建立跨平台共享核心库（core、protocols、storage）
- [ ] 实现平台适配层抽象接口（Desktop、Web、Android、iOS）
- [ ] 定义统一的 Rust 代码规范和跨平台兼容性检查

## 🚀 Rust WASM Web 分析引擎
- [ ] 集成 wasm-bindgen 和 wasm-pack 构建工具链
- [ ] 开发高性能 Rust WASM 核心计算库（指标算法、回测引擎）
- [ ] 实现零拷贝内存管理与 Arrow 数据格式优化
- [ ] 构建混合存储架构（WASM 内存 + IndexedDB + 服务端缓存）
- [ ] 开发流式数据处理和并行计算机制（Web Workers + Rayon）
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
