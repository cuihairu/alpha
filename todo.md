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

## 漏洞扫描遗留（2026-11-03 全舰队扫描，源 scratch/vuln-scan-*.md）

- [x] VULN-ALPHA-001 [中危·RUSTSEC-2024-0363] sqlx 0.7.4→0.9.0【跨大版本需改代码】：一次消除 sqlx 自身 + rustls 0.21.12（0346/0349）+ rustls-webpki 0.101.7（0098/0099/0104/0382）+ webpki-roots 0.25.4（0381）共 8 个 RUSTSEC ID；验收：cargo audit 中上述 8 个 ID 清零 + cargo test --workspace --exclude alpha-desktop 绿。（证据：`Cargo.toml:64` 声明 sqlx 0.9.0（经 d126a69 由 0.8.1 续升），实际解析 sqlx 0.9.0 / rustls 0.23.46 / rustls-webpki 0.103.16 / webpki-roots 0.26.11；`cargo audit` 输出中 RUSTSEC-2024-0363、2026-0346、2026-0349、2026-0098、2026-0099、2026-0104、2026-0382、2026-0381 均不存在；`cargo test --workspace --exclude alpha-desktop` = 627 passed、0 failed、6 ignored）
- [x] VULN-ALPHA-002 [低危·RUSTSEC-2026-0258] h2 0.3.27→0.4【跨大版本需改代码】：hyper 0.14/axum 0.6/tonic 0.10 生态锁定，需 tonic 0.10→≥0.11（连带 axum 0.6→0.7、hyper 0.14→1.x）；验收：cargo audit 该 ID 清零 + cargo test --workspace --exclude alpha-desktop 绿。（证据：`4f0ebc7`；`Cargo.lock` 中 h2=0.4.20、hyper=1.12.0，且 0.3.27/0.14.32 消失；`cargo audit` 输出 `RUSTSEC-2026-0258` 不存在；`cargo test --workspace --exclude alpha-desktop` 全绿，其中 api-gateway=64 passed、0 failed，全工作区 0 failed）
- [x] VULN-ALPHA-003 [高危 7.5·RUSTSEC-2026-0435] thrift 0.17.0→0.23【跨大版本需改代码】：由 parquet 50/datafusion 35 引入，需升 arrow 生态；验收：cargo audit 该 ID 清零 + cargo test --workspace --exclude alpha-desktop 绿。（证据：`1490dfb`；`Cargo.lock` 中 datafusion=55.2.0、arrow=59.3.2、parquet=59.3.2，thrift 完全消失；`cargo audit` 输出 `RUSTSEC-2026-0435` 不存在；`cargo test --workspace --exclude alpha-desktop` 全绿，0 failed、6 ignored，其中 data-engine=52+1 passed）
- [x] VULN-ALPHA-004 [低危 3.8·RUSTSEC-2024-0358] object_store 0.9.1→0.10【跨大版本需改代码】：datafusion 35 引入；验收：cargo audit 该 ID 清零 + cargo test --workspace --exclude alpha-desktop 绿。（证据：`1490dfb` 连带；`Cargo.lock` 中 object_store=0.13.2（datafusion 55.2 传递引入，仓库无直接 object_store 依赖）；`cargo audit` 输出 `RUSTSEC-2024-0358` 不存在；`cargo test --workspace --exclude alpha-desktop` 全绿，0 failed、6 ignored）
- [x] VULN-ALPHA-005 [中危·RUSTSEC-2026-0387] crypto-bigint 0.5.5→0.7.6+【跨大版本需改代码】：由 elliptic-curve 0.13/p256 或 rsa 0.9 锁定，走 p256/jsonwebtoken 升级路径；验收：cargo audit 该 ID 清零 + cargo test --workspace --exclude alpha-desktop 绿。（证据：`27a7916` 改为项目内纯 HMAC-SHA256 HS256/oct JWT 实现，`Cargo.lock` 中 jsonwebtoken、crypto-bigint、p256、p384、elliptic-curve、ecdsa 均不存在；`cargo audit` 输出 `RUSTSEC-2026-0387` 不存在；`cargo test --workspace --exclude alpha-desktop` 全绿，0 failed、6 ignored，其中 api-gateway=64 passed）
- [x] VULN-ALPHA-006 [低危 5.9·RUSTSEC-2023-0071] rsa 0.9.10（经 sqlx-mysql/jsonwebtoken 引入）：已随 `27a7916` 移除 jsonwebtoken、`d126a69` sqlx 0.9 适配而失效；验收：cargo audit 该 ID 清零 + cargo test --workspace --exclude alpha-desktop 绿。（证据：`Cargo.lock` 中 `name = "rsa"` 为 0 处，sqlx-mysql 0.9.0 依赖列表不含 rsa；`cargo audit` 输出无 `RUSTSEC-2023-0071`；`cargo test --workspace --exclude alpha-desktop` = 0 failed、6 ignored）

