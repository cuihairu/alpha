# TODO

> 交付台账：`[x]` 为已完成项，行号随编辑漂移不作引用依据；现状以 README.md、docs/ 与代码为准。

## 优先级行动清单（2026-09-27 现状分析产出，详见 docs/analysis.md）

### P0 打通并守住端到端管线（2026-09-27）
- [x] 修复 normalized 行情误入 DLQ（real-time-feed 字段容错 + data-engine 保留原始字段）
- [x] 新增 Redis Streams 管线集成测试：raw→normalized→realtime 全链路断言（REDIS_TEST_URL 门控）
- [x] normalize_quote 抽纯函数并补单测
- [x] 修 read_latest 的 XREVRANGE 解析崩溃
- [x] 本机真实 Redis 进程级 E2E 验证：raw→normalized→API history，DLQ 为空

### P1 修部署口径（2026-09-27）
- [x] collector 端口统一 8083（ALPHA_COLLECTOR_BIND 可配），对齐 compose/Dockerfile/dev-start
- [x] compose 补 REDIS_URL 与 ALPHA__CLICKHOUSE__*，加 depends_on redis
- [x] 修 4 个服务 Dockerfile workspace 拷贝不完整（COPY . + .dockerignore）
- [x] 解码失败消息隔离进 quotes.dlq（GroupRead 分离 invalid，publish_dlq + ack，DLQ 发布失败不 ack）
- [x] 孤儿 pending 消息清理：claim_stale 认领重放，data-engine/real-time-feed 各起独立 sweeper

### P2 数据落地
- [x] data-engine 按 persistence_enabled/timescale_url 装配 Timescale 落库，三态降级（2026-09-27）
- [x] normalized 层去重（payload_hash 窗口判重，跨重启由 Timescale UPSERT 兜底）（2026-09-28）
- [x] 移除 MemTable 全量重建热点（/query 执行前自刷新），修 register_table 撞名（2026-09-28）
- [x] 内存时序重写：二分有序插入 + UPSERT，批量灌入 48 万点降至 6.4s（2026-09-28）
- [x] 重投递封顶：delivery_count 默认 5，超限毒消息转 quotes.dlq 停投留痕（2026-09-28）
- [x] real-time-feed 显式日志级别初始化（默认 info，ALPHA_LOG_LEVEL 可调）（2026-09-28）
- [x] 持久化镜像失败重试：有界 FIFO 缓冲 + 后台补写（非完整 outbox，边界已在代码注明）（2026-09-28）
- [x] compose 透传持久化 env 为 ${VAR:-默认} 形式，宿主机可覆写（2026-09-28）

