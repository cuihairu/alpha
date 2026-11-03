# TODO

> 交付台账：`[x]` 为已完成项，行号随编辑漂移不作引用依据；现状以 README.md、docs/ 与代码为准。
> 证据格式：`（实现提交；测试数；文档引用）`，一处一行。

## 优先级行动清单（2026-09-27 现状分析产出，详见 docs/analysis.md）

### P0 打通并守住端到端管线
- [x] 修复 normalized 行情误入 DLQ（real-time-feed 字段容错 + data-engine 保留原始字段）（48c2686）
- [x] 新增 Redis Streams 管线集成测试：raw→normalized→realtime 全链路断言（REDIS_TEST_URL 门控）（48c2686）
- [x] normalize_quote 抽纯函数并补单测（48c2686）
- [x] 修 read_latest 的 XREVRANGE 解析崩溃（48c2686）
- [x] 本机真实 Redis 进程级 E2E 验证：raw→normalized→API history，DLQ 为空（48c2686）

### P1 修部署口径
- [x] collector 端口统一 8083（ALPHA_COLLECTOR_BIND 可配），对齐 compose/Dockerfile/dev-start（d63149d）
- [x] compose 补 REDIS_URL 与 ALPHA__CLICKHOUSE__*，加 depends_on redis（d63149d）
- [x] 修 4 个服务 Dockerfile workspace 拷贝不完整（COPY . + .dockerignore）（d63149d）
- [x] 解码失败消息隔离进 quotes.dlq（GroupRead 分离 invalid，publish_dlq + ack，DLQ 发布失败不 ack）（47824ba）
- [x] 孤儿 pending 消息清理：claim_stale 认领重放，data-engine/real-time-feed 各起独立 sweeper（58a09ff）

### P2 数据落地
- [x] data-engine 按 persistence_enabled/timescale_url 装配 Timescale 落库，三态降级（8fda0c5）
- [x] normalized 层去重（payload_hash 窗口判重，跨重启由 Timescale UPSERT 兜底）（0f5b03b）
- [x] 移除 MemTable 全量重建热点（/query 执行前自刷新），修 register_table 撞名（0f5b03b）
- [x] 内存时序重写：二分有序插入 + UPSERT，批量灌入 48 万点降至 6.4s（e5168bf）
- [x] 重投递封顶：delivery_count 默认 5，超限毒消息转 quotes.dlq 停投留痕（a46cf75）
- [x] real-time-feed 显式日志级别初始化（默认 info，ALPHA_LOG_LEVEL 可调）（55d3fd5）
- [x] 持久化镜像失败重试：有界 FIFO 缓冲 + 后台补写（非完整 outbox，边界已在代码注明）（636ba85）
- [x] compose 透传持久化 env 为 ${VAR:-默认} 形式，宿主机可覆写（7300fac）