## 审核发现（dc50e564..HEAD 只审不改）
- [x] 审核发现：本轮 diff 未见问题（dc50e564..HEAD）。该区间共 4 笔提交 076b9f3/f476122/8d38eeb/b307fb0，全部为 docs（todo.md 追加/修订审核结论与漏洞扫描遗留记录），纯文档无任何代码/配置改动（git diff --name-only 仅 todo.md，16+/0-）。本轮复核：①区间计数与提交清单核对属实（git rev-list --count dc50e564..HEAD = 4，提交信息均为 docs:）；②交叉验证 076b9f3/f476122 结论中的事实主张——dc50e56/1d526c8/82d09c1 提交存在且为 docs、5 笔被引修复（c0ab9bb/c401e34/2d38125/b636a2d/89db63c）均在仓库历史中、3 项回归测试 file:line（c0ab9bb/services/collector/src/main_simple.rs:1450、c401e34/services/data-engine/src/main.rs:2754、b636a2d/packages/core/src/indicators.rs:261）逐条比对源码属实，无虚假记载；③f476122 确实实施了区间标签修正（1d526c80..HEAD → 1d526c80..dc50e56），非空头主张；④b307fb0 的 6 条 VULN-ALPHA 记录与扫描源 scratch/vuln-scan-*.md（存在于 ~/.hermes/cache/scratch/）逐条比对：8 个被引 RUSTSEC ID（0363/0346/0349/0098/0099/0104/0382/0381）均出现在源表，且各 crate 版本（sqlx 0.7.4、h2 0.3.27、thrift 0.17.0、object_store 0.9.1、crypto-bigint 0.5.5、rsa 0.9.10、rustls 0.21.12、rustls-webpki 0.101.7、webpki-roots 0.25.4）与 Cargo.lock 逐项一致，严重度/升级路径描述与源一致，无夸大或虚假记载。无安全/正确性/测试缺口可提（均为纯文档台账，未引入任何代码风险）。（核实：dc50e564..HEAD 全区间；todo.md:206-217）