### P3 补对外链路
- [x] api-gateway 真实反代：/api/v1/* 透传、/ws 双向泵、/health 三上游聚合探测（2026-09-28）
- [x] web 前端接 real-time-feed WS + data-engine REST（真实优先、不可达回退演示）（2026-09-28）
- [x] 清理 collector 未挂载死模块与根目录孤儿文件（2026-09-28）

## 跨平台 Rust 架构设计
- [x] 设计统一跨平台架构：docs/cross-platform-architecture.md 三层模型 + check-cross-platform.sh 门禁（2026-09-28）
- [x] Cargo workspace 多目标构建：cargo wasm-check/wasm-build alias，门禁扩为四步（2026-09-28）
- [x] 跨平台共享核心库：core wasm-clean、protocols grpc feature 门控、storage 服务端专属（2026-09-28）
- [x] 平台适配层抽象：KeyValueStore/LocalPersistence/UserNotification trait + InMemory 实现与契约测试（2026-09-28）
- [x] 统一 Rust 代码规范：docs/rust-code-standards.md + check-lint.sh 门禁 + CI 三处红灯修复（2026-09-28）

## Rust WASM Web 分析引擎
- [x] 集成 wasm-bindgen 和 wasm-pack 构建工具链（2026-09-28）
- [x] WASM 核心计算库：指标算法绑定 + backtestSmaCross 回测绑定（2026-09-28）
- [x] 零拷贝内存管理与 Arrow 优化：shared_buffer + ArrowBatch + backtestSmaCrossPtr，A/B 基准实测（2026-09-28）
- [x] 混合存储架构：HybridCacheService 三层 read-through + wasm HybridStorage + IndexedDB 缝（2026-09-29）
- [x] 流式数据处理与并行计算：ParallelStreamProcessor 双路径（Rayon/WorkerPool）（2026-09-29）
- [x] 实时数据同步协议：SyncEngine Full/Delta/Gap/Resync + 服务端 seq 分配 + 前端合入（docs/realtime-sync-protocol.md）（2026-09-29）

## 桌面端应用（Tauri）
- [x] 搭建 Tauri 桌面框架：框架层/接线层拆分 + check-desktop.sh 五步门禁 + 配置/接线契约测试 + fake pkg-config Linux 门禁（docs/desktop-framework.md）（2026-09-30）
- [x] 原生文件系统集成与本地导出：export_symbol_request 严格校验 + 原子落盘 + dialog.save 壳（2026-09-30）
- [x] 系统通知和托盘集成：NotificationQueue + 托盘菜单状态机 + 告警检查链（platform.rs 机械翻译纪律）（2026-09-30）
- [x] 跨平台窗口管理和主题适配：window.rs 放置决策/主题映射 + window_gui 接线（2026-09-30）
- [x] 本地数据库同步和离线模式：offline.rs 降级矩阵/增量同步 + QuoteRemote 缝（2026-09-30）
- [x] 键盘快捷键和右键菜单：DEFAULT_SHORTCUTS + context_menu 模型 + shortcut_gui 接线（2026-09-30）

## Rust 微服务架构
- [x] 基于 Axum + Tokio 构建高性能 HTTP/gRPC 服务
- [x] 使用 Tonic + Prost 替代 go-zero 实现微服务通信
- [x] 集成 DataFusion 替代 DuckDB 服务端实现内存 SQL 引擎
- [x] 开发基于 Arrow 的列式数据处理管线
- [x] 实现基于 Tokio 的异步消息队列和事件驱动架构
- [x] 构建统一配置管理（config-rs）和分布式追踪（tracing）

## 移动端应用（Android & iOS）
- [x] 移动端 Rust 核心库架构（JNI + UniFFI）：docs/mobile-core-architecture.md + alpha-mobile 骨架 16 单测（2026-09-30）
- [x] Android Kotlin + Rust 混合环境（Jetpack Compose）：Gradle 工程 + uniffi 绑定 + NDK 交叉编译本机实证（2026-09-30）
- [x] iOS Swift + Rust 集成（SwiftUI + UniFFI）
- [x] 移动端推送通知和后台同步：Notifier/SyncTrigger/BackgroundSync + FFI 六方法 + PushSyncSeam（docs/mobile-push-sync.md）（2026-10-01）
- [x] 触屏手势和移动端 UI 交互：下拉刷新/长按分析/双击折叠，核心库零改动（docs/mobile-gestures.md）（2026-10-01）
- [x] 移动端离线数据存储和同步：授权红线三条款 + 备份/同步术语分离（docs/mobile-offline.md）（2026-10-01）

## 跨平台 UI 框架开发
- [x] 跨平台 UI 框架选型与集成：React 18 + TypeScript + Vite（docs/web-framework-selection.md），web/app 骨架落地（2026-10-01）
- [x] React 组件化数据分析界面：QuoteTable/WasmProbe/IndicatorPanel + EMA 指标（vitest 12）（2026-10-02）
- [x] 集成高性能图表库：lightweight-charts 5.2 PriceChart（日 K + SMA 叠加，确定性数据）（vitest 16）（2026-10-02）
- [x] 跨平台 SQL 查询编辑器：SqlWorkbench + DuckDB-WASM + ResultGrid（vitest 22）（2026-10-02）
- [x] 响应式设计：三断点单列布局 + 触控目标 ≥40px（视觉验收归浏览器真机）（2026-10-02）
- [x] 统一主题系统和个性化配置：双主题令牌 + ThemeToggle 三态偏好持久化（vitest 26）（2026-10-02）

## 数据采集服务
- [x] 基于 Tokio + Reqwest 开发高性能异步爬虫引擎
- [x] 实现智能任务调度器和限流代理池管理
- [x] 构建反爬虫对策（User-Agent 轮换、请求频率控制）
- [x] 开发多数据源适配器（API、网页、FTP、文件推送）
- [x] 实现数据质量校验、清洗和标准化流程
- [x] 构建采集状态监控和自动故障恢复机制
- [x] 数据源健康面：SourceHealthTracker 三态 + GET /sources/health + gauge 与两档告警（collector 85 测）（2026-10-07）

## 存储与数据处理
- [x] 集成 SQLx + TimescaleDB 实现时序数据存储
- [x] 使用 DataFusion + Arrow 构建内存分析引擎
- [x] Parquet 数据湖架构：docs/data-lake-parquet.md 设计 + LakeWriter 落地（分区写入/读回/compaction/manifest/定时维护，2026-10-09 收口）（2026-10-02）
- [x] Redis 分布式缓存和限流：DistributedCache cache-aside + RedisRateLimiter Lua 固定窗口 + gateway 接线（storage 34 测/gateway 9 测）（2026-10-02）
- [x] 智能数据分区策略：partition.rs 时间/交易所/桶三维映射 + compaction 装箱计划器（2026-10-02）
- [x] 数据压缩和列式存储优化：columnar.rs Delta/RLE/Dictionary 编码器 + 决策矩阵（2026-10-02）

## 性能优化工程
- [x] 零拷贝数据处理和内存池：BufferPool 双阈值回收 + QuoteFrame 零堆解码（2026-10-02）
- [x] SIMD 向量化计算：AVX2 4-lane 与 portable 参考实现位级一致（2026-10-02）
- [x] 多核并行计算引擎（Rayon + Web Workers）：parallel.rs 双路等价 + worker.rs 线程池（2026-10-02）
- [x] 智能预取和后台数据同步：BackgroundPrefetcher 等差检测 + 在飞去重（2026-10-02）
- [x] LLVM Profile 编译优化：build-pgo.sh 四阶段 + docs/pgo-build-optimization.md（gateway 实测跑通）（2026-10-02）
- [x] 内存泄漏检测和性能分析：TrackingAllocator + profile.sh + docs/memory-profiling.md（2026-10-02）

## 监控与可观测性
- [x] Prometheus + Grafana + Loki 全链路监控：四服务 /metrics + compose Loki/Promtail + provisioning 仪表盘（2026-10-02）
- [x] 分布式追踪：gateway X-Trace-Id 贯穿 + docs/distributed-tracing.md（Jaeger OTLP 全量导出登记不实现）（2026-10-02）
- [x] 实现基于 Metrics 的性能指标收集
- [x] 构建健康检查和服务依赖监控
- [x] Rust 内存安全监控：safety_audit.rs 清单扫描 + 编译期 deny 双舱位豁免（2026-10-02）
- [x] 实时告警和智能故障诊断：11 条告警 + Alertmanager + alert-webhook + diagnosis 引擎 + alpha-diagnose CLI（docs/alerting-and-diagnosis.md）（2026-10-02）

## 跨平台 CI/CD 与发布
- [x] GitHub Actions 多目标并行构建：五目标矩阵 fail-fast=false（2026-10-02）
- [x] Docker 镜像和容器化部署：五服务 Dockerfile 硬化 + build-images.sh + docs/docker-deployment.md（2026-10-02）
- [x] 自动化测试体系（单元/集成/端到端）：check-e2e.sh 四服务五段断言 + CI e2e 矩阵（2026-10-02）
- [x] 多平台发布流水线：release.yml 七作业（version-gate/web/desktop/android/ios/docker/updater-feed），只产 artifacts 不建 Release（2026-10-02）
- [x] 应用商店发布面：store/ 元数据 + docs/store-publishing.md（真机提审归发版时人工）（2026-10-02）
- [x] 自动化版本管理和热更新：bump-version.sh 一处输入五处同改 + check-version.sh 门禁 + docs/version-management.md（2026-10-02）

## 多平台用户体验与产品
- [x] 跨平台一致用户体验：docs/ux-consistency.md 对账表 + 平盘归属收敛（Android 39 测）（2026-10-02）
- [x] 统一用户账户系统和数据同步：account.rs 纯函数领域面（档案/三方合并/同步往返/状态机，19 单测）（2026-10-02）
- [x] 平台特色功能（桌面文件导出、移动推送通知）：exportCsv.ts + 既有推送面复用（vitest 64）（2026-10-02）
- [x] 跨平台帮助文档和视频教程：补齐文档站四页 + docs/user-guide.md（视频管线未立项）（2026-10-02）
- [x] 多语言国际化和本地化：web 中英双语 i18n.ts 字典基座（vitest 70）（2026-10-02）
- [x] 用户行为分析和产品优化：behavior.rs 纯函数 + web analytics.ts opt-in 采集缓冲（vitest 74）（2026-10-02）

## 安全与合规
- [x] JWT + OAuth 2.0 身份认证：gateway auth.rs HS256 签发/校验 + OIDC JWKS + WS 握手鉴权（docs/auth.md）（2026-10-02）
- [x] RBAC 细粒度权限控制：roles claims + authorize 纯函数 + 403 中间件（docs/auth.md §6）（2026-10-02）
- [x] 端到端数据加密：crypto.rs AES-256-GCM + EncryptedStorage 包装器 + 列级加密辅助（feature 门控）（2026-10-02）
- [x] 防爬虫/DDoS/API 限流保护：shield.rs 三件套（UA 分类/路径扫描/burst 令牌桶）+ RedisRateLimiter（gateway 27 测）（2026-10-02）
- [x] 安全审计日志和异常行为检测：audit.rs 审计通道 + AuthFailureDetector 风暴检测（只标记不封禁）（gateway 30 测）（2026-10-02）
- [x] 金融数据合规和隐私保护：privacy.ts 权利工具 + PrivacyPanel + docs/data-privacy.md 义务映射（vitest 60）（2026-10-02）

## 质量保证与测试
- [x] 单元/集成/端到端测试体系：docs/testing.md 七层统筹 + e2e WS 消息级断言（vitest 53）（2026-10-02）
- [x] 性能基准测试套件：criterion 三组 8 项 indicators_bench（2026-10-02）
- [x] Proptest 模糊测试和属性验证：9 性质 ×256 案例（2026-10-02）
- [x] 代码覆盖率报告：check-coverage.sh llvm-cov（行覆盖 62.4%，不设阈值）（2026-10-02）
- [x] 自动化性能回归检测：check-perf.sh criterion 基线比对 + CI perf.yml 周滚动基线（2026-10-02）
- [x] Miri 内存安全分析：check-miri.sh 5 种子 + CI miri.yml（2026-10-02）

## 业务功能开发
- [x] 实时行情展示和技术指标计算：liveFeed.ts + useLiveQuotes + LiveQuoteBoard（vitest 41）（2026-10-02）
- [x] 高级技术分析：patterns.rs K 线/线形形态 + Elliott 三规则校验引擎（core 124 测）（2026-10-02）
- [x] 量化回测和优化引擎：grid_search/walk_forward + 过拟合比（2026-10-02）
- [x] 风险管理和投资组合分析：risk.rs 九度量 + RiskReport 聚合（2026-10-02）
- [x] 智能告警和个性化推送：alerts.rs 规则引擎 + real-time-feed 评估接线（2026-10-02）
- [x] 市场数据 API 和第三方集成：X-Api-Key 门 + history.csv 导出 + docs/market-data-api.md（2026-10-02）

## 平台特色功能开发
- [x] Web PWA 和离线功能：manifest + sw.js 缓存策略（vitest 52）（2026-10-02）
- [x] 桌面多标签页和工作区管理：workspaces.ts 归约器 + WorkspaceTabs（vitest 49）（2026-10-02）
- [x] Android 小组件和快捷方式：HomeWidget.kt + 深链契约 + docs/android-widget.md（JVM 39 测）（2026-10-02）
- [x] iOS Live Activities 和动态岛：docs/ios-live-activities.md 设计契约（mobile/ios 目录不动）（2026-10-02 登记）
- [x] 全平台深色模式：web ThemeToggle + Android Theme.kt 三态映射 + docs/theme-adaptation.md（JVM 43 测）（2026-10-02）
- [x] 移动端生物识别认证和隐私保护：BiometricGate + EncryptedKeyValueStore + docs/mobile-privacy.md（JVM 40 测）（2026-10-02）

## 跨平台打包与分发
- [x] Web CDN 部署和静态资源优化：optimize-dist.mjs 指纹/预压缩（-84.8%）+ web-origin nginx + web-cdn-deploy.sh（docs/web-cdn.md）（2026-10-02）
- [x] 桌面多平台安装包：desktop-release.sh 三平台产物 + 修 tauri conf 两处真 bug（docs/desktop-release.md）（2026-10-02）
- [x] Android APK/AAB 分包和多渠道：双 ABI splits + play/direct flavor + aab 本机实测（docs/android-release.md）（2026-10-02）
- [x] iOS IPA 签名和 TestFlight：ios-release.sh 四阶段 + docs/ios-release.md（真机执行归 CI macOS 作业）（2026-10-02）
- [x] 自动更新机制：Tauri updater 登记 inert + release-update-feed.sh latest.json（docs/auto-update.md）（2026-10-02）
- [x] 平台合规性检查：permissions-registry.txt + check-compliance.sh 四面对账 + docs/platform-compliance.md（2026-10-02）
- [x] 消息投递/重送交付计数上限：已并入 P2「重投递封顶」条（2026-09-28）

## 拍板登记（2026-10-10 巡检，逐项按仓内决策记录拍板）
- [x] 桌面 frontendDist 指向 web/app/dist（web-framework §6 选项 A）：桥接 + 契约测试 + CI 双桌面作业全落（2026-10-09）
- [x] RustSec 升级债维持报告型不阻塞：收口归类可升级 0 / 误报 2（object_store、rsa）/ 记录风险 3（h2、webpki×3、sqlx），逐条证据见 docs/rust-code-standards.md §12（2026-10-10）
- [x] Cargo.lock 维持不入库（库惯例，复现性代价已在部署文档登记）（2026-10-10）
- [x] 三级 API 分层 / MCP 慢启动维持 P3 登记（无新消费方证据不开工）（2026-10-10）
- [x] announcement/news 域维持未实现（无消费方证据）（2026-10-10）
- [x] data-privacy 客户端接线维持登记（随各客户端工程节消化）（2026-10-10）
- [x] 桌面通知点击唤起主窗：已落（2026-10-10）——XDG 分支 notify-rust 动作注册 + tray_action 映射
- [x] 平板断点维持待反馈（触发条件未到）（2026-10-10）
- [x] 护栏白名单/告警翻译层维持现状（GW-004 只标记不封禁，翻译层已随诊断知识库落地）（2026-10-10）
- [x] Jaeger OTLP span 导出、§10.1 对象存储适配、差分更新：维持登记不开工（2026-10-10）
- [x] 导出覆盖确认已落（7fad591）：框架层确认流程 + tauri 原生对话框，命令返回 Option<ExportOutcome>
- [x] 依赖升级债逐条归因：h2/sqlx/webpki/object_store/rsa 五条全为传递依赖，修复需框架大版本升级，维持报告型（2026-10-10）
- [x] 托盘图标随状态/主题换图：登记待资产（缺资产不自行生成代餐）（2026-10-10）
- [x] 定时轮询取数维持登记：确定性演示行情下无消费方，触发条件 = QuoteRemote 接真实后端（2026-10-10）
- [x] 记忆上次所用显示器已落（1c10638：WindowGeometry.monitor 按名查屏；最大化回原屏 f70ee74）
- [x] L117 前端消费面（快捷键/右键菜单）维持不迁移：桌面无行情展示面，触发条件 = 接真实行情落表（2026-10-10）
- [x] 外部输入核验：托盘资产、QuoteRemote 真实后端、真机多屏/移动端验收均未到位，维持不开工（2026-10-10）
- [x] 路线图对账：architecture-review §5 P2 翻「已落」、cross-platform-architecture §4/§7 翻正、desktop-framework §5 冻结注记（纯文档）（2026-10-10）