### P3 补对外链路
- [x] api-gateway 真实反代：/api/v1/* 透传、/ws 双向泵、/health 三上游聚合探测（0de3097）
- [x] web 前端接 real-time-feed WS + data-engine REST（真实优先、不可达回退演示）（5c643d7）
- [x] 清理 collector 未挂载死模块与根目录孤儿文件（17313c6）

## 跨平台 Rust 架构设计
- [x] 设计统一跨平台架构：三层模型 + check-cross-platform.sh 门禁（afff857；docs/cross-platform-architecture.md）
- [x] Cargo workspace 多目标构建：cargo wasm-check/wasm-build alias，门禁扩为四步（28b3688）
- [x] 跨平台共享核心库：core wasm-clean、protocols grpc feature 门控、storage 服务端专属（b437412）
- [x] 平台适配层抽象：KeyValueStore/LocalPersistence/UserNotification trait + InMemory 实现与契约测试（f6b44cd）
- [x] 统一 Rust 代码规范：fmt/clippy 硬门禁 + CI 三处长期红灯修复（d69e864；docs/rust-code-standards.md）

## Rust WASM Web 分析引擎
- [x] 集成 wasm-bindgen 和 wasm-pack 构建工具链（d02e003）
- [x] WASM 核心计算库：指标算法绑定 + backtestSmaCross 回测绑定（d02e003）
- [x] 零拷贝内存管理与 Arrow 优化：shared_buffer + ArrowBatch + backtestSmaCrossPtr，A/B 基准实测（a8aaa97；59d4eef）
- [x] 混合存储架构：HybridCacheService 三层 read-through + wasm HybridStorage + IndexedDB 缝（034ff84）
- [x] 流式数据处理与并行计算：ParallelStreamProcessor 双路径（Rayon/WorkerPool）（f930375）
- [x] 实时数据同步协议：SyncEngine Full/Delta/Gap/Resync + 服务端 seq 分配 + 前端合入（bd900f2；docs/realtime-sync-protocol.md）

## 桌面端应用（Tauri）
- [x] 搭建 Tauri 桌面框架：框架层/接线层拆分 + check-desktop.sh 五步门禁 + 配置/接线契约测试 + fake pkg-config Linux 门禁（d23bda4；docs/desktop-framework.md）
- [x] 原生文件系统集成与本地导出：export_symbol_request 严格校验 + 原子落盘 + dialog.save 壳（d23bda4）
- [x] 系统通知和托盘集成：NotificationQueue + 托盘菜单状态机 + 告警检查链（platform.rs 机械翻译纪律）（d23bda4）
- [x] 跨平台窗口管理和主题适配：window.rs 放置决策/主题映射 + window_gui 接线（d23bda4）
- [x] 本地数据库同步和离线模式：offline.rs 降级矩阵/增量同步 + QuoteRemote 缝（d23bda4）
- [x] 键盘快捷键和右键菜单：DEFAULT_SHORTCUTS + context_menu 模型 + shortcut_gui 接线（d23bda4）

## Rust 微服务架构
- [x] 基于 Axum + Tokio 构建高性能 HTTP/gRPC 服务（2d32fa7）
- [x] 使用 Tonic + Prost 替代 go-zero 实现微服务通信（2d32fa7）
- [x] 集成 DataFusion 替代 DuckDB 服务端实现内存 SQL 引擎（2d32fa7）
- [x] 开发基于 Arrow 的列式数据处理管线（2d32fa7）
- [x] 实现基于 Tokio 的异步消息队列和事件驱动架构（2d32fa7）
- [x] 构建统一配置管理（config-rs）和分布式追踪（tracing）（2d32fa7）

## 移动端应用（Android & iOS）
- [x] 移动端 Rust 核心库架构（JNI + UniFFI）：alpha-mobile 骨架 16 单测（7a606e9；docs/mobile-core-architecture.md）
- [x] Android Kotlin + Rust 混合环境（Jetpack Compose）：Gradle 工程 + uniffi 绑定 + NDK 交叉编译本机实证（7a606e9）
- [x] iOS Swift + Rust 集成（SwiftUI + UniFFI）（7a606e9）
- [x] 移动端推送通知和后台同步：Notifier/SyncTrigger/BackgroundSync + FFI 六方法 + PushSyncSeam（d839f52；docs/mobile-push-sync.md）
- [x] 触屏手势和移动端 UI 交互：下拉刷新/长按分析/双击折叠，核心库零改动（67e023f；docs/mobile-gestures.md）
- [x] 移动端离线数据存储和同步：授权红线三条款 + 备份/同步术语分离（9bb0c69；docs/mobile-offline.md）

## 跨平台 UI 框架开发
- [x] 跨平台 UI 框架选型与集成：React 18 + TypeScript + Vite（a135ce6；docs/web-framework-selection.md）
- [x] React 组件化数据分析界面：QuoteTable/WasmProbe/IndicatorPanel + EMA 指标（vitest 12）（a135ce6）
- [x] 集成高性能图表库：lightweight-charts 5.2 PriceChart（日 K + SMA 叠加，确定性数据）（vitest 16）（a135ce6）
- [x] 跨平台 SQL 查询编辑器：SqlWorkbench + DuckDB-WASM + ResultGrid（vitest 22）（a135ce6）
- [x] 响应式设计：三断点单列布局 + 触控目标 ≥40px（视觉验收归浏览器真机）（a135ce6）
- [x] 统一主题系统和个性化配置：双主题令牌 + ThemeToggle 三态偏好持久化（vitest 26）（a135ce6）

## 数据采集服务
- [x] 基于 Tokio + Reqwest 开发高性能异步爬虫引擎（2d32fa7）
- [x] 实现智能任务调度器和限流代理池管理（2d32fa7）
- [x] 构建反爬虫对策（User-Agent 轮换、请求频率控制）（2d32fa7）
- [x] 开发多数据源适配器（API、网页、FTP、文件推送）（2d32fa7）
- [x] 实现数据质量校验、清洗和标准化流程（2d32fa7）
- [x] 构建采集状态监控和自动故障恢复机制（2d32fa7）
- [x] 数据源健康面：SourceHealthTracker 三态 + GET /sources/health + gauge 与两档告警（collector 85 测）（17313c6）

## 存储与数据处理
- [x] 集成 SQLx + TimescaleDB 实现时序数据存储（2d32fa7）
- [x] 使用 DataFusion + Arrow 构建内存分析引擎（2d32fa7）
- [x] Parquet 数据湖架构：LakeWriter 落地（分区写入/读回/compaction/manifest/定时维护）（1bce1b1；docs/data-lake-parquet.md）
- [x] Redis 分布式缓存和限流：DistributedCache cache-aside + RedisRateLimiter Lua 固定窗口 + gateway 接线（storage 34 测/gateway 9 测）（2d32fa7）
- [x] 智能数据分区策略：partition.rs 时间/交易所/桶三维映射 + compaction 装箱计划器（2d32fa7）
- [x] 数据压缩和列式存储优化：columnar.rs Delta/RLE/Dictionary 编码器 + 决策矩阵（2d32fa7）

## 性能优化工程
- [x] 零拷贝数据处理和内存池：BufferPool 双阈值回收 + QuoteFrame 零堆解码（2d32fa7）
- [x] SIMD 向量化计算：AVX2 4-lane 与 portable 参考实现位级一致（2d32fa7）
- [x] 多核并行计算引擎（Rayon + Web Workers）：parallel.rs 双路等价 + worker.rs 线程池（2d32fa7）
- [x] 智能预取和后台数据同步：BackgroundPrefetcher 等差检测 + 在飞去重（2d32fa7）
- [x] LLVM Profile 编译优化：PGO 四阶段构建脚本（gateway 实测跑通）（de1ad94；docs/pgo-build-optimization.md）
- [x] 内存泄漏检测和性能分析：TrackingAllocator + perf 封装（8f9d0ba；docs/memory-profiling.md）

## 监控与可观测性
- [x] Prometheus + Grafana + Loki 全链路监控：四服务 /metrics + compose Loki/Promtail + provisioning 仪表盘（2d32fa7）
- [x] 分布式追踪：gateway X-Trace-Id 贯穿（Jaeger OTLP 全量导出登记不实现）（2fae7e8；docs/distributed-tracing.md）
- [x] 实现基于 Metrics 的性能指标收集（2d32fa7）
- [x] 构建健康检查和服务依赖监控（2d32fa7）
- [x] Rust 内存安全监控：safety_audit.rs 清单扫描 + 编译期 deny 双舱位豁免（2d32fa7）
- [x] 实时告警和智能故障诊断：11 条告警 + Alertmanager + alert-webhook + diagnosis 引擎 + alpha-diagnose CLI（f86630f；docs/alerting-and-diagnosis.md）

## 跨平台 CI/CD 与发布
- [x] GitHub Actions 多目标并行构建：五目标矩阵 fail-fast=false（2d32fa7）
- [x] Docker 镜像和容器化部署：五服务 Dockerfile 硬化 + build-images.sh（6d88359；docs/docker-deployment.md）
- [x] 自动化测试体系（单元/集成/端到端）：check-e2e.sh 四服务五段断言 + CI e2e 矩阵（2d32fa7）
- [x] 多平台发布流水线：release.yml 七作业（version-gate/web/desktop/android/ios/docker/updater-feed），只产 artifacts 不建 Release（2d32fa7）
- [x] 应用商店发布面：store/ 元数据（真机提审归发版时人工）（faf7c9c；docs/store-publishing.md）
- [x] 自动化版本管理和热更新：bump-version.sh 一处输入五处同改 + check-version.sh 门禁（b8e67a4；docs/version-management.md）

## 多平台用户体验与产品
- [x] 跨平台一致用户体验：对账表 + 平盘归属收敛（Android 39 测）（31f97fb；docs/ux-consistency.md）
- [x] 统一用户账户系统和数据同步：account.rs 纯函数领域面（档案/三方合并/同步往返/状态机，19 单测）（6e02420；docs/account-sync.md）
- [x] 平台特色功能（桌面文件导出、移动推送通知）：exportCsv.ts + 既有推送面复用（vitest 64）（2d32fa7）
- [x] 跨平台帮助文档和视频教程：补齐文档站四页 + docs/user-guide.md（视频管线未立项）（15294ec；docs/user-guide.md）
- [x] 多语言国际化和本地化：web 中英双语 i18n.ts 字典基座（vitest 70）（2d32fa7）
- [x] 用户行为分析和产品优化：behavior.rs 纯函数 + web analytics.ts opt-in 采集缓冲（vitest 74）（2d32fa7）

## 安全与合规
- [x] JWT + OAuth 2.0 身份认证：gateway auth.rs HS256 签发/校验 + OIDC JWKS + WS 握手鉴权（6439795；docs/auth.md）
- [x] RBAC 细粒度权限控制：roles claims + authorize 纯函数 + 403 中间件（6439795；docs/auth.md §6）
- [x] 端到端数据加密：crypto.rs AES-256-GCM + EncryptedStorage 包装器 + 列级加密辅助（feature 门控）（2d32fa7）
- [x] 防爬虫/DDoS/API 限流保护：shield.rs 三件套（UA 分类/路径扫描/burst 令牌桶）+ RedisRateLimiter（gateway 27 测）（2d32fa7）
- [x] 安全审计日志和异常行为检测：audit.rs 审计通道 + AuthFailureDetector 风暴检测（只标记不封禁）（gateway 30 测）（2d32fa7）
- [x] 金融数据合规和隐私保护：privacy.ts 权利工具 + PrivacyPanel + 义务映射（vitest 60）（eb0158a；docs/data-privacy.md）

## 质量保证与测试
- [x] 单元/集成/端到端测试体系：七层统筹 + e2e WS 消息级断言（vitest 53）（ea8f172；docs/testing.md）
- [x] 性能基准测试套件：criterion 三组 8 项 indicators_bench（2d32fa7）
- [x] Proptest 模糊测试和属性验证：9 性质 ×256 案例（2d32fa7）
- [x] 代码覆盖率报告：check-coverage.sh llvm-cov（行覆盖 62.4%，不设阈值）（2d32fa7）
- [x] 自动化性能回归检测：check-perf.sh criterion 基线比对 + CI perf.yml 周滚动基线（2d32fa7）
- [x] Miri 内存安全分析：check-miri.sh 5 种子 + CI miri.yml（2d32fa7）

## 业务功能开发
- [x] 实时行情展示和技术指标计算：liveFeed.ts + useLiveQuotes + LiveQuoteBoard（vitest 41）（2d32fa7）
- [x] 高级技术分析：patterns.rs K 线/线形形态 + Elliott 三规则校验引擎（core 124 测）（2d32fa7）
- [x] 量化回测和优化引擎：grid_search/walk_forward + 过拟合比（2d32fa7）
- [x] 风险管理和投资组合分析：risk.rs 九度量 + RiskReport 聚合（2d32fa7）
- [x] 智能告警和个性化推送：alerts.rs 规则引擎 + real-time-feed 评估接线（2d32fa7）
- [x] 市场数据 API 和第三方集成：X-Api-Key 门 + history.csv 导出（ae8a32a；docs/market-data-api.md）

## 平台特色功能开发
- [x] Web PWA 和离线功能：manifest + sw.js 缓存策略（vitest 52）（2d32fa7）
- [x] 桌面多标签页和工作区管理：workspaces.ts 归约器 + WorkspaceTabs（vitest 49）（2d32fa7）
- [x] Android 小组件和快捷方式：HomeWidget.kt + 深链契约（JVM 39 测）（b46a0ac；docs/android-widget.md）
- [x] iOS Live Activities 和动态岛：工程设计契约登记（mobile/ios 目录不动）（8ee6d0a；docs/ios-live-activities.md）
- [x] 全平台深色模式：web ThemeToggle + Android Theme.kt 三态映射（JVM 43 测）（427bea6；docs/theme-adaptation.md）
- [x] 移动端生物识别认证和隐私保护：BiometricGate + EncryptedKeyValueStore（JVM 40 测）（65d04d3；docs/mobile-privacy.md）

## 跨平台打包与分发
- [x] Web CDN 部署和静态资源优化：optimize-dist.mjs 指纹/预压缩（-84.8%）+ web-origin nginx + web-cdn-deploy.sh（6ad8f9e；docs/web-cdn.md）
- [x] 桌面多平台安装包：desktop-release.sh 三平台产物 + 修 tauri conf 两处真 bug（8db02bc；docs/desktop-release.md）
- [x] Android APK/AAB 分包和多渠道：双 ABI splits + play/direct flavor + aab 本机实测（22e04bf；docs/android-release.md）
- [x] iOS IPA 签名和 TestFlight：ios-release.sh 四阶段（真机执行归 CI macOS 作业）（ada7517；docs/ios-release.md）
- [x] 自动更新机制：Tauri updater 登记 inert + release-update-feed.sh latest.json（616032c；docs/auto-update.md）
- [x] 平台合规性检查：permissions-registry.txt + check-compliance.sh 四面对账（cecc656；docs/platform-compliance.md）
- [x] 消息投递/重送交付计数上限：已并入 P2「重投递封顶」条（a46cf75）

## 拍板登记（2026-10-10 巡检，逐项按仓内决策记录拍板）
- [x] 桌面 frontendDist 指向 web/app/dist（web-framework §6 选项 A）：桥接 + 契约测试 + CI 双桌面作业全落（2026-10-09）
- [x] RustSec 升级债维持报告型不阻塞：收口归类可升级 0 / 误报 2（object_store、rsa）/ 记录风险 3（h2、webpki×3、sqlx），逐条证据见 docs/rust-code-standards.md §12
- [x] Cargo.lock 维持不入库（库惯例，复现性代价已在部署文档登记）
- [x] 三级 API 分层 / MCP 慢启动维持 P3 登记（无新消费方证据不开工）
- [x] announcement/news 域维持未实现（无消费方证据）
- [x] data-privacy 客户端接线维持登记（随各客户端工程节消化）
- [x] 桌面通知点击唤起主窗：已落——XDG 分支 notify-rust 动作注册 + tray_action 映射
- [x] 平板断点维持待反馈（触发条件未到）
- [x] 护栏白名单/告警翻译层维持现状（GW-004 只标记不封禁，翻译层已随诊断知识库落地）
- [x] Jaeger OTLP span 导出、§10.1 对象存储适配、差分更新：维持登记不开工
- [x] 导出覆盖确认已落（7fad591）：框架层确认流程 + tauri 原生对话框，命令返回 Option<ExportOutcome>
- [x] 依赖升级债逐条归因：h2/sqlx/webpki/object_store/rsa 五条全为传递依赖，修复需框架大版本升级，维持报告型
- [x] 托盘图标随状态/主题换图：登记待资产（缺资产不自行生成代餐）
- [x] 定时轮询取数维持登记：确定性演示行情下无消费方，触发条件 = QuoteRemote 接真实后端
- [x] 记忆上次所用显示器已落（1c10638：WindowGeometry.monitor 按名查屏；最大化回原屏 f70ee74）
- [x] L117 前端消费面（快捷键/右键菜单）维持不迁移：桌面无行情展示面，触发条件 = 接真实行情落表
- [x] 外部输入核验：托盘资产、QuoteRemote 真实后端、真机多屏/移动端验收均未到位，维持不开工
- [x] 路线图对账：architecture-review §5 P2 翻「已落」、cross-platform-architecture §4/§7 翻正、desktop-framework §5 冻结注记（纯文档）

## 审核发现（89db63ca..HEAD 只审不改）
- [x] 审核发现：本轮 diff 未见问题（89db63ca..HEAD）。该区间仅 1 笔提交 82d09c1（docs：新增 2026-11-01-code-review.md 巡检报告 + 14 张 before/after 截图），纯文档无任何代码/配置改动；报告引用的 5 笔修复（c0ab9bb/c401e34/2d38125/b636a2d/89db63c）及其 3 项回归测试均核实存在，file:line 证据准确，截图均为有效 PNG。无安全/正确性/测试缺口可提。（核实：82d09c1；docs/review/2026-11-01-code-review.md:1-5；回归测试 c0ab9bb/services/collector/src/main_simple.rs:1450、c401e34/services/data-engine/src/main.rs:2754、b636a2d/packages/core/src/indicators.rs:261）

## 审核发现（82d09c1d..HEAD 只审不改）
- [x] 审核发现：本轮 diff 未见问题（82d09c1d..5e5d0cf）。该区间仅 1 笔提交 5e5d0cf（docs：todo.md 追加 3 行，即上一轮 89db63ca..HEAD 的审核结论落盘），纯文档无任何代码/配置改动。本轮复核：git log/diff --stat 确认区间内仅此一笔且只触及 todo.md；交叉验证了 5e5d0cf 结论中的事实主张——82d09c1 提交存在且内容为巡检报告+14 张 PNG、5 笔被引修复（c0ab9bb/c401e34/2d38125/b636a2d/89db63c）均在仓库历史中、3 项回归测试 file:line（c0ab9bb/services/collector/src/main_simple.rs:1450、c401e34/services/data-engine/src/main.rs:2754、b636a2d/packages/core/src/indicators.rs:261）逐条比对源码属实，无虚假记载。无安全/正确性/测试缺口可提。（核实：5e5d0cf；todo.md:194-195）

## 审核发现（5e5d0cfb..HEAD 只审不改）
- [x] 审核发现：本轮 diff 未见问题（5e5d0cfb..HEAD）。该区间仅 1 笔提交 1d526c8（docs：todo.md 追加 3 行，即上一轮 82d09c1d..HEAD 的审核结论落盘），纯文档无任何代码/配置改动。本轮复核：git log/diff --name-only 确认区间内仅此一笔且只触及 todo.md；交叉验证了 1d526c8 结论中的事实主张——82d09c1 提交存在且内容为巡检报告 + 14 张 PNG（7 issue × before/after）、5 笔被引修复（c0ab9bb/c401e34/2d38125/b636a2d/89db63c）均在仓库历史中、3 项回归测试 file:line（c0ab9bb/services/collector/src/main_simple.rs:1450、c401e34/services/data-engine/src/main.rs:2754、b636a2d/packages/core/src/indicators.rs:261）逐条比对源码属实，无虚假记载。无安全/正确性/测试缺口可提。（核实：1d526c8；todo.md:194-195；复核：todo.md:201）

## 审核发现（1d526c80..dc50e56 只审不改）
- [x] 审核发现：本轮 diff 未见问题（1d526c80..dc50e56）。该区间仅 1 笔提交 dc50e56（docs：todo.md 追加 3 行，即上一轮 5e5d0cfb..HEAD 的审核结论落盘），纯文档无任何代码/配置改动。本轮复核：git log/diff --stat 确认区间内仅此一笔且只触及 todo.md；交叉验证了 dc50e56 结论中的事实主张——1d526c8 提交存在且为 docs（todo.md 追加 3 行）、5e5d0cfb..1d526c8 区间仅此一笔、82d09c1 提交存在且内容为巡检报告 + 14 张 PNG（7 issue × before/after）、5 笔被引修复（c0ab9bb/c401e34/2d38125/b636a2d/89db63c）均在仓库历史中、3 项回归测试 file:line（c0ab9bb/services/collector/src/main_simple.rs:1450、c401e34/services/data-engine/src/main.rs:2754、b636a2d/packages/core/src/indicators.rs:261）逐条比对源码属实，无虚假记载。无安全/正确性/测试缺口可提。（核实：dc50e56；todo.md:199-201）

## 审核发现（dc50e564..076b9f3f 只审不改）
- [x] 审核发现：076b9f3 新增结论把区间标注为「1d526c80..HEAD」并称「该区间仅 1 笔提交 dc50e56」，但当前 HEAD=076b9f3 下 `git rev-list --count 1d526c80..HEAD` 实为 2 笔（dc50e56+076b9f3 自身），该主张仅对父区间 1d526c80..dc50e56 成立，标签与结论自相矛盾，属审核台账事实性不一致。已核实属实并将上方结论区间修正为「1d526c80..dc50e56」、事实主张保留（dc50e56 确实为 5e5d0cfb..HEAD 审核结论落盘）。（修正：f476122；todo.md:207）
- [x] 审核发现：本轮 diff 未见问题（dc50e564..076b9f3f）。该区间仅 1 笔提交 076b9f3（docs：todo.md 追加 3 行，即上一轮 1d526c80..HEAD 的审核结论落盘，含 1 项后续被本行发现的事实性不一致），纯文档无任何代码/配置改动。本轮复核：git log/diff --stat 确认区间内仅此一笔且只触及 todo.md；交叉验证了 076b9f3 结论中的事实主张——dc50e56 提交存在且为 docs（todo.md 追加 3 行）、1d526c8 提交存在且为 docs、5 笔被引修复（c0ab9bb/c401e34/2d38125/b636a2d/89db63c）均在仓库历史中、3 项回归测试 file:line（c0ab9bb/services/collector/src/main_simple.rs:1450、c401e34/services/data-engine/src/main.rs:2754、b636a2d/packages/core/src/indicators.rs:261）逐条比对源码属实，无虚假记载；唯一发现（区间计数自相矛盾）已由本行上方修正。无安全/正确性/测试缺口可提。（核实：076b9f3；todo.md:203-208）