## 审核发现（b307fb0555b91161ee2d3f26d0159f7fe91c2538..72432136e795a723a050c2ec3dbc70c0beadac3b 只审不改）
- [x] 审核发现：jwt.rs 的 HS256 签名比对用 `expected != sig`（Vec<u8> 逐字节短路比较），非恒定时间比较，存在经响应时序泄漏签名前缀匹配长度的侧信道风险；原 jsonwebtoken 走恒定时间校验，此为本实现的安全退化。已核实属实并修复：改用 `subtle::ConstantTimeEq`（`expected.ct_eq(&sig)`），逐字节短路比较消除（services/api-gateway/src/jwt.rs:162；修复提交见 git log；api-gateway 测试 70 passed 0 failed，含新增 jwt::tests::constant_time_compare_rejects_different）
- [x] 审核发现：jwt.rs 的 aud 校验仅接受字符串形式（`as_str()`），RFC 7519 §4.1.3 允许 `aud` 为字符串数组，IdP 以数组形式签发时会被判无效而 401，相对原 jsonwebtoken（支持数组并做成员匹配）属兼容性/互操作回归。已核实属实并修复：decode() 现接受字符串或字符串数组并做任一成员匹配（services/api-gateway/src/jwt.rs:188；新增测试 jwt::tests::aud_array_accepted）
- [x] 审核发现：jwt.rs 的 `Validation.required_spec_claims` 字段被声明且 auth.rs:122 向其中 push("exp")，但 decode() 从不读取该校验，属死配置；若未来以 validate_exp=false 搭配 required_spec_claims 使用将静默不强制所声明项，语义与 jsonwebtoken 不一致。已核实属实并修复：decode() 在 exp 校验前遍历 required_spec_claims 强制存在（services/api-gateway/src/jwt.rs:183；新增测试 jwt::tests::missing_exp_rejected_by_required_spec_claims）
- [x] 审核发现：新增 194 行纯 HMAC-SHA256 JWT 模块 jwt.rs 无任何 #[cfg(test)] 直接单测，算法混淆（alg=none/非 HS256）、多余分段、空签名、aud 数组、缺 exp 等边界均无覆盖；且现有测试均为同实现 encode/decode 往返，对称性缺陷无法暴露。已核实属实并修复：jwt.rs 内置 mod tests 共 6 项（alg=none 拒绝、多余分段拒绝、空签名拒绝、aud 数组接受、缺 exp 拒绝、恒定时间比对拒绝），api-gateway 测试 64→70（services/api-gateway/src/jwt.rs:1；cargo test -p alpha-api-gateway = 70 passed、0 failed）
- [x] 审核发现：sqlx 0.9 改用 `AssertSqlSafe` 包裹 format! 插值标识符（{table}）的 DDL/DML，显式关闭 sqlx 静态 SQL 安全网；当前调用方均传常量（"alpha_kv"/"alpha_accounts"）无实际注入，但 connect() 接口接受任意 &str，一旦 table 改为配置/输入来源即成为注入点。已核实属实并修复：connect() 入口新增表名校验（仅小写字母/数字/下划线，首字符非数字），非可信标识符直接报错，关闭注入面（packages/storage/src/postgres_kv.rs:28；cargo test -p alpha-storage = 76 passed、0 failed）
- [x] 审核发现：todo.md VULN-ALPHA-001 证据声明「Cargo.toml:64 声明 sqlx 0.8.1，实际解析 sqlx 0.8.6」，但同区间内 d126a69 已升 sqlx 0.9.0（当前 Cargo.toml:64 = sqlx 0.9.0、Cargo.lock = 0.9.0），台账证据与最终树状态不符，标题「0.7.4→0.8.1」亦非最终版本。已核实属实并修订：证据改为「sqlx 0.7.4→0.9.0，Cargo.toml:64 声明 sqlx 0.9.0（经 d126a69 由 0.8.1 续升），实际解析 sqlx 0.9.0」，与最终树状态一致（todo.md:212）

## 审核发现（72432136e795a723a050c2ec3dbc70c0beadac3b..8adc1c9330c930b925dce53cc443df80c9ac74ed 只审不改）
- [x] 审核发现：新增测试 missing_exp_rejected_by_required_spec_claims 未隔离本轮新加的 required_spec_claims 强制逻辑——测试助手 val() 同时置 validate_exp=true，缺 exp 的 token 会被 validate_exp 分支（jwt.rs:178，`value.get("exp").and_then(as_i64).ok_or(())`）先拒绝；即便删除新加的 required_spec_claims 循环（jwt.rs:172-176）该测试仍通过，无法防该逻辑回归，也未覆盖提交注释宣称的「validate_exp=false 时仍强制 exp」正交路径（services/api-gateway/src/jwt.rs:281；val() 见 jwt.rs:234-239）。已核实属实并修复：val() 默认 validate_exp=false（jwt.rs:236），测试显式断言 `!val().validate_exp` 并同时验证缺 exp 拒绝/含 exp 接受（jwt.rs:281-287）；变异验证——删除 required_spec_claims 循环后该测试失败（assertion failed: dec(&t).is_err()），确认已真正守护该逻辑（cargo test -p alpha-api-gateway = 70 passed、0 failed；变异见上）
- [x] 审核发现：新增测试 constant_time_compare_rejects_different 仅断言篡改签名为拒，用旧的非恒定时间 `expected != sig`（逐字节短路比较）同样通过，故并未真正守护/证成「恒定时间」属性，仅作证据引用（本轮修复说明及 todo 前节以之佐证侧信道修复）说服力不足（services/api-gateway/src/jwt.rs:287；ct_eq 调用见 jwt.rs:164）。已核实属实并修复：测试改名 signature_validation_accepts_valid_and_rejects_tampered 并停止过度宣称——单元测试无法证成时序恒定，恒定时间保证由 `subtle::ConstantTimeEq`（jwt.rs:164）提供；测试改为语义覆盖：未篡改签名接受 + 真实篡改解码后签名首字节（旧法改末字符可能解码出相同字节，非确定）拒绝（jwt.rs:290-298）；变异验证——比较恒拒/恒接受均使测试失败，确认正反例都有效（cargo test -p alpha-api-gateway = 70 passed、0 failed；变异见上）

## 审核发现（8adc1c9330c930b925dce53cc443df80c9ac74ed..d069e86 只审不改）
- [x] 审核发现：本轮 diff 未见问题（8adc1c9330c930b925dce53cc443df80c9ac74ed..d069e86）。区间仅 1 笔提交 d069e86（test(jwt)：isolate required_spec_claims and harden signature tests），只改 services/api-gateway/src/jwt.rs 的 #[cfg(test)] 测试代码（10+5，无生产代码/依赖/配置改动）并追加 todo.md 审核台账；已交叉验证：cargo test -p alpha-api-gateway 70 passed、0 failed（jwt 模块 6 项全过），git diff --check 无空白错误，工作区无未提交改动。审查要点均无问题：val() 置 validate_exp=false 且显式断言，missing_exp_rejected_by_required_spec_claims 同时覆盖缺 exp 拒绝与含 exp 接受，真正隔离 required_spec_claims 逻辑；测试改名 signature_validation_accepts_valid_and_rejects_tampered 后不再过度宣称恒定时间（恒定时间由 subtle::ConstantTimeEq jwt.rs:164 提供），改为解码后篡改签名首字节（确定性）拒绝 + 未篡改接受，正反例有效；无生产逻辑回归、无死代码、无未覆盖新分支、无台账事实性/引用不一致、无越权修改（仅测试文件与台账）。（核实：d069e86；jwt.rs:234-298；todo.md:230-232）
- [x] 审核发现：上一轮结论称 8adc1c93..HEAD 仅含 d069e86 一笔提交，但当前该区间还包含结论自身的提交 9d2e116，区间计数与记录不一致。已核实属实并将上方结论区间修正为「8adc1c93..d069e86」、事实主张保留（git rev-list --count 8adc1c9330c930b925dce53cc443df80c9ac74ed..d069e86 = 1，仅 d069e86；d069e86 父提交为 8adc1c9）。（修正：本行；todo.md:234-235）

## 审核发现（9d2e11653b25e91382b495dd49391c928fd25e3e..6da182fdfe828a63823f170a198e550b05acbd87 只审不改）
- [x] 审核发现：本轮 diff 未见问题（9d2e1165..6da182f）。区间仅 1 笔提交 6da182f（docs: fix audit range record），只改 todo.md 审核台账（3+2 行：区间标题与结论行修正为 8adc1c93..d069e86，追加区间计数勘误行），无生产代码/依赖/配置改动；已交叉验证：git rev-list --count 8adc1c93..d069e86 = 1、d069e86 父提交 = 8adc1c9（与修正后记录一致），git diff --check 无空白错误，工作区除本台账外无未提交改动，无越权修改；本行后续勘误：原标签 9d2e1165..HEAD 在 caec9bd 后会错误包含审核结论自身，已固定为 9d2e1165..6da182f（git rev-list --count 9d2e1165..6da182f = 1，仅 6da182f）。（核实：6da182f；git rev-list/diff --check；todo.md:234-236）

## 审核发现（6da182fdfe828a63823f170a198e550b05acbd87..caec9bdd48c48215e64e717ab77273a0bf15f0af 只审不改）
- [x] 审核发现：caec9bd 新增结论行把审计区间标为「9d2e1165..HEAD」，但该行本身即结论提交 caec9bd，提交后 HEAD=caec9bd，「9d2e1165..HEAD」实为 9d2e116..caec9bd 共 2 笔（含结论自身），与其自身主张「区间仅 1 笔提交 6da182f」矛盾（git rev-list --count 9d2e116..HEAD = 2），与上一轮 6da182f 已标记并修正的「..HEAD 含结论自身致计数不一致」缺陷（todo.md:236，修正为 8adc1c93..d069e86）同类复发，按既有惯例应固定上界为「9d2e1165..6da182f」（工作区未提交勘误行已如此标注）。已核实属实并修复：上界已固定为 9d2e1165..6da182f（本行上方结论行已改；git rev-list --count 9d2e116..6da182f = 1，仅 6da182f）。（修正：本行；todo.md:237）
- [x] 审核发现：结论行称被审提交 6da182f「只改 todo.md 审核台账（2+2 行）」，但 git show 6da182f --stat 实为 3 插入+2 删除（2 处修改行各 1 删 1 增 + 1 行新增区间计数勘误行），「2+2」少计 1 行新增，应为「3+2」。已核实属实并修复：结论行「2+2 行」已改为「3+2 行」（git show 6da182f --numstat = 3 插入 2 删除，仅 todo.md）。（修正：本行；todo.md:237）
- [x] 审核发现：caec9bd 将「本轮 diff 未见问题（9d2e1165..6da182f）」结论行追加进「审核发现（8adc1c9330c930b925dce53cc443df80c9ac74ed..d069e86 只审不改）」区块（todo.md:234），而非新建 9d2e116..6da182f 对应区间区块；4b15461 仅将行内区间标签修正为 9d2e1165..6da182f（todo.md:237）、未修正区块归属，致该行所在区块标题（8adc1c93..d069e86）与其实际审核区间（9d2e116..6da182f）不一致，台账结构错位（todo.md:237）。已核实属实并修复：该结论行已移出 8adc1c93..d069e86 区块，新建独立「审核发现（9d2e116..6da182f 只审不改）」区块承载（git show caec9bd -- todo.md 确认该行在 @@ -234 hunk 内追加进 8adc1c93..d069e86 区块；git show 4b15461 -- todo.md 确认仅改行内标签未移动行；git rev-list --count 9d2e116..6da182f = 1）。（修正：todo.md 审核区块结构；git show caec9bd/4b15461；todo.md 本行上方新区块）

## 审核发现（4b15461d185c95463a0a8358c59c5c7f3707d8aa..e03a108b7ee9c008d7aa590ff2a69cb1740d523e 只审不改）
- [x] 审核发现：e03a108 提交信息「docs: relocate 9d2e116..6da182f audit block」仅描述区块搬迁，但该提交（git show e03a108 --numstat = 3 插入 0 删除，仅 todo.md）除新建 9d2e116..6da182f 区块标题行（todo.md:238）外还向 6da182f..caec9bd 区块追加一条新审核发现（todo.md:244），提交内容超出提交信息所述范围，属提交内容与信息描述不一致。已核实属实；因 e03a108 已推送至 origin/main（git branch -r --contains e03a108）且禁改写历史/force push，无法修改该提交信息，故在台账记录并接受此不一致（git show e03a108 --stat = 3 插入 0 删除仅 todo.md，含新区块标题行与新审核发现行；git show e03a108 确认新区块标题与 line 244 追加同处一并落地）。（修正：本行记录；todo.md:246-247）
