[0m
> build · gpt-6-luna
[0m→ [0mSkill "code-review" 
> build · gpt-6-luna
我会先盘点仓库结构、待办状态、架构/设计文档与测试入口，再沿主要模块核对实现；结论会区分已验证风险和文档信息不足之处。
[0m$ [0mrg --files | sed -n '1,260p'
Cargo.toml
start-web.sh
TODO.md
README_DEPLOYMENT.md
tools/build-tools/src/main.rs
tools/build-tools/Cargo.toml
tools/diagnose/src/main.rs
tools/diagnose/Cargo.toml
build-wasm-optimized.sh
patches/arrow-arith-50.0.0/src/bitwise.rs
patches/arrow-arith-50.0.0/src/temporal.rs
patches/arrow-arith-50.0.0/src/boolean.rs
patches/arrow-arith-50.0.0/src/lib.rs
patches/arrow-arith-50.0.0/src/arithmetic.rs
patches/arrow-arith-50.0.0/src/numeric.rs
patches/arrow-arith-50.0.0/src/arity.rs
patches/arrow-arith-50.0.0/src/aggregate.rs
patches/arrow-arith-50.0.0/README.md
patches/arrow-arith-50.0.0/Cargo.toml
LICENSE
PROJECT_SUMMARY.md
README.md
scripts/deploy.sh
scripts/check-version.sh
scripts/build-images.sh
scripts/check-desktop.sh
scripts/check-miri.sh
scripts/profile.sh
scripts/check-compliance.sh
scripts/update-dependencies.sh
scripts/web-cdn-deploy.sh
scripts/release-update-feed.sh
scripts/dev-start.sh
scripts/deploy-ubuntu.sh
scripts/check-perf.sh
scripts/check-web.sh
scripts/android-release.sh
scripts/clickhouse-init.sh
scripts/check-cross-platform.sh
scripts/ios-release.sh
scripts/check-coverage.sh
docker-compose.yml
Makefile
packages/core/tests/proptest_invariants.rs
packages/core/tests/proptest_invariants.proptest-regressions
docs/analysis.md
docs/auth.md
docs/mobile-push-sync.md
docs/android-release.md
docs/user-guide.md
docs/docusaurus.config.js
docs/patches/braces@3.0.3.patch
docs/rust-code-standards.md
docs/alerting-and-diagnosis.md
docs/deployment.md
docs/web-framework-selection.md
docs/mobile-gestures.md
docs/pnpm-workspace.yaml
docs/account-sync.md
docs/installation.md
docs/ux-consistency.md
docs/data-lake-parquet.md
docs/store-publishing.md
docs/ios-live-activities.md
docs/pnpm-lock.yaml
docs/auto-update.md
docs/memory-profiling.md
docs/mobile-offline.md
docs/mobile-privacy.md
docs/src/css/custom.css
packages/core/src/patterns.rs
packages/core/src/errors.rs
packages/core/src/account.rs
packages/core/src/diagnosis.rs
packages/core/src/models.rs
packages/core/src/backtest.rs
packages/core/src/alloc_tracking.rs
packages/core/src/alerts.rs
packages/core/src/streaming.rs
packages/core/src/utils.rs
packages/core/src/optimize.rs
packages/core/src/lib.rs
packages/core/src/safety_audit.rs
packages/core/src/parallel.rs
packages/core/src/behavior.rs
packages/core/src/crypto.rs
packages/core/src/memory.rs
packages/core/src/hybrid_cache.rs
packages/core/src/simd.rs
packages/core/src/platform.rs
scripts/desktop-fake-pc/gio-2.0.pc
scripts/desktop-fake-pc/atk.pc
scripts/desktop-fake-pc/glib-2.0.pc
scripts/desktop-fake-pc/libsoup-2.4.pc
scripts/desktop-fake-pc/cairo-gobject.pc
docs/src/pages/index.js
scripts/desktop-fake-pc/gtk+-3.0.pc
scripts/desktop-fake-pc/javascriptcoregtk-4.0.pc
scripts/desktop-fake-pc/x11.pc
scripts/desktop-fake-pc/pango.pc
scripts/desktop-fake-pc/gdk-pixbuf-2.0.pc
docs/deployment-runbook.md
scripts/desktop-fake-pc/gdk-3.0.pc
docs/platform-compliance.md
scripts/desktop-fake-pc/gdk-x11-3.0.pc
docs/审计-文档一致性.md
scripts/desktop-fake-pc/README.md
docs/cross-platform-architecture.md
scripts/desktop-fake-pc/cairo.pc
docs/android-widget.md
scripts/desktop-fake-pc/webkit2gtk-4.0.pc
docs/sidebars.js
scripts/desktop-fake-pc/gobject-2.0.pc
docs/desktop-framework.md
scripts/desktop-fake-pc/gmodule-2.0.pc
docs/realtime-sync-protocol.md
scripts/desktop-fake-pc/gtk+-x11-2.0.pc
docs/desktop-release.md
docs/ios-release.md
docs/README.md
docs/version-management.md
docs/architecture-review.md
scripts/bump-version.sh
docs/testing.md
docs/market-data-api.md
docs/theme-adaptation.md
docs/data-privacy.md
docs/intro.md
docs/package.json
docs/mobile-core-architecture.md
docs/pgo-build-optimization.md
docs/docker-deployment.md
docs/distributed-tracing.md
docs/web-cdn.md
docs/getting-started.md
docs/architecture.md
scripts/check-e2e.sh
scripts/desktop-release.sh
WASM_COMPLETION_SUMMARY.md
scripts/init-db.sql
packages/core/Cargo.toml
scripts/check-lint.sh
scripts/build-pgo.sh
scripts/store-release.sh
packages/core/src/indicators/advanced.rs
packages/core/src/analytics.rs
packages/core/src/indicators.rs
packages/core/src/risk.rs
packages/core/src/sync.rs
README.zh.md
wasm-analyzer/examples/streaming_bench.rs
wasm-analyzer/examples/zero_copy_bench.rs
wasm-analyzer/examples/parallel_bench.rs
web/style.css
services/collector/examples/simple_integration_demo.rs
services/collector/examples/crawler_integration_demo.rs
services/collector/examples/integration_summary.rs
services/collector/COMPLETION_REPORT.md
packages/core/benches/indicators_bench.rs
wasm-analyzer/tests/performance_tests.rs
wasm-analyzer/BUILD_REPORT.md
web/vendor/.gitkeep
web/wasm-demo.html
web/app.js
web/demo-data.js
wasm-analyzer/src/storage.rs
wasm-analyzer/src/worker.rs
wasm-analyzer/src/streaming.rs
wasm-analyzer/src/lib.rs
wasm-analyzer/src/shared_buffer.rs
wasm-analyzer/src/arrow_adapter.rs
wasm-analyzer/src/websocket.rs
packages/protocols/proto/data_engine.proto
wasm-analyzer/node-bench/zero-copy.mjs
wasm-analyzer/Cargo.toml
wasm-analyzer/IMPLEMENTATION_REPORT.md
packages/protocols/src/events.rs
packages/protocols/src/dataset.rs
packages/protocols/src/grpc.rs
packages/protocols/src/lib.rs
packages/protocols/src/instrument.rs
packages/protocols/src/websocket.rs
packages/protocols/src/rest.rs
packages/protocols/build.rs
packages/protocols/Cargo.toml
mobile/tests/android_shell_contract.rs
mobile/ios/gen-bindings.sh
packages/storage/examples/stream_inject.rs
packages/storage/tests/redis_streams.rs
web/index.html
web/server.js
web/README.md
config/alertmanager.yml
config/alpha-alerts.yml
config/collector.tasks.yaml
packages/storage/src/timescale.rs
packages/storage/src/redis_kv.rs
packages/storage/src/prefetch.rs
packages/storage/README.md
packages/storage/Cargo.toml
packages/storage/src/rate_limit.rs
packages/storage/src/lake.rs
packages/storage/src/disk_kv.rs
packages/storage/src/timeseries.rs
packages/storage/src/memory.rs
packages/storage/src/cloud.rs
packages/storage/src/redis_streams.rs
services/collector/src/source_health.rs
services/collector/src/main_simple.rs
services/collector/src/types.rs
services/collector/src/cleaner.rs
services/collector/COLLECTOR_README.md
services/collector/Dockerfile
services/collector/crawler_integration_config.json
services/collector/crawlers.json
services/collector/Cargo.toml
desktop/tauri.conf.json
config/compliance/permissions-registry.txt
config/clickhouse-users.xml
packages/storage/src/bin/clickhouse_test.rs
crawlers/README.md
packages/storage/src/encryption.rs
packages/storage/src/dal.rs
packages/storage/src/columnar.rs
packages/storage/src/postgres_kv.rs
packages/storage/src/lib.rs
packages/storage/src/cache.rs
packages/storage/src/partition.rs
packages/storage/src/diagnosis.rs
packages/storage/src/clickhouse.rs
packages/storage/src/dataset_registry.rs
services/collector/src/scheduler.rs
services/collector/src/multilang_simple.rs
services/collector/src/main.rs
services/collector/src/rate_limiter.rs
services/collector/src/storage.rs
services/collector/src/task_templates.rs
services/collector/src/prelude.rs
services/collector/src/sources/mod_sina.rs
services/collector/src/sources/mod_eastmoney.rs
services/collector/src/sources/mod_163.rs
services/collector/src/sources/mod.rs
services/collector/src/sources/mod_tencent.rs
services/collector/src/sources/ua_rotation.rs
services/collector/src/lib.rs
services/collector/src/cron_scheduler.rs
services/collector/src/source_scheduler.rs
services/collector/src/raw_archive.rs
config/grafana/dashboards/alpha-services-overview.json
crawlers/python/eastmoney_quote.py
services/alert-webhook/Dockerfile
services/alert-webhook/Cargo.toml
services/alert-webhook/src/main.rs
config/grafana/provisioning/dashboards/dashboards.yml
build-wasm.sh
mobile/Cargo.toml
config/nginx/web-origin.conf
web/package.json
web/duckdb-ui.mjs
config/grafana/provisioning/datasources/datasources.yml
[0m$ [0mpwd && git status --short && rg --files -g 'AGENTS.md' -g 'CLAUDE.md' -g 'project.yaml' -g 'README*' -g '*TODO*' -g '*todo*' -g 'docs/**' -g 'production/**' | head -300
/home/cui/workspaces/alpha
 D web/dist/.gitkeep
TODO.md
README.zh.md
README_DEPLOYMENT.md
patches/arrow-arith-50.0.0/README.md
crawlers/README.md
README.md
docs/analysis.md
docs/auth.md
docs/mobile-push-sync.md
docs/android-release.md
docs/.gitignore
docs/user-guide.md
docs/docusaurus.config.js
docs/patches/braces@3.0.3.patch
docs/rust-code-standards.md
docs/alerting-and-diagnosis.md
docs/deployment.md
docs/web-framework-selection.md
docs/static/.gitkeep
docs/mobile-gestures.md
docs/.docusaurus/site-storage.json
docs/.docusaurus/routes.js
docs/platform-compliance.md
docs/审计-文档一致性.md
docs/cross-platform-architecture.md
docs/android-widget.md
docs/sidebars.js
docs/desktop-framework.md
docs/realtime-sync-protocol.md
docs/desktop-release.md
docs/ios-release.md
docs/README.md
docs/version-management.md
docs/architecture-review.md
docs/testing.md
docs/market-data-api.md
docs/theme-adaptation.md
docs/data-privacy.md
docs/intro.md
docs/package.json
docs/mobile-core-architecture.md
docs/pgo-build-optimization.md
docs/docker-deployment.md
docs/distributed-tracing.md
docs/web-cdn.md
docs/getting-started.md
docs/architecture.md
docs/.docusaurus/docusaurus.config.mjs
docs/.docusaurus/globalData.json
docs/.docusaurus/routesChunkNames.json
docs/.docusaurus/site-metadata.json
web/README.md
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-platform-compliance-md-27b.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-rust-code-standards-md-898.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-mobile-offline-md-f07.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-installation-md-575.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-deployment-md-b7b.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-desktop-release-md-576.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-memory-profiling-md-4a6.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-alerting-and-diagnosis-md-9e4.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-intro-md-2e5.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-data-lake-parquet-md-58d.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-testing-md-a56.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-pgo-build-optimization-md-735.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-architecture-md-ba3.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-docker-deployment-md-61a.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-mobile-core-architecture-md-6b6.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-deployment-runbook-md-d3a.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-getting-started-md-243.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-distributed-tracing-md-e8f.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-readme-md-23c.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-web-framework-selection-md-8c7.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-realtime-sync-protocol-md-981.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-mobile-gestures-md-f5c.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-auth-md-aa0.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-user-guide-md-c8f.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-ios-live-activities-md-7a3.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-version-management-md-1e1.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-web-cdn-md-8c6.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-ios-release-md-22b.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-cross-platform-architecture-md-6e7.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/__plugin.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-desktop-framework-md-806.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-store-publishing-md-8d5.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-android-widget-md-f0c.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-architecture-review-md-dcf.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-mobile-privacy-md-f28.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-mobile-push-sync-md-4a3.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-data-privacy-md-007.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-auto-update-md-d78.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/__mdx-loader-dependency.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-android-release-md-b50.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-account-sync-md-30b.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-ux-consistency-md-18a.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-theme-adaptation-md-42d.json
docs/pnpm-lock.yaml
docs/auto-update.md
docs/memory-profiling.md
docs/mobile-offline.md
docs/mobile-privacy.md
docs/deployment-runbook.md
docs/ux-consistency.md
docs/data-lake-parquet.md
docs/store-publishing.md
docs/ios-live-activities.md
docs/account-sync.md
docs/installation.md
docs/pnpm-workspace.yaml
docs/.docusaurus/DONT-EDIT-THIS-FOLDER
docs/.docusaurus/registry.js
docs/.docusaurus/codeTranslations.json
docs/.docusaurus/i18n.json
docs/.docusaurus/client-manifest.json
docs/.docusaurus/client-modules.js
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-analysis-md-304.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/site-market-data-api-md-100.json
docs/.docusaurus/docusaurus-plugin-content-docs/default/p/alpha-docs-a37.json
docs/src/css/custom.css
scripts/desktop-fake-pc/README.md
docs/src/pages/index.js
packages/storage/README.md
docs/.docusaurus/docusaurus-plugin-content-pages/default/__plugin.json
mobile/android/README.md
desktop/icons/README.md
[0m$ [0mrg -n -i '\b(todo|fixme|xxx|hack)\b|\[ \]' --glob '!node_modules/**' --glob '!vendor/**' --glob '!dist/**' --glob '!build/**' . | head -300
./Cargo.toml:21:    # 平台壳/工具链归后续 TODO，见 docs/mobile-core-architecture.md）
./TODO.md:1:# TODO
./scripts/check-cross-platform.sh:2:# 跨平台兼容性检查（TODO：跨平台 Rust 架构设计 — 统一 Rust 代码规范和跨平台兼容性检查）
./scripts/check-version.sh:2:# 版本一致性门禁（TODO L472）：五处版本号必须同值，SW 缓存名与版本同后缀。
./tools/build-tools/src/main.rs:198:    // TODO: 实现服务逻辑
./tools/build-tools/src/main.rs:275:  // TODO: 定义组件属性
./tools/build-tools/src/main.rs:282:      {{/* TODO: 实现组件逻辑 */}}
./tools/build-tools/src/main.rs:323:    <!-- TODO: 实现模板 -->
./tools/build-tools/src/main.rs:328:// TODO: 实现组件逻辑
./tools/build-tools/src/main.rs:330:  // TODO: 定义组件属性
./tools/build-tools/src/main.rs:338:  /* TODO: 实现样式 */
./docs/analysis.md:4:> 并给出按价值排序的行动清单（见 §5，已同步到 TODO.md）。
./docs/analysis.md:57:## 5. 行动清单（按价值排序，已同步 TODO.md）
./scripts/build-images.sh:2:# 跨平台 Docker 镜像构建（TODO L468，方案文档 docs/docker-deployment.md）
./docs/mobile-push-sync.md:35:| 自建推送（长连接 + 服务端） | 本仓已有 real-time-feed WS 面，但移动端需自管心跳/重连 | 归后续 TODO；不属「推送通道」骨架范围 |
./docs/mobile-push-sync.md:86:已在 L114 注明（移动端通知是平台能力，桌面归托盘/系统通知 TODO）。
./docs/mobile-push-sync.md:100:  送达实现留真机 TODO）+ WorkManager 装配面（`PeriodicSyncWorker` + enqueue 帮助
./docs/mobile-push-sync.md:101:  函数已实现，尚未在壳入口接线，归真机 TODO，映射见 §3）。
./docs/mobile-push-sync.md:144:4. 远程通道（FCM/APNs）接入时的凭据链与可靠性——归后续 TODO，本轮不引入。
./docs/mobile-push-sync.md:147:## 11. 后续 TODO 映射
./docs/mobile-push-sync.md:149:| TODO 项 | 本文依托 |
./scripts/check-desktop.sh:2:# 桌面端框架门禁（TODO L112「搭建 Tauri + Rust 桌面应用框架」的构建门禁；非交互）
./scripts/check-miri.sh:2:# Miri 内存安全静态分析（TODO L496；非交互）
./docs/审计-文档一致性.md:5:> 范围：README/README.zh、docs/**、公开门面（GitHub About），加上 TODO 自吹式与 `- [x]` 勾选主张。
./docs/审计-文档一致性.md:52:| F-26 | docs/user-guide.md（通知节） | 「通知需显式开启（opt-in），见应用内隐私设置」 | mobile/android/app/src/main/AndroidManifest.xml 无 POST_NOTIFICATIONS；web/app/src 无 Notification.requestPermission；checkAlerts/setAlertRules 仅存在于 FFI 桥（AlphaBridge.kt + 生成绑定），无应用内设置 UI | 文档超前于码 | P1 | 改「通知为真机 TODO，无应用内设置面；opt-in 语义待壳层接线」 |
./docs/user-guide.md:48:- 通知为真机 TODO，无应用内设置面；opt-in 语义待壳层接线。
./docs/cross-platform-architecture.md:3:> 状态：v1 草案（2026-09-28），对应 TODO「跨平台 Rust 架构设计」首项「设计统一跨平台架构」。
./docs/cross-platform-architecture.md:28:| `web/` | 原生 JS + duckdb-wasm vendor + WebSocket | 浏览器 | 已接 real-time-feed WS 与 data-engine REST（见 TODO P3-2） |
./docs/cross-platform-architecture.md:119:## 7. 落地路线图（映射本节 TODO 余项）
./docs/cross-platform-architecture.md:121:| TODO 项 | 依赖/顺序 | 说明 |
./docs/android-widget.md:31:热路由归单实例改造 TODO）；动态 shortcuts（随用户观察列表生成）归设置
./docs/android-widget.md:32:页 TODO。
./scripts/store-release.sh:19:#     （全流程已在本地 mock 服务上实测四端点时序，见 TODO 注记）。
./packages/core/src/account.rs:1://! 统一账户与跨端数据同步的协议/领域面（纯函数，TODO L476）
./docs/desktop-framework.md:3:> TODO.md L112「搭建 Tauri + Rust 桌面应用框架」的落地说明。
./docs/desktop-framework.md:230:| TODO | 现状 | 下一步 |
./scripts/check-coverage.sh:2:# 代码覆盖率报告（TODO L494；非交互）
./WASM_COMPLETION_SUMMARY.md:213:- [ ] SIMD 向量化优化 (rustc target-feature=+simd128)
./WASM_COMPLETION_SUMMARY.md:216:- [ ] WebGPU 加速
./docs/realtime-sync-protocol.md:3:> 对应 TODO「实现实时数据同步协议（WebSocket 增量更新 + 版本控制）」最小可用版本。
./docs/theme-adaptation.md:21:   进程内热切换随设置页 TODO 一起接）。
./docs/rust-code-standards.md:3:> 状态 v1（2026-09-28），对应 TODO「定义统一的 Rust 代码规范和跨平台兼容性检查」。
./docs/rust-code-standards.md:100:  TODO.md 记录进度与依据，二者在 commit message 中互相引用。
./docs/rust-code-standards.md:138:缺 Redis 简单获取途径，其编译面由 `build` 覆盖，运行期矩阵归 TODO L469 后续登记）；
./docs/rust-code-standards.md:157:  TODO.md §拍板登记 RustSec 条目；
./docs/README.md:32:- 标题不带内部条目编号（`（TODO Lxxx）` 不出现在 H1）；正文中的设计登记
./docs/README.md:33:  引用（`docs/xxx.md §n`、L 编号）保留，是本仓设计文档的溯源口径。
./docs/README.md:36:- 文档间链接用相对路径（`./xxx.md`），构建时会校验。
./packages/core/src/backtest.rs:3://! 最小可用口径（TODO「开发高性能 Rust WASM 核心计算库」，2026-09-28）：
./web/README.md:56:## React 骨架（`app/`，TODO L427）
./scripts/check-perf.sh:2:# 自动化性能回归检测（TODO L495；非交互）
./docs/data-lake-parquet.md:118:## 9. 与后续 TODO 的边界
./docs/web-framework-selection.md:27:| 本仓既有投入 | 零（web/ 为 vanilla JS，无迁移负担） | 零 | 零（TODO L428 仅是设想项，无一行代码） |
./docs/web-framework-selection.md:28:| 后续 TODO 项契合度 | L428 组件化界面 / L429 图表库 / L430 SQL 编辑器均有现成 React 路线 | L430 需自封装 | 全部需自行开发 |
./docs/web-framework-selection.md:41:- **TODO L428**（「基于 Yew/Leptos 开发 Web 端组件化数据分析界面」）：✅
./docs/web-framework-selection.md:45:- **TODO L429**（图表库 D3 + Canvas）：✅ **已定论 lightweight-charts 5.2 并落地**
./docs/web-framework-selection.md:50:  ~300KB+）落备选。TODO 原文「D3.js」按此定论解释落实。
./docs/web-framework-selection.md:119:  随 A 落地撤销备选。落地分三步（见 TODO 拍板登记）：桥接模块 + 产物接入
./docs/web-framework-selection.md:141:1. 记账行号 L427（沿 TODO 行号；行号漂移不作编号依据）。
./packages/core/src/alerts.rs:1://! 业务级智能告警规则引擎（纯函数，TODO L503）
./docs/mobile-offline.md:18:| **同步（sync / delta）** | 与**远端**做增量对齐：以指纹比较决定是否需要对齐、对齐哪些数据 | **出设备**（经网络到远端） | 决策与增量判断本轮落地；真实网络执行归远端接入 TODO（§8④） |
./docs/mobile-offline.md:36:   后续 TODO，配置与文案面本轮就绪）。
./docs/mobile-offline.md:88:  设置页 TODO）；语义对齐 alpha-core `platform::KeyValueStore`：后写覆盖、删除幂等。
./docs/mobile-offline.md:121:1. 本轮台账记 L390（沿 TODO 行号；行号漂移不作编号依据）。
./docs/mobile-offline.md:126:   决策 + 增量判断契约；`SyncDelta.needed=true` 后壳层的对齐动作归远端接入 TODO。
./docs/mobile-offline.md:128:   跨版本迁移策略归发布流水线 TODO。
./docs/mobile-offline.md:132:   后续 TODO，本轮交付配置面 + `dataScopeSummary` 文案函数。
./docs/mobile-offline.md:146:## 10. 后续 TODO 映射
./docs/mobile-offline.md:148:| TODO 项 | 本文依托 |
./docs/mobile-gestures.md:55:页面范围**不扩**：本轮不新增二级页/导航（导航骨架归后续 TODO，见 §9）。
./docs/mobile-gestures.md:109:1. 本轮台账记 L367（沿 TODO 行号；行号漂移不作编号依据）。
./docs/mobile-gestures.md:118:6. 页面范围不扩（沿用 L301 单页），导航/二级页归后续 TODO（§9）。
./docs/mobile-gestures.md:130:## 9. 后续 TODO 映射
./docs/mobile-gestures.md:132:| TODO 项 | 本文依托 |
./docs/mobile-privacy.md:76:  TODO 一起接，见 docs/mobile-offline.md §8⑦）；
./packages/core/src/streaming.rs:1://! 流式数据处理与并行聚合核心（TODO「开发流式数据处理和并行计算机制（Web Workers + Rayon）」
./docs/mobile-core-architecture.md:3:> TODO.md「移动端应用」节第 1 项「设计移动端 Rust 核心库架构（JNI + UniFFI）」
./docs/mobile-core-architecture.md:14:* **L301 增量交付**（TODO 270）：Android Kotlin + Jetpack Compose 混合工程
./docs/mobile-core-architecture.md:18:* **仍不交付**（按 TODO 顺序留给后续项，见 §13）：L118 首轮清单中 272/273/274
./docs/mobile-core-architecture.md:26:┌──────────── 平台壳（后续 TODO：270/271）────────────┐
./docs/mobile-core-architecture.md:54:| **裸 JNI**（`jni = 0.21`，workspace 已 pin） | 逃生舱：uniffi 覆盖不到的场景——SDK 级回调（推送 token、生命周期）、性能敏感的零拷贝通道 | ⏸ 本轮不引用；架构上预留 pin 与本文档，首个真实场景（TODO 270/272）落地时启用 |
./docs/mobile-core-architecture.md:108:`MobileCore` 持 `symbols`（观察列表）与 `api_url`（后端地址配置槽，TODO 272/274
./docs/mobile-core-architecture.md:111:`api_url` 骨架内只随 `status_json` 下行（探测/同步属 TODO 274，本轮不联网）。
./docs/mobile-core-architecture.md:117:/`synthetic_series` 两个入口）。真实数据源接入（api-gateway）是后续 TODO，届时
./docs/mobile-core-architecture.md:127:| iOS | `staticlib`（`.a`，Swift 侧经 uniffi 生成的 Swift 绑定链接） | Xcode + Swift Package（TODO 271） | 骨架已落（L119：SwiftUI + UniFFI 绑定骨架 + 模型契约）；Xcode 工具链链路未启（无 macOS/Xcode） |
./docs/mobile-core-architecture.md:205:   TODO.md 行号会随注释增长漂移，不作编号依据）。
./docs/mobile-core-architecture.md:210:   （TODO 270/272）启用。
./docs/mobile-core-architecture.md:233:   wasm 门控同款式），不做 optional feature——TODO 272 启用 JNI 逃生舱时
./docs/mobile-core-architecture.md:245:4. 推送 token/生命周期等 SDK 回调经 JNI 逃生舱回灌核心库（TODO 272 的首个
./docs/mobile-core-architecture.md:250:## 13. 后续 TODO 映射
./docs/mobile-core-architecture.md:252:| TODO 项 | 本文依托 | 状态 |
./wasm-analyzer/examples/streaming_bench.rs:1://! 流式 + 并行 A/B 基准（TODO「开发流式数据处理和并行计算机制」验证件）
./wasm-analyzer/examples/streaming_bench.rs:118:    println!("      数值随机器波动，绝对值仅供对比参考（记录于 TODO L89 注与 commit message）。");
./packages/core/src/sync.rs:8://! ## 版本与增量契约（最小可用口径，TODO「实现实时数据同步协议」）
./packages/core/src/sync.rs:534:    /// 亦收敛。覆盖 TODO 同步协议的完整闭环。
./docs/pgo-build-optimization.md:59:   PGO 二进制随发布流水线（TODO「多平台发布流水线」）产出并归档 profdata 溯源。
./wasm-analyzer/examples/zero_copy_bench.rs:1://! 零拷贝 A/B 基准（native 近似口径，TODO「实现零拷贝内存管理」验证件）
./wasm-analyzer/examples/zero_copy_bench.rs:64:    println!("      数值随机器波动，绝对值仅供对比参考（记录于 TODO L54 注与 commit message）。");
./scripts/bump-version.sh:2:# 发版版本号推进（TODO L472）：一处输入，五处同改。
./scripts/check-e2e.sh:2:# 端到端跨服务冒烟测试（TODO L469，方案见 docs/docker-deployment.md §4 与 TODO 标注）
./scripts/desktop-fake-pc/README.md:7:CI 的 macOS 作业编译，签名漂移要等一次推送往返才暴露（TODO L112 连续两次 CI 红灯）。
./web/app/src/lib/privacy.ts:2: * 数据主体权利工具（TODO L488）：GDPR「访问/可携权」与「删除权/被遗忘权」
./services/data-engine/src/main.rs:1021:/// 增长，见 TODO.md P2）——/query 每次执行前自行 refresh（execute_query），
./wasm-analyzer/src/storage.rs:3://! TODO「构建混合存储架构（WASM 内存 + IndexedDB + 服务端缓存）」wasm 侧落地件。
./desktop/tests/tauri_config.rs:1://! `tauri.conf.json` 契约测试（TODO L112 构建门禁的可执行部分）
./services/api-gateway/src/account.rs:1://! 统一账户与跨端数据同步的服务端存储面（TODO L476）
./desktop/tests/wiring_contract.rs:1://! 接线层薄度契约（TODO L112 构建门禁的可执行部分之二）
./packages/protocols/src/websocket.rs:71:/// 版本与增量契约（TODO「实现实时数据同步协议（WebSocket 增量更新 + 版本控制）」的最小口径）：
./packages/core/src/hybrid_cache.rs:1://! 混合存储架构核心（TODO「构建混合存储架构（WASM 内存 + IndexedDB + 服务端缓存）」落地件）。
./services/api-gateway/src/shield.rs:1://! 防爬虫与应用层 DDoS 护栏（TODO L486）
./services/api-gateway/src/audit.rs:1://! 安全审计日志与异常行为检测（TODO L487）
./wasm-analyzer/node-bench/zero-copy.mjs:1:// 零拷贝 A/B 基准——真实 wasm pkg 产物（JS 边界口径，TODO「实现零拷贝内存管理」验证件）
./web/app/src/demoData.ts:3: * 真实数据面（REST/WS 经 services）归远端接入 TODO，骨架期不碰网络。
./mobile/tests/android_shell_contract.rs:403:            "interface NotificationDispatcher", // 送达口（平台实现归真机 TODO）
./web/app/src/lib/i18n.ts:24:    'TODO L427 选型 + L428 组件化 + L429 图表库 + L430 SQL 工作台 + L431 响应式 + L432 主题系统 + L499 实时看板 + L508 多标签工作区。旧演示页（web/index.html 等）零改动并存。',
./web/app/src/lib/i18n.ts:158:    'TODO L427 selection + L428 components + L429 charts + L430 SQL workbench + L431 responsive + L432 theming + L499 live board + L508 workspace tabs. Legacy demo pages (web/index.html, etc.) coexist untouched.',
./wasm-analyzer/src/shared_buffer.rs:1://! wasm 边界零拷贝价格缓冲区（TODO「实现零拷贝内存管理与 Arrow 数据格式优化」落地件）。
./wasm-analyzer/src/lib.rs:428:    /// （复用缓冲）→ `freePriceBuffer`（TODO「实现零拷贝内存管理」，A/B 基准见
./wasm-analyzer/src/lib.rs:429:    /// examples/zero_copy_bench.rs 与 TODO 注）。
./desktop/src/market.rs:3://! 骨架阶段尚无真实数据源（接入 api-gateway 属 TODO「统一配置管理」之后的接线工作），
./desktop/src/market.rs:8://! 真实数据接入时（TODO L113 本地数据导出等）只需替换本模块的取数实现，
./web/app/src/components/PrivacyPanel.tsx:12: * 数据与隐私面板（TODO L488）：GDPR/CCPA 数据主体权利的 UI 面——
./desktop/src/lib.rs:1://! Alpha Finance 桌面端框架层（TODO.md L112「搭建 Tauri + Rust 桌面应用框架」）
./mobile/src/market.rs:3://! 骨架阶段尚无真实数据源（接入 api-gateway 属后续 TODO），但演示数据不能是
./desktop/src/shortcuts.rs:1://! 键盘快捷键与右键菜单模型（TODO L117「开发键盘快捷键和右键菜单支持」）
./desktop/src/kv.rs:3://! TODO L38（跨平台适配层）只给了 trait 面与说明（"Desktop（Tauri）: KeyValueStore
./desktop/src/offline.rs:1://! 本地数据库同步与离线模式（TODO L116）
./desktop/src/export.rs:3://! 两条写路径（TODO L112 骨架 + L113 原生文件集成）：
./mobile/src/lib.rs:1://! Alpha Finance 移动端 Rust 核心库（TODO「设计移动端 Rust 核心库架构
./desktop/src/window.rs:1://! 窗口几何持久化与多显示器放置（TODO L115「构建跨平台窗口管理和主题适配」）
./desktop/src/alerts.rs:4://! 触发与系统通知属 TODO L114（系统通知/托盘），此处只管持久化。
./desktop/src/notify.rs:1://! 系统通知与托盘状态（TODO L114「开发系统通知和托盘集成功能」）
./desktop/src/notify.rs:263:/// 常用菜单三件套即 TODO L114 的口径：显示/隐藏主窗、退出。
./mobile/src/state.rs:61:    /// 后端地址配置槽（骨架期只随 `status_json` 下行；探测/同步归后续 TODO）
./mobile/android/README.md:58:## 边界（真机/发布归后续 TODO）
./mobile/android/README.md:60:- 真机加载 `.so` 与运行时验证：本机无设备，模拟器/真机验收归后续 TODO；
./mobile/android/README.md:62:- 发布流水线（签名/R8 规则/AAB 分包）：TODO「多平台发布流水线」。
./mobile/android/README.md:63:- JNI 直接回调（SDK 推送/生命周期）：TODO 272 逃生舱，当前纯 UniFFI 面。
./mobile/android/app/build.gradle.kts:64:            // 混淆关闭：uniffi 生成面靠 JNA 反射，R8 处理规则归发布流水线 TODO
./mobile/android/app/src/main/AndroidManifest.xml:3:     演示数据由 Rust 侧生成；api-gateway 接入与 INTERNET 权限归后续 TODO） -->
./mobile/android/app/src/main/java/uniffi/alpha_mobile/alpha_mobile.kt:323:// TODO: refactor callbacks to use this class
./mobile/android/app/src/main/java/uniffi/alpha_mobile/alpha_mobile.kt:830:        // TODO: maybe we should log a warning if called more than once?
./mobile/android/app/src/main/res/xml/shortcuts.xml:4:     演示值，动态 shortcuts（随用户观察列表生成）归设置页 TODO。 -->
./mobile/android/app/src/main/java/com/alpha/finance/mobile/MainActivity.kt:44: * 接入、配置下行与离线同步归后续 TODO。
./mobile/android/app/src/main/java/com/alpha/finance/mobile/MainActivity.kt:77:        // 每次新建实例走 onCreate；进程内热路由归单实例改造 TODO）
./mobile/android/app/src/main/java/com/alpha/finance/mobile/OfflineStore.kt:52: * TODO 一起接，见 docs/mobile-offline.md §8⑦）
./mobile/android/app/src/main/java/com/alpha/finance/mobile/OfflineStore.kt:117: *   以 0 起步。**不发起网络请求**——真实对齐执行归远端接入 TODO（文档 §8④）。
./mobile/android/app/src/main/java/com/alpha/finance/mobile/PushSyncSeam.kt:18: * 归 L339 / 真机 TODO（文档 §10）。
./mobile/android/build.gradle.kts:6:// Kotlin 须先升级 uniffi（另起 TODO，见 docs/mobile-core-architecture.md 假设⑩）。
> build · gpt-6-luna
[0m→ [0mRead TODO.md [90m[limit=600][0m
[0m→ [0mRead Cargo.toml [90m[limit=260][0m
[0m→ [0mRead docs/architecture.md [90m[limit=400][0m
[0m→ [0mRead docs/审计-文档一致性.md [90m[limit=300][0m
[0m$ [0mfind services web/app mobile desktop packages -maxdepth 4 -type f | sort
m_inject.rs
packages/storage/README.md
packages/storage/src/bin/clickhouse_test.rs
packages/storage/src/cache.rs
packages/storage/src/clickhouse.rs
packages/storage/src/cloud.rs
packages/storage/src/columnar.rs
packages/storage/src/dal.rs
packages/storage/src/dataset_registry.rs
packages/storage/src/diagnosis.rs
packages/storage/src/disk_kv.rs
packages/storage/src/encryption.rs
packages/storage/src/lake.rs
packages/storage/src/lib.rs
packages/storage/src/memory.rs
packages/storage/src/partition.rs
packages/storage/src/postgres_kv.rs
packages/storage/src/prefetch.rs
packages/storage/src/rate_limit.rs
packages/storage/src/redis_kv.rs
packages/storage/src/redis_streams.rs
packages/storage/src/timescale.rs
packages/storage/src/timeseries.rs
packages/storage/tests/redis_streams.rs
services/alert-webhook/Cargo.toml
services/alert-webhook/Dockerfile
services/alert-webhook/src/main.rs
services/api-gateway/Cargo.toml
services/api-gateway/Dockerfile
services/api-gateway/src/account.rs
services/api-gateway/src/audit.rs
services/api-gateway/src/auth.rs
services/api-gateway/src/main.rs
services/api-gateway/src/shield.rs
services/collector/Cargo.toml
services/collector/COLLECTOR_README.md
services/collector/COMPLETION_REPORT.md
services/collector/crawler_integration_config.json
services/collector/crawlers.json
services/collector/Dockerfile
services/collector/examples/crawler_integration_demo.rs
services/collector/examples/integration_summary.rs
services/collector/examples/simple_integration_demo.rs
services/collector/src/cleaner.rs
services/collector/src/cron_scheduler.rs
services/collector/src/lib.rs
services/collector/src/main.rs
services/collector/src/main_simple.rs
services/collector/src/multilang_simple.rs
services/collector/src/prelude.rs
services/collector/src/rate_limiter.rs
services/collector/src/raw_archive.rs
services/collector/src/scheduler.rs
services/collector/src/source_health.rs
services/collector/src/source_scheduler.rs
services/collector/src/sources/mod_163.rs
services/collector/src/sources/mod_eastmoney.rs
services/collector/src/sources/mod.rs
services/collector/src/sources/mod_sina.rs
services/collector/src/sources/mod_tencent.rs
services/collector/src/sources/ua_rotation.rs
services/collector/src/storage.rs
services/collector/src/task_templates.rs
services/collector/src/types.rs
services/data-engine/Cargo.toml
services/data-engine/Dockerfile
services/data-engine/src/grpc.rs
services/data-engine/src/main.rs
services/data-engine/src/outlier.rs
services/data-engine/src/query_udfs.rs
services/data-engine/src/sequence_gap.rs
services/data-engine/src/settings.rs
services/data-engine/src/source_divergence.rs
services/data-engine/tests/normalizer_dedup.rs
services/real-time-feed/Cargo.toml
services/real-time-feed/Dockerfile
services/real-time-feed/src/main.rs
services/real-time-feed/tests/pipeline_compat.rs
web/app/dist/assets/index-CfUhsKLZ.css
web/app/dist/assets/index-DAvyM14c.js
web/app/dist/.gitkeep
web/app/dist/icon.svg
web/app/dist/index.html
web/app/dist/manifest.webmanifest
web/app/dist/sw.js
web/app/dist/vendor/duckdb/duckdb-browser.mjs
web/app/dist/vendor/duckdb/duckdb-browser-mvp.worker.js
web/app/dist/vendor/duckdb/duckdb-mvp.wasm
web/app/index.html
web/app/node_modules/assertion-error/index.d.ts
web/app/node_modules/assertion-error/index.js
web/app/node_modules/assertion-error/LICENSE
web/app/node_modules/assertion-error/package.json
web/app/node_modules/assertion-error/README.md
web/app/node_modules/@babel/code-frame/LICENSE
web/app/node_modules/@babel/code-frame/package.json
web/app/node_modules/@babel/code-frame/README.md
web/app/node_modules/@babel/compat-data/corejs2-built-ins.js
web/app/node_modules/@babel/compat-data/corejs3-shipped-proposals.js
web/app/node_modules/@babel/compat-data/LICENSE
web/app/node_modules/@babel/compat-data/native-modules.js
web/app/node_modules/@babel/compat-data/overlapping-plugins.js
web/app/node_modules/@babel/compat-data/package.json
web/app/node_modules/@babel/compat-data/plugin-bugfixes.js
web/app/node_modules/@babel/compat-data/plugins.js
web/app/node_modules/@babel/compat-data/README.md
web/app/node_modules/@babel/core/LICENSE
web/app/node_modules/@babel/core/package.json
web/app/node_modules/@babel/core/README.md
web/app/node_modules/@babel/generator/LICENSE
web/app/node_modules/@babel/generator/package.json
web/app/node_modules/@babel/generator/README.md
web/app/node_modules/@babel/helper-compilation-targets/LICENSE
web/app/node_modules/@babel/helper-compilation-targets/package.json
web/app/node_modules/@babel/helper-compilation-targets/README.md
web/app/node_modules/@babel/helper-globals/LICENSE
web/app/node_modules/@babel/helper-globals/package.json
web/app/node_modules/@babel/helper-globals/README.md
web/app/node_modules/@babel/helper-module-imports/LICENSE
web/app/node_modules/@babel/helper-module-imports/package.json
web/app/node_modules/@babel/helper-module-imports/README.md
web/app/node_modules/@babel/helper-module-transforms/LICENSE
web/app/node_modules/@babel/helper-module-transforms/package.json
web/app/node_modules/@babel/helper-module-transforms/README.md
web/app/node_modules/@babel/helper-plugin-utils/LICENSE
web/app/node_modules/@babel/helper-plugin-utils/package.json
web/app/node_modules/@babel/helper-plugin-utils/README.md
web/app/node_modules/@babel/helpers/LICENSE
web/app/node_modules/@babel/helpers/package.json
web/app/node_modules/@babel/helpers/README.md
web/app/node_modules/@babel/helper-string-parser/LICENSE
web/app/node_modules/@babel/helper-string-parser/package.json
web/app/node_modules/@babel/helper-string-parser/README.md
web/app/node_modules/@babel/helper-validator-identifier/LICENSE
web/app/node_modules/@babel/helper-validator-identifier/package.json
web/app/node_modules/@babel/helper-validator-identifier/README.md
web/app/node_modules/@babel/helper-validator-option/LICENSE
web/app/node_modules/@babel/helper-validator-option/package.json
web/app/node_modules/@babel/helper-validator-option/README.md
web/app/node_modules/@babel/parser/CHANGELOG.md
web/app/node_modules/@babel/parser/LICENSE
web/app/node_modules/@babel/parser/package.json
web/app/node_modules/@babel/parser/README.md
web/app/node_modules/@babel/plugin-transform-react-jsx-self/LICENSE
web/app/node_modules/@babel/plugin-transform-react-jsx-self/package.json
web/app/node_modules/@babel/plugin-transform-react-jsx-self/README.md
web/app/node_modules/@babel/plugin-transform-react-jsx-source/LICENSE
web/app/node_modules/@babel/plugin-transform-react-jsx-source/package.json
web/app/node_modules/@babel/plugin-transform-react-jsx-source/README.md
web/app/node_modules/@babel/template/LICENSE
web/app/node_modules/@babel/template/package.json
web/app/node_modules/@babel/template/README.md
web/app/node_modules/@babel/traverse/LICENSE
web/app/node_modules/@babel/traverse/package.json
web/app/node_modules/@babel/traverse/README.md
web/app/node_modules/@babel/traverse/tsconfig.overrides.json
web/app/node_modules/@babel/types/LICENSE
web/app/node_modules/@babel/types/package.json
web/app/node_modules/@babel/types/README.md
web/app/node_modules/baseline-browser-mapping/dist/cli.cjs
web/app/node_modules/baseline-browser-mapping/dist/index.cjs
web/app/node_modules/baseline-browser-mapping/dist/index.d.ts
web/app/node_modules/baseline-browser-mapping/dist/index.js
web/app/node_modules/baseline-browser-mapping/dist/types.d.ts
web/app/node_modules/baseline-browser-mapping/dist/utils.d.ts
web/app/node_modules/baseline-browser-mapping/LICENSE.txt
web/app/node_modules/baseline-browser-mapping/package.json
web/app/node_modules/baseline-browser-mapping/README.md
web/app/node_modules/browserslist/browser.js
web/app/node_modules/browserslist/cli.js
web/app/node_modules/browserslist/error.d.ts
web/app/node_modules/browserslist/error.js
web/app/node_modules/browserslist/index.d.ts
web/app/node_modules/browserslist/index.js
web/app/node_modules/browserslist/LICENSE
web/app/node_modules/browserslist/node.js
web/app/node_modules/browserslist/package.json
web/app/node_modules/browserslist/parse.js
web/app/node_modules/browserslist/README.md
web/app/node_modules/caniuse-lite/data/agents.js
web/app/node_modules/caniuse-lite/data/browsers.js
web/app/node_modules/caniuse-lite/data/browserVersions.js
web/app/node_modules/caniuse-lite/data/features.js
web/app/node_modules/caniuse-lite/data/versionGroups.js
web/app/node_modules/caniuse-lite/LICENSE
web/app/node_modules/caniuse-lite/package.json
web/app/node_modules/caniuse-lite/README.md
web/app/node_modules/chai/index.js
web/app/node_modules/chai/LICENSE
web/app/node_modules/chai/package.json
web/app/node_modules/chai/README.md
web/app/node_modules/chai/register-assert.js
web/app/node_modules/chai/register-expect.js
web/app/node_modules/chai/register-should.js
web/app/node_modules/convert-source-map/index.js
web/app/node_modules/convert-source-map/LICENSE
web/app/node_modules/convert-source-map/package.json
web/app/node_modules/convert-source-map/README.md
web/app/node_modules/csstype/index.d.ts
web/app/node_modules/csstype/index.js.flow
web/app/node_modules/csstype/LICENSE
web/app/node_modules/csstype/package.json
web/app/node_modules/csstype/README.md
web/app/node_modules/debug/LICENSE
web/app/node_modules/debug/package.json
web/app/node_modules/debug/README.md
web/app/node_modules/debug/src/browser.js
web/app/node_modules/debug/src/common.js
web/app/node_modules/debug/src/index.js
web/app/node_modules/debug/src/node.js
web/app/node_modules/detect-libc/index.d.ts
web/app/node_modules/detect-libc/lib/detect-libc.js
web/app/node_modules/detect-libc/lib/elf.js
web/app/node_modules/detect-libc/lib/filesystem.js
web/app/node_modules/detect-libc/lib/process.js
web/app/node_modules/detect-libc/LICENSE
web/app/node_modules/detect-libc/package.json
web/app/node_modules/detect-libc/README.md
web/app/node_modules/electron-to-chromium/chromium-versions.js
web/app/node_modules/electron-to-chromium/chromium-versions.json
web/app/node_modules/electron-to-chromium/full-chromium-versions.js
web/app/node_modules/electron-to-chromium/full-chromium-versions.json
web/app/node_modules/electron-to-chromium/full-versions.js
web/app/node_modules/electron-to-chromium/full-versions.json
web/app/node_modules/electron-to-chromium/index.js
web/app/node_modules/electron-to-chromium/LICENSE
web/app/node_modules/electron-to-chromium/package.json
web/app/node_modules/electron-to-chromium/README.md
web/app/node_modules/electron-to-chromium/versions.js
web/app/node_modules/electron-to-chromium/versions.json
web/app/node_modules/escalade/dist/index.js
web/app/node_modules/escalade/dist/index.mjs
web/app/node_modules/escalade/index.d.mts
web/app/node_modules/escalade/index.d.ts
web/app/node_modules/escalade/license
web/app/node_modules/escalade/package.json
web/app/node_modules/escalade/readme.md
web/app/node_modules/escalade/sync/index.d.mts
web/app/node_modules/escalade/sync/index.d.ts
web/app/node_modules/escalade/sync/index.js
web/app/node_modules/escalade/sync/index.mjs
web/app/node_modules/es-module-lexer/dist/lexer.asm.js
web/app/node_modules/es-module-lexer/dist/lexer.cjs
web/app/node_modules/es-module-lexer/dist/lexer.js
web/app/node_modules/es-module-lexer/dist/lexer.minimal.asm.js
web/app/node_modules/es-module-lexer/dist/lexer.minimal.cjs
web/app/node_modules/es-module-lexer/dist/lexer.minimal.js
web/app/node_modules/es-module-lexer/lexer.js
web/app/node_modules/es-module-lexer/LICENSE
web/app/node_modules/es-module-lexer/package.json
web/app/node_modules/es-module-lexer/README.md
web/app/node_modules/es-module-lexer/types/lexer.d.ts
web/app/node_modules/es-module-lexer/types/lexer.minimal.d.ts
web/app/node_modules/estree-walker/LICENSE
web/app/node_modules/estree-walker/package.json
web/app/node_modules/estree-walker/README.md
web/app/node_modules/estree-walker/src/async.js
web/app/node_modules/estree-walker/src/index.js
web/app/node_modules/estree-walker/src/sync.js
web/app/node_modules/estree-walker/src/walker.js
web/app/node_modules/estree-walker/types/async.d.ts
web/app/node_modules/estree-walker/types/index.d.ts
web/app/node_modules/estree-walker/types/sync.d.ts
web/app/node_modules/estree-walker/types/walker.d.ts
web/app/node_modules/expect-type/dist/branding.d.ts
web/app/node_modules/expect-type/dist/branding.js
web/app/node_modules/expect-type/dist/index.d.ts
web/app/node_modules/expect-type/dist/index.js
web/app/node_modules/expect-type/dist/messages.d.ts
web/app/node_modules/expect-type/dist/messages.js
web/app/node_modules/expect-type/dist/overloads.d.ts
web/app/node_modules/expect-type/dist/overloads.js
web/app/node_modules/expect-type/dist/utils.d.ts
web/app/node_modules/expect-type/dist/utils.js
web/app/node_modules/expect-type/LICENSE
web/app/node_modules/expect-type/package.json
web/app/node_modules/expect-type/README.md
web/app/node_modules/expect-type/SECURITY.md
web/app/node_modules/fancy-canvas/canvas-element-bitmap-size.d.mts
web/app/node_modules/fancy-canvas/canvas-element-bitmap-size.d.ts
web/app/node_modules/fancy-canvas/canvas-element-bitmap-size.js
web/app/node_modules/fancy-canvas/canvas-element-bitmap-size.mjs
web/app/node_modules/fancy-canvas/canvas-rendering-target.d.mts
web/app/node_modules/fancy-canvas/canvas-rendering-target.d.ts
web/app/node_modules/fancy-canvas/canvas-rendering-target.js
web/app/node_modules/fancy-canvas/canvas-rendering-target.mjs
web/app/node_modules/fancy-canvas/device-pixel-ratio.d.mts
web/app/node_modules/fancy-canvas/device-pixel-ratio.d.ts
web/app/node_modules/fancy-canvas/device-pixel-ratio.js
web/app/node_modules/fancy-canvas/device-pixel-ratio.mjs
web/app/node_modules/fancy-canvas/disposable.d.mts
web/app/node_modules/fancy-canvas/disposable.d.ts
web/app/node_modules/fancy-canvas/disposable.js
web/app/node_modules/fancy-canvas/disposable.mjs
web/app/node_modules/fancy-canvas/index.d.mts
web/app/node_modules/fancy-canvas/index.d.ts
web/app/node_modules/fancy-canvas/index.js
web/app/node_modules/fancy-canvas/index.mjs
web/app/node_modules/fancy-canvas/package.json
web/app/node_modules/fancy-canvas/rx.d.mts
web/app/node_modules/fancy-canvas/rx.d.ts
web/app/node_modules/fancy-canvas/rx.js
web/app/node_modules/fancy-canvas/rx.mjs
web/app/node_modules/fancy-canvas/size.d.mts
web/app/node_modules/fancy-canvas/size.d.ts
web/app/node_modules/fancy-canvas/size.js
web/app/node_modules/fancy-canvas/size.mjs
web/app/node_modules/fdir/dist/index.cjs
web/app/node_modules/fdir/dist/index.d.cts
web/app/node_modules/fdir/dist/index.d.mts
web/app/node_modules/fdir/dist/index.mjs
web/app/node_modules/fdir/LICENSE
web/app/node_modules/fdir/package.json
web/app/node_modules/fdir/README.md
web/app/node_modules/gensync/index.js
web/app/node_modules/gensync/index.js.flow
web/app/node_modules/gensync/LICENSE
web/app/node_modules/gensync/package.json
web/app/node_modules/gensync/README.md
web/app/node_modules/gensync/test/.babelrc
web/app/node_modules/gensync/test/index.test.js
web/app/node_modules/@jridgewell/gen-mapping/LICENSE
web/app/node_modules/@jridgewell/gen-mapping/package.json
web/app/node_modules/@jridgewell/gen-mapping/README.md
web/app/node_modules/@jridgewell/remapping/LICENSE
web/app/node_modules/@jridgewell/remapping/package.json
web/app/node_modules/@jridgewell/remapping/README.md
web/app/node_modules/@jridgewell/resolve-uri/LICENSE
web/app/node_modules/@jridgewell/resolve-uri/package.json
web/app/node_modules/@jridgewell/resolve-uri/README.md
web/app/node_modules/@jridgewell/sourcemap-codec/LICENSE
web/app/node_modules/@jridgewell/sourcemap-codec/package.json
web/app/node_modules/@jridgewell/sourcemap-codec/README.md
web/app/node_modules/@jridgewell/trace-mapping/LICENSE
web/app/node_modules/@jridgewell/trace-mapping/package.json
web/app/node_modules/@jridgewell/trace-mapping/README.md
web/app/node_modules/jsesc/bin/jsesc
web/app/node_modules/jsesc/jsesc.js
web/app/node_modules/jsesc/LICENSE-MIT.txt
web/app/node_modules/jsesc/man/jsesc.1
web/app/node_modules/jsesc/package.json
web/app/node_modules/jsesc/README.md
web/app/node_modules/json5/dist/index.js
web/app/node_modules/json5/dist/index.min.js
web/app/node_modules/json5/dist/index.min.mjs
web/app/node_modules/json5/dist/index.mjs
web/app/node_modules/json5/lib/cli.js
web/app/node_modules/json5/lib/index.d.ts
web/app/node_modules/json5/lib/index.js
web/app/node_modules/json5/lib/parse.d.ts
web/app/node_modules/json5/lib/parse.js
web/app/node_modules/json5/lib/register.js
web/app/node_modules/json5/lib/require.js
web/app/node_modules/json5/lib/stringify.d.ts
web/app/node_modules/json5/lib/stringify.js
web/app/node_modules/json5/lib/unicode.d.ts
web/app/node_modules/json5/lib/unicode.js
web/app/node_modules/json5/lib/util.d.ts
web/app/node_modules/json5/lib/util.js
web/app/node_modules/json5/LICENSE.md
web/app/node_modules/json5/package.json
web/app/node_modules/json5/README.md
web/app/node_modules/js-tokens/CHANGELOG.md
web/app/node_modules/js-tokens/index.js
web/app/node_modules/js-tokens/LICENSE
web/app/node_modules/js-tokens/package.json
web/app/node_modules/js-tokens/README.md
web/app/node_modules/lightningcss/LICENSE
web/app/node_modules/lightningcss-linux-x64-gnu/LICENSE
web/app/node_modules/lightningcss-linux-x64-gnu/lightningcss.linux-x64-gnu.node
web/app/node_modules/lightningcss-linux-x64-gnu/package.json
web/app/node_modules/lightningcss-linux-x64-gnu/README.md
web/app/node_modules/lightningcss/node/ast.d.ts
web/app/node_modules/lightningcss/node/ast.js.flow
web/app/node_modules/lightningcss/node/browserslistToTargets.js
web/app/node_modules/lightningcss/node/composeVisitors.js
web/app/node_modules/lightningcss/node/flags.js
web/app/node_modules/lightningcss/node/index.d.ts
web/app/node_modules/lightningcss/node/index.js
web/app/node_modules/lightningcss/node/index.js.flow
web/app/node_modules/lightningcss/node/index.mjs
web/app/node_modules/lightningcss/node/targets.d.ts
web/app/node_modules/lightningcss/node/targets.js.flow
web/app/node_modules/lightningcss/package.json
web/app/node_modules/lightningcss/README.md
web/app/node_modules/lightweight-charts/dist/lightweight-charts.development.mjs
web/app/node_modules/lightweight-charts/dist/lightweight-charts.production.mjs
web/app/node_modules/lightweight-charts/dist/lightweight-charts.standalone.development.js
web/app/node_modules/lightweight-charts/dist/lightweight-charts.standalone.development.mjs
web/app/node_modules/lightweight-charts/dist/lightweight-charts.standalone.production.js
web/app/node_modules/lightweight-charts/dist/lightweight-charts.standalone.production.mjs
web/app/node_modules/lightweight-charts/dist/typings.d.ts
web/app/node_modules/lightweight-charts/LICENSE
web/app/node_modules/lightweight-charts/package.json
web/app/node_modules/lightweight-charts/README.md
web/app/node_modules/loose-envify/cli.js
web/app/node_modules/loose-envify/custom.js
web/app/node_modules/loose-envify/index.js
web/app/node_modules/loose-envify/LICENSE
web/app/node_modules/loose-envify/loose-envify.js
web/app/node_modules/loose-envify/package.json
web/app/node_modules/loose-envify/README.md
web/app/node_modules/loose-envify/replace.js
web/app/node_modules/lru-cache/index.js
web/app/node_modules/lru-cache/LICENSE
web/app/node_modules/lru-cache/package.json
web/app/node_modules/lru-cache/README.md
web/app/node_modules/magic-string/dist/index.d.mts
web/app/node_modules/magic-string/dist/index.mjs
web/app/node_modules/magic-string/LICENSE
web/app/node_modules/magic-string/package.json
web/app/node_modules/magic-string/README.md
web/app/node_modules/ms/index.js
web/app/node_modules/ms/license.md
web/app/node_modules/ms/package.json
web/app/node_modules/ms/readme.md
web/app/node_modules/nanoid/async/index.browser.cjs
web/app/node_modules/nanoid/async/index.browser.js
web/app/node_modules/nanoid/async/index.cjs
web/app/node_modules/nanoid/async/index.d.ts
web/app/node_modules/nanoid/async/index.js
web/app/node_modules/nanoid/async/index.native.js
web/app/node_modules/nanoid/async/package.json
web/app/node_modules/nanoid/bin/nanoid.cjs
web/app/node_modules/nanoid/index.browser.cjs
web/app/node_modules/nanoid/index.browser.js
web/app/node_modules/nanoid/index.cjs
web/app/node_modules/nanoid/index.d.cts
web/app/node_modules/nanoid/index.d.ts
web/app/node_modules/nanoid/index.js
web/app/node_modules/nanoid/LICENSE
web/app/node_modules/nanoid/nanoid.js
web/app/node_modules/nanoid/non-secure/index.cjs
web/app/node_modules/nanoid/non-secure/index.d.ts
web/app/node_modules/nanoid/non-secure/index.js
web/app/node_modules/nanoid/non-secure/package.json
web/app/node_modules/nanoid/package.json
web/app/node_modules/nanoid/README.md
web/app/node_modules/nanoid/url-alphabet/index.cjs
web/app/node_modules/nanoid/url-alphabet/index.js
web/app/node_modules/nanoid/url-alphabet/package.json
web/app/node_modules/node-releases/LICENSE
web/app/node_modules/node-releases/package.json
web/app/node_modules/node-releases/README.md
web/app/node_modules/obug/dist/ansi.d.ts
web/app/node_modules/obug/dist/ansi.js
web/app/node_modules/obug/dist/browser.d.ts
web/app/node_modules/obug/dist/browser.js
web/app/node_modules/obug/dist/browser.min.js
web/app/node_modules/obug/dist/core.d.ts
web/app/node_modules/obug/dist/core.js
web/app/node_modules/obug/dist/plain.d.ts
web/app/node_modules/obug/dist/plain.js
web/app/node_modules/obug/LICENSE
web/app/node_modules/obug/package.json
web/app/node_modules/obug/README.md
web/app/node_modules/@oxc-project/types/LICENSE
web/app/node_modules/@oxc-project/types/package.json
web/app/node_modules/@oxc-project/types/README.md
web/app/node_modules/@oxc-project/types/types.d.ts
web/app/node_modules/.package-lock.json
web/app/node_modules/picocolors/LICENSE
web/app/node_modules/picocolors/package.json
web/app/node_modules/picocolors/picocolors.browser.js
web/app/node_modules/picocolors/picocolors.d.ts
web/app/node_modules/picocolors/picocolors.js
web/app/node_modules/picocolors/README.md
web/app/node_modules/picocolors/types.d.ts
web/app/node_modules/picomatch/index.js
web/app/node_modules/picomatch/lib/constants.js
web/app/node_modules/picomatch/lib/parse.js
web/app/node_modules/picomatch/lib/picomatch.js
web/app/node_modules/picomatch/lib/scan.js
web/app/node_modules/picomatch/lib/utils.js
web/app/node_modules/picomatch/LICENSE
web/app/node_modules/picomatch/package.json
web/app/node_modules/picomatch/posix.js
web/app/node_modules/picomatch/README.md
web/app/node_modules/postcss/lib/at-rule.d.ts
web/app/node_modules/postcss/lib/at-rule.js
web/app/node_modules/postcss/lib/comment.d.ts
web/app/node_modules/postcss/lib/comment.js
web/app/node_modules/postcss/lib/container.d.ts
web/app/node_modules/postcss/lib/container.js
web/app/node_modules/postcss/lib/css-syntax-error.d.ts
web/app/node_modules/postcss/lib/css-syntax-error.js
web/app/node_modules/postcss/lib/declaration.d.ts
web/app/node_modules/postcss/lib/declaration.js
web/app/node_modules/postcss/lib/document.d.ts
web/app/node_modules/postcss/lib/document.js
web/app/node_modules/postcss/lib/fromJSON.d.ts
web/app/node_modules/postcss/lib/fromJSON.js
web/app/node_modules/postcss/lib/input.d.ts
web/app/node_modules/postcss/lib/input.js
web/app/node_modules/postcss/lib/lazy-result.d.ts
web/app/node_modules/postcss/lib/lazy-result.js
web/app/node_modules/postcss/lib/list.d.ts
web/app/node_modules/postcss/lib/list.js
web/app/node_modules/postcss/lib/map-generator.js
web/app/node_modules/postcss/lib/node.d.ts
web/app/node_modules/postcss/lib/node.js
web/app/node_modules/postcss/lib/no-work-result.d.ts
web/app/node_modules/postcss/lib/no-work-result.js
web/app/node_modules/postcss/lib/parse.d.ts
web/app/node_modules/postcss/lib/parse.js
web/app/node_modules/postcss/lib/parser.js
web/app/node_modules/postcss/lib/postcss.d.mts
web/app/node_modules/postcss/lib/postcss.d.ts
web/app/node_modules/postcss/lib/postcss.js
web/app/node_modules/postcss/lib/postcss.mjs
web/app/node_modules/postcss/lib/previous-map.d.ts
web/app/node_modules/postcss/lib/previous-map.js
web/app/node_modules/postcss/lib/processor.d.ts
web/app/node_modules/postcss/lib/processor.js
web/app/node_modules/postcss/lib/result.d.ts
web/app/node_modules/postcss/lib/result.js
web/app/node_modules/postcss/lib/root.d.ts
web/app/node_modules/postcss/lib/root.js
web/app/node_modules/postcss/lib/rule.d.ts
web/app/node_modules/postcss/lib/rule.js
web/app/node_modules/postcss/lib/stringifier.d.ts
web/app/node_modules/postcss/lib/stringifier.js
web/app/node_modules/postcss/lib/stringify.d.ts
web/app/node_modules/postcss/lib/stringify.js
web/app/node_modules/postcss/lib/symbols.js
web/app/node_modules/postcss/lib/terminal-highlight.js
web/app/node_modules/postcss/lib/tokenize.js
web/app/node_modules/postcss/lib/warning.d.ts
web/app/node_modules/postcss/lib/warning.js
web/app/node_modules/postcss/lib/warn-once.js
web/app/node_modules/postcss/LICENSE
web/app/node_modules/postcss/package.json
web/app/node_modules/postcss/README.md
web/app/node_modules/react/cjs/react.development.js
web/app/node_modules/react/cjs/react-jsx-dev-runtime.development.js
web/app/node_modules/react/cjs/react-jsx-dev-runtime.production.min.js
web/app/node_modules/react/cjs/react-jsx-dev-runtime.profiling.min.js
web/app/node_modules/react/cjs/react-jsx-runtime.development.js
web/app/node_modules/react/cjs/react-jsx-runtime.production.min.js
web/app/node_modules/react/cjs/react-jsx-runtime.profiling.min.js
web/app/node_modules/react/cjs/react.production.min.js
web/app/node_modules/react/cjs/react.shared-subset.development.js
web/app/node_modules/react/cjs/react.shared-subset.production.min.js
web/app/node_modules/react-dom/cjs/react-dom.development.js
web/app/node_modules/react-dom/cjs/react-dom.production.min.js
web/app/node_modules/react-dom/cjs/react-dom.profiling.min.js
web/app/node_modules/react-dom/cjs/react-dom-server.browser.development.js
web/app/node_modules/react-dom/cjs/react-dom-server.browser.production.min.js
web/app/node_modules/react-dom/cjs/react-dom-server-legacy.browser.development.js
web/app/node_modules/react-dom/cjs/react-dom-server-legacy.browser.production.min.js
web/app/node_modules/react-dom/cjs/react-dom-server-legacy.node.development.js
web/app/node_modules/react-dom/cjs/react-dom-server-legacy.node.production.min.js
web/app/node_modules/react-dom/cjs/react-dom-server.node.development.js
web/app/node_modules/react-dom/cjs/react-dom-server.node.production.min.js
web/app/node_modules/react-dom/cjs/react-dom-test-utils.development.js
web/app/node_modules/react-dom/cjs/react-dom-test-utils.production.min.js
web/app/node_modules/react-dom/client.js
web/app/node_modules/react-dom/index.js
web/app/node_modules/react-dom/LICENSE
web/app/node_modules/react-dom/package.json
web/app/node_modules/react-dom/profiling.js
web/app/node_modules/react-dom/README.md
web/app/node_modules/react-dom/server.browser.js
web/app/node_modules/react-dom/server.js
web/app/node_modules/react-dom/server.node.js
web/app/node_modules/react-dom/test-utils.js
web/app/node_modules/react-dom/umd/react-dom.development.js
web/app/node_modules/react-dom/umd/react-dom.production.min.js
web/app/node_modules/react-dom/umd/react-dom.profiling.min.js
web/app/node_modules/react-dom/umd/react-dom-server.browser.development.js
web/app/node_modules/react-dom/umd/react-dom-server.browser.production.min.js
web/app/node_modules/react-dom/umd/react-dom-server-legacy.browser.development.js
web/app/node_modules/react-dom/umd/react-dom-server-legacy.browser.production.min.js
web/app/node_modules/react-dom/umd/react-dom-test-utils.development.js
web/app/node_modules/react-dom/umd/react-dom-test-utils.production.min.js
web/app/node_modules/react/index.js
web/app/node_modules/react/jsx-dev-runtime.js
web/app/node_modules/react/jsx-runtime.js
web/app/node_modules/react/LICENSE
web/app/node_modules/react/package.json
web/app/node_modules/react/react.shared-subset.js
web/app/node_modules/react/README.md
web/app/node_modules/react-refresh/babel.js
web/app/node_modules/react-refresh/cjs/react-refresh-babel.development.js
web/app/node_modules/react-refresh/cjs/react-refresh-babel.production.js
web/app/node_modules/react-refresh/cjs/react-refresh-runtime.development.js
web/app/node_modules/react-refresh/cjs/react-refresh-runtime.production.js
web/app/node_modules/react-refresh/LICENSE
web/app/node_modules/react-refresh/package.json
web/app/node_modules/react-refresh/README.md
web/app/node_modules/react-refresh/runtime.js
web/app/node_modules/react/umd/react.development.js
web/app/node_modules/react/umd/react.production.min.js
web/app/node_modules/react/umd/react.profiling.min.js
web/app/node_modules/rolldown/bin/cli.mjs
web/app/node_modules/@rolldown/binding-linux-x64-gnu/package.json
web/app/node_modules/@rolldown/binding-linux-x64-gnu/README.md
web/app/node_modules/@rolldown/binding-linux-x64-gnu/rolldown-binding.linux-x64-gnu.node
web/app/node_modules/rolldown/dist/cli.d.mts
web/app/node_modules/rolldown/dist/cli.mjs
web/app/node_modules/rolldown/dist/config.d.mts
web/app/node_modules/rolldown/dist/config.mjs
web/app/node_modules/rolldown/dist/experimental-index.d.mts
web/app/node_modules/rolldown/dist/experimental-index.mjs
web/app/node_modules/rolldown/dist/experimental-runtime.d.ts
web/app/node_modules/rolldown/dist/experimental-runtime.mjs
web/app/node_modules/rolldown/dist/experimental-runtime-types.d.ts
web/app/node_modules/rolldown/dist/filter-index.d.mts
web/app/node_modules/rolldown/dist/filter-index.mjs
web/app/node_modules/rolldown/dist/get-log-filter.d.mts
web/app/node_modules/rolldown/dist/get-log-filter.mjs
web/app/node_modules/rolldown/dist/index.d.mts
web/app/node_modules/rolldown/dist/index.mjs
web/app/node_modules/rolldown/dist/parallel-plugin.d.mts
web/app/node_modules/rolldown/dist/parallel-plugin.mjs
web/app/node_modules/rolldown/dist/parallel-plugin-worker.d.mts
web/app/node_modules/rolldown/dist/parallel-plugin-worker.mjs
web/app/node_modules/rolldown/dist/parse-ast-index.d.mts
web/app/node_modules/rolldown/dist/parse-ast-index.mjs
web/app/node_modules/rolldown/dist/plugins-index.d.mts
web/app/node_modules/rolldown/dist/plugins-index.mjs
web/app/node_modules/rolldown/dist/utils-index.d.mts
web/app/node_modules/rolldown/dist/utils-index.mjs
web/app/node_modules/rolldown/LICENSE
web/app/node_modules/rolldown/package.json
web/app/node_modules/@rolldown/pluginutils/LICENSE
web/app/node_modules/@rolldown/pluginutils/package.json
web/app/node_modules/@rolldown/pluginutils/README.md
web/app/node_modules/rolldown/README.md
web/app/node_modules/rolldown/THIRD-PARTY-LICENSE
web/app/node_modules/scheduler/cjs/scheduler.development.js
web/app/node_modules/scheduler/cjs/scheduler.production.min.js
web/app/node_modules/scheduler/cjs/scheduler-unstable_mock.development.js
web/app/node_modules/scheduler/cjs/scheduler-unstable_mock.production.min.js
web/app/node_modules/scheduler/cjs/scheduler-unstable_post_task.development.js
web/app/node_modules/scheduler/cjs/scheduler-unstable_post_task.production.min.js
web/app/node_modules/scheduler/index.js
web/app/node_modules/scheduler/LICENSE
web/app/node_modules/scheduler/package.json
web/app/node_modules/scheduler/README.md
web/app/node_modules/scheduler/umd/scheduler.development.js
web/app/node_modules/scheduler/umd/scheduler.production.min.js
web/app/node_modules/scheduler/umd/scheduler.profiling.min.js
web/app/node_modules/scheduler/umd/scheduler-unstable_mock.development.js
web/app/node_modules/scheduler/umd/scheduler-unstable_mock.production.min.js
web/app/node_modules/scheduler/unstable_mock.js
web/app/node_modules/scheduler/unstable_post_task.js
web/app/node_modules/semver/bin/semver.js
web/app/node_modules/semver/LICENSE
web/app/node_modules/semver/package.json
web/app/node_modules/semver/range.bnf
web/app/node_modules/semver/README.md
web/app/node_modules/semver/semver.js
web/app/node_modules/source-map-js/lib/array-set.js
web/app/node_modules/source-map-js/lib/base64.js
web/app/node_modules/source-map-js/lib/base64-vlq.js
web/app/node_modules/source-map-js/lib/binary-search.js
web/app/node_modules/source-map-js/lib/mapping-list.js
web/app/node_modules/source-map-js/lib/quick-sort.js
web/app/node_modules/source-map-js/lib/source-map-consumer.d.ts
web/app/node_modules/source-map-js/lib/source-map-consumer.js
web/app/node_modules/source-map-js/lib/source-map-generator.d.ts
web/app/node_modules/source-map-js/lib/source-map-generator.js
web/app/node_modules/source-map-js/lib/source-node.d.ts
web/app/node_modules/source-map-js/lib/source-node.js
web/app/node_modules/source-map-js/lib/util.js
web/app/node_modules/source-map-js/LICENSE
web/app/node_modules/source-map-js/package.json
web/app/node_modules/source-map-js/README.md
web/app/node_modules/source-map-js/source-map.d.ts
web/app/node_modules/source-map-js/source-map.js
web/app/node_modules/std-env/dist/index.d.mts
web/app/node_modules/std-env/dist/index.mjs
web/app/node_modules/std-env/LICENCE
web/app/node_modules/std-env/package.json
web/app/node_modules/std-env/README.md
web/app/node_modules/@tauri-apps/api/app.cjs
web/app/node_modules/@tauri-apps/api/app.d.ts
web/app/node_modules/@tauri-apps/api/app.js
web/app/node_modules/@tauri-apps/api/CHANGELOG.md
web/app/node_modules/@tauri-apps/api/cli.cjs
web/app/node_modules/@tauri-apps/api/cli.d.ts
web/app/node_modules/@tauri-apps/api/cli.js
web/app/node_modules/@tauri-apps/api/clipboard.cjs
web/app/node_modules/@tauri-apps/api/clipboard.d.ts
web/app/node_modules/@tauri-apps/api/clipboard.js
web/app/node_modules/@tauri-apps/api/dialog.cjs
web/app/node_modules/@tauri-apps/api/dialog.d.ts
web/app/node_modules/@tauri-apps/api/dialog.js
web/app/node_modules/@tauri-apps/api/event.cjs
web/app/node_modules/@tauri-apps/api/event.d.ts
web/app/node_modules/@tauri-apps/api/event.js
web/app/node_modules/@tauri-apps/api/fs.cjs
web/app/node_modules/@tauri-apps/api/fs.d.ts
web/app/node_modules/@tauri-apps/api/fs.js
web/app/node_modules/@tauri-apps/api/globalShortcut.cjs
web/app/node_modules/@tauri-apps/api/globalShortcut.d.ts
web/app/node_modules/@tauri-apps/api/globalShortcut.js
web/app/node_modules/@tauri-apps/api/http.cjs
web/app/node_modules/@tauri-apps/api/http.d.ts
web/app/node_modules/@tauri-apps/api/http.js
web/app/node_modules/@tauri-apps/api/index.cjs
web/app/node_modules/@tauri-apps/api/index.d.ts
web/app/node_modules/@tauri-apps/api/index.js
web/app/node_modules/@tauri-apps/api/LICENSE_APACHE-2.0
web/app/node_modules/@tauri-apps/api/LICENSE_MIT
web/app/node_modules/@tauri-apps/api/mocks.cjs
web/app/node_modules/@tauri-apps/api/mocks.d.ts
web/app/node_modules/@tauri-apps/api/mocks.js
web/app/node_modules/@tauri-apps/api/notification.cjs
web/app/node_modules/@tauri-apps/api/notification.d.ts
web/app/node_modules/@tauri-apps/api/notification.js
web/app/node_modules/@tauri-apps/api/os.cjs
web/app/node_modules/@tauri-apps/api/os.d.ts
web/app/node_modules/@tauri-apps/api/os.js
web/app/node_modules/@tauri-apps/api/package.json
web/app/node_modules/@tauri-apps/api/path.cjs
web/app/node_modules/@tauri-apps/api/path.d.ts
web/app/node_modules/@tauri-apps/api/path.js
web/app/node_modules/@tauri-apps/api/process.cjs
web/app/node_modules/@tauri-apps/api/process.d.ts
web/app/node_modules/@tauri-apps/api/process.js
web/app/node_modules/@tauri-apps/api/README.md
web/app/node_modules/@tauri-apps/api/shell.cjs
web/app/node_modules/@tauri-apps/api/shell.d.ts
web/app/node_modules/@tauri-apps/api/shell.js
web/app/node_modules/@tauri-apps/api/tauri.cjs
web/app/node_modules/@tauri-apps/api/tauri.d.ts
web/app/node_modules/@tauri-apps/api/tauri.js
web/app/node_modules/@tauri-apps/api/updater.cjs
web/app/node_modules/@tauri-apps/api/updater.d.ts
web/app/node_modules/@tauri-apps/api/updater.js
web/app/node_modules/@tauri-apps/api/window.cjs
web/app/node_modules/@tauri-apps/api/window.d.ts
web/app/node_modules/@tauri-apps/api/window.js
web/app/node_modules/tinybench/dist/index.d.ts
web/app/node_modules/tinybench/dist/index.js
web/app/node_modules/tinybench/LICENSE
web/app/node_modules/tinybench/package.json
web/app/node_modules/tinybench/README.md
web/app/node_modules/tinyexec/dist/main.d.mts
web/app/node_modules/tinyexec/dist/main.mjs
web/app/node_modules/tinyexec/LICENSE
web/app/node_modules/tinyexec/package.json
web/app/node_modules/tinyexec/README.md
web/app/node_modules/tinyglobby/dist/index.cjs
web/app/node_modules/tinyglobby/dist/index.d.cts
web/app/node_modules/tinyglobby/dist/index.d.mts
web/app/node_modules/tinyglobby/dist/index.mjs
web/app/node_modules/tinyglobby/LICENSE
web/app/node_modules/tinyglobby/package.json
web/app/node_modules/tinyglobby/README.md
web/app/node_modules/@types/babel__core/index.d.ts
web/app/node_modules/@types/babel__core/LICENSE
web/app/node_modules/@types/babel__core/package.json
web/app/node_modules/@types/babel__core/README.md
web/app/node_modules/@types/babel__generator/index.d.ts
web/app/node_modules/@types/babel__generator/LICENSE
web/app/node_modules/@types/babel__generator/package.json
web/app/node_modules/@types/babel__generator/README.md
web/app/node_modules/@types/babel__template/index.d.ts
web/app/node_modules/@types/babel__template/LICENSE
web/app/node_modules/@types/babel__template/package.json
web/app/node_modules/@types/babel__template/README.md
web/app/node_modules/@types/babel__traverse/index.d.ts
web/app/node_modules/@types/babel__traverse/LICENSE
web/app/node_modules/@types/babel__traverse/package.json
web/app/node_modules/@types/babel__traverse/README.md
web/app/node_modules/@types/chai/index.d.ts
web/app/node_modules/@types/chai/LICENSE
web/app/node_modules/@types/chai/package.json
web/app/node_modules/@types/chai/README.md
web/app/node_modules/@types/chai/register-should.d.ts
web/app/node_modules/typescript/bin/tsc
web/app/node_modules/typescript/bin/tsserver
web/app/node_modules/typescript/lib/cancellationToken.js
web/app/node_modules/typescript/lib/lib.decorators.d.ts
web/app/node_modules/typescript/lib/lib.decorators.legacy.d.ts
web/app/node_modules/typescript/lib/lib.dom.asynciterable.d.ts
web/app/node_modules/typescript/lib/lib.dom.d.ts
web/app/node_modules/typescript/lib/lib.dom.iterable.d.ts
web/app/node_modules/typescript/lib/lib.d.ts
web/app/node_modules/typescript/lib/lib.es2015.collection.d.ts
web/app/node_modules/typescript/lib/lib.es2015.core.d.ts
web/app/node_modules/typescript/lib/lib.es2015.d.ts
web/app/node_modules/typescript/lib/lib.es2015.generator.d.ts
web/app/node_modules/typescript/lib/lib.es2015.iterable.d.ts
web/app/node_modules/typescript/lib/lib.es2015.promise.d.ts
web/app/node_modules/typescript/lib/lib.es2015.proxy.d.ts
web/app/node_modules/typescript/lib/lib.es2015.reflect.d.ts
web/app/node_modules/typescript/lib/lib.es2015.symbol.d.ts
web/app/node_modules/typescript/lib/lib.es2015.symbol.wellknown.d.ts
web/app/node_modules/typescript/lib/lib.es2016.array.include.d.ts
web/app/node_modules/typescript/lib/lib.es2016.d.ts
web/app/node_modules/typescript/lib/lib.es2016.full.d.ts
web/app/node_modules/typescript/lib/lib.es2016.intl.d.ts
web/app/node_modules/typescript/lib/lib.es2017.date.d.ts
web/app/node_modules/typescript/lib/lib.es2017.d.ts
web/app/node_modules/typescript/lib/lib.es2017.full.d.ts
web/app/node_modules/typescript/lib/lib.es2017.intl.d.ts
web/app/node_modules/typescript/lib/lib.es2017.object.d.ts
web/app/node_modules/typescript/lib/lib.es2017.sharedmemory.d.ts
web/app/node_modules/typescript/lib/lib.es2017.string.d.ts
web/app/node_modules/typescript/lib/lib.es2017.typedarrays.d.ts
web/app/node_modules/typescript/lib/lib.es2018.asyncgenerator.d.ts
web/app/node_modules/typescript/lib/lib.es2018.asynciterable.d.ts
web/app/node_modules/typescript/lib/lib.es2018.d.ts
web/app/node_modules/typescript/lib/lib.es2018.full.d.ts
web/app/node_modules/typescript/lib/lib.es2018.intl.d.ts
web/app/node_modules/typescript/lib/lib.es2018.promise.d.ts
web/app/node_modules/typescript/lib/lib.es2018.regexp.d.ts
web/app/node_modules/typescript/lib/lib.es2019.array.d.ts
web/app/node_modules/typescript/lib/lib.es2019.d.ts
web/app/node_modules/typescript/lib/lib.es2019.full.d.ts
web/app/node_modules/typescript/lib/lib.es2019.intl.d.ts
web/app/node_modules/typescript/lib/lib.es2019.object.d.ts
web/app/node_modules/typescript/lib/lib.es2019.string.d.ts
web/app/node_modules/typescript/lib/lib.es2019.symbol.d.ts
web/app/node_modules/typescript/lib/lib.es2020.bigint.d.ts
web/app/node_modules/typescript/lib/lib.es2020.date.d.ts
web/app/node_modules/typescript/lib/lib.es2020.d.ts
web/app/node_modules/typescript/lib/lib.es2020.full.d.ts
web/app/node_modules/typescript/lib/lib.es2020.intl.d.ts
web/app/node_modules/typescript/lib/lib.es2020.number.d.ts
web/app/node_modules/typescript/lib/lib.es2020.promise.d.ts
web/app/node_modules/typescript/lib/lib.es2020.sharedmemory.d.ts
web/app/node_modules/typescript/lib/lib.es2020.string.d.ts
web/app/node_modules/typescript/lib/lib.es2020.symbol.wellknown.d.ts
web/app/node_modules/typescript/lib/lib.es2021.d.ts
web/app/node_modules/typescript/lib/lib.es2021.full.d.ts
web/app/node_modules/typescript/lib/lib.es2021.intl.d.ts
web/app/node_modules/typescript/lib/lib.es2021.promise.d.ts
web/app/node_modules/typescript/lib/lib.es2021.string.d.ts
web/app/node_modules/typescript/lib/lib.es2021.weakref.d.ts
web/app/node_modules/typescript/lib/lib.es2022.array.d.ts
web/app/node_modules/typescript/lib/lib.es2022.d.ts
web/app/node_modules/typescript/lib/lib.es2022.error.d.ts
web/app/node_modules/typescript/lib/lib.es2022.full.d.ts
web/app/node_modules/typescript/lib/lib.es2022.intl.d.ts
web/app/node_modules/typescript/lib/lib.es2022.object.d.ts
web/app/node_modules/typescript/lib/lib.es2022.regexp.d.ts
web/app/node_modules/typescript/lib/lib.es2022.sharedmemory.d.ts
web/app/node_modules/typescript/lib/lib.es2022.string.d.ts
web/app/node_modules/typescript/lib/lib.es2023.array.d.ts
web/app/node_modules/typescript/lib/lib.es2023.collection.d.ts
web/app/node_modules/typescript/lib/lib.es2023.d.ts
web/app/node_modules/typescript/lib/lib.es2023.full.d.ts
web/app/node_modules/typescript/lib/lib.es2023.intl.d.ts
web/app/node_modules/typescript/lib/lib.es5.d.ts
web/app/node_modules/typescript/lib/lib.es6.d.ts
web/app/node_modules/typescript/lib/lib.esnext.array.d.ts
web/app/node_modules/typescript/lib/lib.esnext.collection.d.ts
web/app/node_modules/typescript/lib/lib.esnext.decorators.d.ts
web/app/node_modules/typescript/lib/lib.esnext.disposable.d.ts
web/app/node_modules/typescript/lib/lib.esnext.d.ts
web/app/node_modules/typescript/lib/lib.esnext.full.d.ts
web/app/node_modules/typescript/lib/lib.esnext.intl.d.ts
web/app/node_modules/typescript/lib/lib.esnext.iterator.d.ts
web/app/node_modules/typescript/lib/lib.esnext.object.d.ts
web/app/node_modules/typescript/lib/lib.esnext.promise.d.ts
web/app/node_modules/typescript/lib/lib.esnext.regexp.d.ts
web/app/node_modules/typescript/lib/lib.esnext.string.d.ts
web/app/node_modules/typescript/lib/lib.scripthost.d.ts
web/app/node_modules/typescript/lib/lib.webworker.asynciterable.d.ts
web/app/node_modules/typescript/lib/lib.webworker.d.ts
web/app/node_modules/typescript/lib/lib.webworker.importscripts.d.ts
web/app/node_modules/typescript/lib/lib.webworker.iterable.d.ts
web/app/node_modules/typescript/lib/tsc.js
web/app/node_modules/typescript/lib/tsserver.js
web/app/node_modules/typescript/lib/tsserverlibrary.d.ts
web/app/node_modules/typescript/lib/tsserverlibrary.js
web/app/node_modules/typescript/lib/typescript.d.ts
web/app/node_modules/typescript/lib/typescript.js
web/app/node_modules/typescript/lib/typesMap.json
web/app/node_modules/typescript/lib/typingsInstaller.js
web/app/node_modules/typescript/lib/watchGuard.js
web/app/node_modules/typescript/LICENSE.txt
web/app/node_modules/typescript/package.json
web/app/node_modules/typescript/README.md
web/app/node_modules/typescript/SECURITY.md
web/app/node_modules/typescript/ThirdPartyNoticeText.txt
web/app/node_modules/@types/deep-eql/index.d.ts
web/app/node_modules/@types/deep-eql/LICENSE
web/app/node_modules/@types/deep-eql/package.json
web/app/node_modules/@types/deep-eql/README.md
web/app/node_modules/@types/estree/flow.d.ts
web/app/node_modules/@types/estree/index.d.ts
web/app/node_modules/@types/estree/LICENSE
web/app/node_modules/@types/estree/package.json
web/app/node_modules/@types/estree/README.md
web/app/node_modules/@types/prop-types/index.d.ts
web/app/node_modules/@types/prop-types/LICENSE
web/app/node_modules/@types/prop-types/package.json
web/app/node_modules/@types/prop-types/README.md
web/app/node_modules/@types/react/canary.d.ts
web/app/node_modules/@types/react-dom/canary.d.ts
web/app/node_modules/@types/react-dom/client.d.ts
web/app/node_modules/@types/react-dom/experimental.d.ts
web/app/node_modules/@types/react-dom/index.d.ts
web/app/node_modules/@types/react-dom/LICENSE
web/app/node_modules/@types/react-dom/package.json
web/app/node_modules/@types/react-dom/README.md
web/app/node_modules/@types/react-dom/server.d.ts
web/app/node_modules/@types/react/experimental.d.ts
web/app/node_modules/@types/react/global.d.ts
web/app/node_modules/@types/react/index.d.ts
web/app/node_modules/@types/react/jsx-dev-runtime.d.ts
web/app/node_modules/@types/react/jsx-runtime.d.ts
web/app/node_modules/@types/react/LICENSE
web/app/node_modules/@types/react/package.json
web/app/node_modules/@types/react/README.md
web/app/node_modules/update-browserslist-db/check-npm-version.js
web/app/node_modules/update-browserslist-db/cli.js
web/app/node_modules/update-browserslist-db/index.d.ts
web/app/node_modules/update-browserslist-db/index.js
web/app/node_modules/update-browserslist-db/LICENSE
web/app/node_modules/update-browserslist-db/package.json
web/app/node_modules/update-browserslist-db/README.md
web/app/node_modules/update-browserslist-db/utils.js
web/app/node_modules/vite/bin/openChrome.js
web/app/node_modules/vite/bin/vite.js
web/app/node_modules/vite/client.d.ts
web/app/node_modules/@vitejs/plugin-react/LICENSE
web/app/node_modules/@vitejs/plugin-react/package.json
web/app/node_modules/@vitejs/plugin-react/README.md
web/app/node_modules/vite/LICENSE.md
web/app/node_modules/vite/misc/false.js
web/app/node_modules/vite/misc/true.js
web/app/node_modules/vite/package.json
web/app/node_modules/vite/README.md
web/app/node_modules/vitest/browser/context.d.ts
web/app/node_modules/vitest/browser/context.js
web/app/node_modules/vitest/config.d.ts
web/app/node_modules/vitest/coverage.d.ts
web/app/node_modules/vitest/dist/browser.d.ts
web/app/node_modules/vitest/dist/browser.js
web/app/node_modules/vitest/dist/cli.js
web/app/node_modules/vitest/dist/config.cjs
web/app/node_modules/vitest/dist/config.d.ts
web/app/node_modules/vitest/dist/config.js
web/app/node_modules/vitest/dist/index.d.ts
web/app/node_modules/vitest/dist/index.js
web/app/node_modules/vitest/dist/module-evaluator.d.ts
web/app/node_modules/vitest/dist/module-evaluator.js
web/app/node_modules/vitest/dist/node.d.ts
web/app/node_modules/vitest/dist/node.js
web/app/node_modules/vitest/dist/nodejs-worker-loader.js
web/app/node_modules/vitest/dist/path.js
web/app/node_modules/vitest/dist/runtime.d.ts
web/app/node_modules/vitest/dist/runtime.js
web/app/node_modules/vitest/dist/spy.js
web/app/node_modules/vitest/dist/task-utils.js
web/app/node_modules/vitest/dist/traces.js
web/app/node_modules/vitest/dist/worker.d.ts
web/app/node_modules/vitest/dist/worker.js
web/app/node_modules/vitest/environments.d.ts
web/app/node_modules/vitest/globals.d.ts
web/app/node_modules/vitest/import-meta.d.ts
web/app/node_modules/vitest/importMeta.d.ts
web/app/node_modules/vitest/index.cjs
web/app/node_modules/vitest/index.d.cts
web/app/node_modules/vitest/jsdom.d.ts
web/app/node_modules/vitest/LICENSE.md
web/app/node_modules/vitest/mocker.d.ts
web/app/node_modules/@vitest/mocker/LICENSE
web/app/node_modules/@vitest/mocker/package.json
web/app/node_modules/@vitest/mocker/README.md
web/app/node_modules/vitest/node.d.ts
web/app/node_modules/vitest/optional-runtime-types.d.ts
web/app/node_modules/vitest/optional-types.d.ts
web/app/node_modules/vitest/package.json
web/app/node_modules/vitest/README.md
web/app/node_modules/vitest/reporters.d.ts
web/app/node_modules/vitest/runners.d.ts
web/app/node_modules/vitest/snapshot.d.ts
web/app/node_modules/@vitest/spy/LICENSE
web/app/node_modules/@vitest/spy/optional-types.d.ts
web/app/node_modules/@vitest/spy/package.json
web/app/node_modules/@vitest/spy/README.md
web/app/node_modules/vitest/suite.d.ts
web/app/node_modules/vitest/suppress-warnings.cjs
web/app/node_modules/vitest/vitest.mjs
web/app/node_modules/vitest/worker.d.ts
web/app/node_modules/vite/types/customEvent.d.ts
web/app/node_modules/vite/types/hmrPayload.d.ts
web/app/node_modules/vite/types/hot.d.ts
web/app/node_modules/vite/types/importGlob.d.ts
web/app/node_modules/vite/types/import-meta.d.ts
web/app/node_modules/vite/types/importMeta.d.ts
web/app/node_modules/vite/types/metadata.d.ts
web/app/node_modules/why-is-node-running/cli.js
web/app/node_modules/why-is-node-running/include.js
web/app/node_modules/why-is-node-running/index.d.ts
web/app/node_modules/why-is-node-running/index.js
web/app/node_modules/why-is-node-running/LICENSE
web/app/node_modules/why-is-node-running/package.json
web/app/node_modules/why-is-node-running/README.md
web/app/node_modules/yallist/iterator.js
web/app/node_modules/yallist/LICENSE
web/app/node_modules/yallist/package.json
web/app/node_modules/yallist/README.md
web/app/node_modules/yallist/yallist.js
web/app/package.json
web/app/package-lock.json
web/app/public/.gitkeep
web/app/public/icon.svg
web/app/public/manifest.webmanifest
web/app/public/sw.js
web/app/public/vendor/duckdb/duckdb-browser.mjs
web/app/public/vendor/duckdb/duckdb-browser-mvp.worker.js
web/app/public/vendor/duckdb/duckdb-mvp.wasm
web/app/src/App.tsx
web/app/src/components/DesktopPanel.tsx
web/app/src/components/IndicatorPanel.tsx
web/app/src/components/LiveQuoteBoard.tsx
web/app/src/components/PriceChart.tsx
web/app/src/components/PrivacyPanel.tsx
web/app/src/components/QuoteTable.tsx
web/app/src/components/ResultGrid.tsx
web/app/src/components/SqlWorkbench.tsx
web/app/src/components/ThemeToggle.tsx
web/app/src/components/WasmProbe.tsx
web/app/src/components/WorkspaceTabs.tsx
web/app/src/demoData.ts
web/app/src/hooks/useLiveQuotes.ts
web/app/src/hooks/useLocale.tsx
web/app/src/hooks/useWorkspaces.ts
web/app/src/lib/account.test.ts
web/app/src/lib/account.ts
web/app/src/lib/analytics.test.ts
web/app/src/lib/analytics.ts
web/app/src/lib/demoWalk.test.ts
web/app/src/lib/demoWalk.ts
web/app/src/lib/desktop.test.ts
web/app/src/lib/desktop.ts
web/app/src/lib/duckdb.test.ts
web/app/src/lib/duckdb.ts
web/app/src/lib/exportCsv.test.ts
web/app/src/lib/exportCsv.ts
web/app/src/lib/i18n.test.ts
web/app/src/lib/i18n.ts
web/app/src/lib/indicators.test.ts
web/app/src/lib/indicators.ts
web/app/src/lib/liveFeed.test.ts
web/app/src/lib/liveFeed.ts
web/app/src/lib/privacy.test.ts
web/app/src/lib/privacy.ts
web/app/src/lib/pwa.test.ts
web/app/src/lib/pwa.ts
web/app/src/lib/resultTable.test.ts
web/app/src/lib/resultTable.ts
web/app/src/lib/theme.test.ts
web/app/src/lib/theme.ts
web/app/src/lib/workspaces.test.ts
web/app/src/lib/workspaces.ts
web/app/src/main.tsx
web/app/src/styles.css
web/app/tsconfig.json
web/app/tsconfig.node.json
web/app/tsconfig.tsbuildinfo
web/app/vite.config.ts


[showing lines 110-1170 of 1170; full output saved to /home/cui/.local/share/opencode/shell/c1f3bd68d6c4792960bc4f7a64a58d38c0d0aac9/sh_14e52d13f001hDMDr1P9AjJn5C.out]
> build · gpt-6-luna
[0m→ [0mRead services/api-gateway/src/auth.rs [90m[limit=420][0m
[0m→ [0mRead docker-compose.yml [90m[limit=340][0m
[0m→ [0mRead services/api-gateway/src/main.rs [90m[offset=100, limit=220][0m
[0m→ [0mRead docs/auth.md [90m[limit=280][0m
[0m→ [0mRead .github/workflows/ci.yml [90m[limit=360][0m
> build · gpt-6-luna
[0m→ [0mRead docs/architecture-review.md [90m[limit=320][0m
[0m$ [0mrg -n '^\s*(#\[arg|auth_mode|rate_limit|bind:|fn build_router|async fn auth|async fn proxy|Route|route\(|nest\()' services/api-gateway/src/main.rs | head -150 && rg -n 'TODO|未实现|规划项|后续|归.*TODO|未接线|mock|演示数据|暂不' README.md README.zh.md docs TODO.md | head -220
19:    Router,
66:    #[arg(short, long, default_value = "0.0.0.0:8080")]
67:    bind: SocketAddr,
70:    #[arg(long, default_value = "http://127.0.0.1:8081")]
74:    #[arg(long, default_value = "http://127.0.0.1:8082")]
78:    #[arg(long, default_value = "http://127.0.0.1:8083")]
82:    #[arg(long, default_value = "")]
83:    rate_limit_url: String,
86:    #[arg(long, default_value_t = 120)]
87:    rate_limit_per_minute: u32,
90:    #[arg(long, default_value = "off")]
91:    auth_mode: String,
94:    #[arg(long, default_value = "")]
98:    #[arg(long, default_value = "")]
102:    #[arg(long, default_value = "")]
108:    #[arg(long, default_value = "")]
113:    #[arg(long, default_value = "600")]
118:    #[arg(long, default_value = "")]
122:    #[arg(long, default_value = "")]
126:    #[arg(long, default_value = "")]
131:    #[arg(long, default_value = "")]
135:    #[arg(short, long, default_value = "info")]
156:    rate_limiter: Option<RedisRateLimiter>,
158:    rate_limit_per_minute: u32,
241:        auth_mode == auth::AuthMode::JwtRequired,
274:        rate_limiter,
275:        rate_limit_per_minute: args.rate_limit_per_minute,
400:fn build_router(state: GatewayState) -> Router {
414:            rate_limit_middleware,
425:    Router::new()
615:        rate_limit_rejection(&decision)
625:async fn auth_middleware(
1364:            rate_limiter: None,
1365:            rate_limit_per_minute: 120,
TODO.md:1:# TODO
TODO.md:178:- [x] announcement/news 域维持未实现（无消费方证据）（2026-10-10）
README.zh.md:12:- 完全运行在自托管局域网环境；对外发布（如 Cloudflare Tunnel）为规划项，尚未配置。
README.zh.md:24:   - 当前在用流：`quotes.raw`、`quotes.normalized`、`quotes.dlq`。公告/新闻域尚未实现。
docs/distributed-tracing.md:12:| 入站带 `X-Trace-Id`（非空） | 原样沿用——跨服务/客户端发起的重试与后续请求串同链路 |
docs/mobile-privacy.md:72:- **L337 推送/告警**：门锁定期间通知仍可见（系统层），应用内告警面板未实现（随门隐藏语义登记）；
docs/mobile-privacy.md:76:  TODO 一起接，见 docs/mobile-offline.md §8⑦）；
docs/mobile-privacy.md:78:- **L512 后续（桌面/iOS）**：桌面 Tauri 无生物识别门（OS 锁屏承担），
docs/architecture.md:6:  通过 Cloudflare Tunnel 暴露给外部客户端为规划项，当前未配置。
docs/architecture.md:36:- 当前默认方案：Redis Streams（stream：`quotes.raw`/`quotes.normalized`/`news.raw`/`announcements.raw`/`quotes.dlq`；实际在用的是 quotes 三流，公告/新闻域未实现）。
docs/architecture.md:37:  （拍板 2026-10-10：维持未实现——无消费方证据，域登记不立项。）
docs/architecture.md:50:  资金流归集为规划项，无代码。
docs/architecture.md:51:- **批处理**：每日/每周任务（复权因子校准、行业分类同步）为规划项，无调度实现。
docs/architecture.md:57:  INSERT 写路径）；公告/新闻/舆情主仓与倒排/全文索引为规划项。
docs/architecture.md:76:- **安全**：Cloudflare Tunnel 暴露网关为规划项；内部服务仅限内网访问；TLS 缺省由
docs/architecture.md:100:  节点为规划项。
docs/architecture.md:103:## 后续路线
docs/architecture.md:108:4. 上线最小可用数据集（指数/主板行情 + 公告）——公告域未实现。
docs/analysis.md:4:> 并给出按价值排序的行动清单（见 §5，已同步到 TODO.md）。
docs/analysis.md:16:| services/api-gateway | 低 | **纯 stub**：health 返回硬编码健康状态，proxy 返回 mock，无真实转发/鉴权〔现注：已落真实反代 + JWT/OIDC 鉴权 + RBAC + Redis 限流 + 探测式 health（2026-09-28 起），本行为 2026-09-27 快照〕 |
docs/analysis.md:17:| wasm-analyzer + web | 中 | WASM 引擎 + 前端自洽（demo 数据 + Canvas），可配置直连 data-engine REST + real-time-feed WS（web/app.js restBase/wsUrl），缺省仍走演示数据 |
docs/analysis.md:57:## 5. 行动清单（按价值排序，已同步 TODO.md）
docs/analysis.md:73:### P2 数据落地（后续）
docs/analysis.md:79:### P3 补对外链路（后续）
docs/analysis.md:82:| 9 | api-gateway 真实反代 data-engine（8081）/real-time-feed（8082），去掉 mock | 集成测试经网关取 `/stocks/:symbol/history` 与 WS |
docs/architecture-review.md:180:| P0（立即，docs-only 可先行） | 定位声明落 README/PROJECT_SUMMARY；本文入文档站 | 文档收敛 | ✅ 已落（6968e57 + 后续对账批） |
docs/architecture-review.md:186:**不做清单在此文档 §2.4（Kafka/K8s/AI 预测/反爬核心化/iOS 主线）——后续评审以此为对齐锚。**
docs/auth.md:59:不经网关，不受此校验；feed 侧自身鉴权归 WS hardening 后续项（web 客户端
docs/auth.md:110:   Subscribe 均放行，通道级 ACL 归 WS hardening 后续项。
docs/testing.md:34:## 4. 边界与后续（诚实登记）
docs/theme-adaptation.md:21:   进程内热切换随设置页 TODO 一起接）。
docs/theme-adaptation.md:29:  默认 light/dark 配色板为骨架期配色，品牌色板归后续设计项（三端
docs/mobile-push-sync.md:34:| 远程推送 FCM / APNs | FCM 免费但需 Play services 与 Google 项目；APNs 需 Apple 证书与签名链 | **不引入**（付费/账号/签名依赖）；留作 `NotificationChannel` 的后续实现（§6 可插拔点） |
docs/mobile-push-sync.md:35:| 自建推送（长连接 + 服务端） | 本仓已有 real-time-feed WS 面，但移动端需自管心跳/重连 | 归后续 TODO；不属「推送通道」骨架范围 |
docs/mobile-push-sync.md:86:已在 L114 注明（移动端通知是平台能力，桌面归托盘/系统通知 TODO）。
docs/mobile-push-sync.md:100:  送达实现留真机 TODO）+ WorkManager 装配面（`PeriodicSyncWorker` + enqueue 帮助
docs/mobile-push-sync.md:101:  函数已实现，尚未在壳入口接线，归真机 TODO，映射见 §3）。
docs/mobile-push-sync.md:144:4. 远程通道（FCM/APNs）接入时的凭据链与可靠性——归后续 TODO，本轮不引入。
docs/mobile-push-sync.md:147:## 11. 后续 TODO 映射
docs/mobile-push-sync.md:149:| TODO 项 | 本文依托 |
docs/data-privacy.md:6:已落地，客户端尚未接线同步（web 端 account.ts 纯逻辑未接 UI/网络、Android
docs/data-privacy.md:52:  （隐私政策 §1.6），应用内导出/清除归后续项（登记）；
docs/data-privacy.md:62:归 L487）→ 通知监管机构与受影响数据主体 → 修复与复盘。当前客户端未接线
docs/desktop-release.md:21:## 2. 签名与公证（登记，未实现）
docs/user-guide.md:36:服务端只存公开行情；账户同步面已落地但客户端未接线前不存个人数据（见 `docs/data-privacy.md`）。
docs/user-guide.md:48:- 通知为真机 TODO，无应用内设置面；opt-in 语义待壳层接线。
docs/user-guide.md:53:录制管线（录屏 → 剪辑 → 托管 → 站内嵌入）登记为后续增量，不预写占位。
docs/intro.md:14:  后端不可达回退演示数据并在状态栏注明，断线出复位按钮可重连。
docs/realtime-sync-protocol.md:3:> 对应 TODO「实现实时数据同步协议（WebSocket 增量更新 + 版本控制）」最小可用版本。
docs/realtime-sync-protocol.md:38:已知最小口径边界（刻意简化，扩展留后续立项）：不做嵌套（深度 >1）递归 diff、
docs/realtime-sync-protocol.md:60:* 发布：`next_versioned_frame` 在锁内 `seq += 1`，首帧 Full、后续 Delta
docs/account-sync.md:151:  `plan_sync`）可调，UI 接线登记后续；
docs/account-sync.md:153:  （Web SW / 移动前台任务），登记后续；
docs/account-sync.md:158:  不动，故 iOS 侧同步登记后续。
docs/README.md:32:- 标题不带内部条目编号（`（TODO Lxxx）` 不出现在 H1）；正文中的设计登记
docs/desktop-framework.md:3:> TODO.md L112「搭建 Tauri + Rust 桌面应用框架」的落地说明。
docs/desktop-framework.md:228:## 5. 边界登记与后续
docs/desktop-framework.md:230:| TODO | 现状 | 下一步 |
docs/installation.md:20:| macOS | `.app` / `.dmg`（公证为登记设计，CI 尚未接线） |
docs/rust-code-standards.md:3:> 状态 v1（2026-09-28），对应 TODO「定义统一的 Rust 代码规范和跨平台兼容性检查」。
docs/rust-code-standards.md:100:  TODO.md 记录进度与依据，二者在 commit message 中互相引用。
docs/rust-code-standards.md:138:缺 Redis 简单获取途径，其编译面由 `build` 覆盖，运行期矩阵归 TODO L469 后续登记）；
docs/rust-code-standards.md:157:  TODO.md §拍板登记 RustSec 条目；
docs/rust-code-standards.md:159:  clippy 债务已于 2026-09-28 一并清零；后续新增代码直接被 `-D warnings` 拦截；
docs/mobile-core-architecture.md:3:> TODO.md「移动端应用」节第 1 项「设计移动端 Rust 核心库架构（JNI + UniFFI）」
docs/mobile-core-architecture.md:14:* **L301 增量交付**（TODO 270）：Android Kotlin + Jetpack Compose 混合工程
docs/mobile-core-architecture.md:18:* **仍不交付**（按 TODO 顺序留给后续项，见 §13）：L118 首轮清单中 272/273/274
docs/mobile-core-architecture.md:26:┌──────────── 平台壳（后续 TODO：270/271）────────────┐
docs/mobile-core-architecture.md:34:│  MobileCore（观察列表/配置/演示数据源/错误映射）            │
docs/mobile-core-architecture.md:54:| **裸 JNI**（`jni = 0.21`，workspace 已 pin） | 逃生舱：uniffi 覆盖不到的场景——SDK 级回调（推送 token、生命周期）、性能敏感的零拷贝通道 | ⏸ 本轮不引用；架构上预留 pin 与本文档，首个真实场景（TODO 270/272）落地时启用 |
docs/mobile-core-architecture.md:108:`MobileCore` 持 `symbols`（观察列表）与 `api_url`（后端地址配置槽，TODO 272/274
docs/mobile-core-architecture.md:111:`api_url` 骨架内只随 `status_json` 下行（探测/同步属 TODO 274，本轮不联网）。
docs/mobile-core-architecture.md:113:## 8. 演示数据源
docs/mobile-core-architecture.md:117:/`synthetic_series` 两个入口）。真实数据源接入（api-gateway）是后续 TODO，届时
docs/mobile-core-architecture.md:127:| iOS | `staticlib`（`.a`，Swift 侧经 uniffi 生成的 Swift 绑定链接） | Xcode + Swift Package（TODO 271） | 骨架已落（L119：SwiftUI + UniFFI 绑定骨架 + 模型契约）；Xcode 工具链链路未启（无 macOS/Xcode） |
docs/mobile-core-architecture.md:205:   TODO.md 行号会随注释增长漂移，不作编号依据）。
docs/mobile-core-architecture.md:210:   （TODO 270/272）启用。
docs/mobile-core-architecture.md:213:   平台 SDK 依赖，Linux 门禁可覆盖）；若后续引入 NDK-only 依赖需退回
docs/mobile-core-architecture.md:233:   wasm 门控同款式），不做 optional feature——TODO 272 启用 JNI 逃生舱时
docs/mobile-core-architecture.md:245:4. 推送 token/生命周期等 SDK 回调经 JNI 逃生舱回灌核心库（TODO 272 的首个
docs/mobile-core-architecture.md:250:## 13. 后续 TODO 映射
docs/mobile-core-architecture.md:252:| TODO 项 | 本文依托 | 状态 |
docs/platform-compliance.md:23:   用户主动反馈时附带（后续若引入崩溃收集，须在此节更新并随版本公告）。
docs/platform-compliance.md:61:| `fs` | 工作区数据读写 | 限 dialog 选定路径；后续可评估 scope 收窄 |
docs/deployment-runbook.md:193:后续调度。real-time-feed 的 WS 不推任务事件，推的是行情帧与 alerts 通道。
docs/alerting-and-diagnosis.md:41:   指标体系是后续项，本单只在诊断引擎知识库留规则槽位。
docs/alerting-and-diagnosis.md:92:   INFRA-002/003（Loki/AllocTracker）对应的是登记未实现指标，属规则
docs/data-lake-parquet.md:4:零代码改动；落地实现归「智能数据分区策略（L447）」及后续工程项。
docs/data-lake-parquet.md:105:  catalog v1，挂后续项；再往后的 ACID 事务需求出现时才评估 Delta/Iceberg。
docs/data-lake-parquet.md:118:## 9. 与后续 TODO 的边界
docs/cross-platform-architecture.md:3:> 状态：v1 草案（2026-09-28），对应 TODO「跨平台 Rust 架构设计」首项「设计统一跨平台架构」。
docs/cross-platform-architecture.md:28:| `web/` | 原生 JS + duckdb-wasm vendor + WebSocket | 浏览器 | 已接 real-time-feed WS 与 data-engine REST（见 TODO P3-2） |
docs/cross-platform-architecture.md:119:## 7. 落地路线图（映射本节 TODO 余项）
docs/cross-platform-architecture.md:121:| TODO 项 | 依赖/顺序 | 说明 |
docs/pgo-build-optimization.md:59:   PGO 二进制随发布流水线（TODO「多平台发布流水线」）产出并归档 profdata 溯源。
docs/mobile-offline.md:18:| **同步（sync / delta）** | 与**远端**做增量对齐：以指纹比较决定是否需要对齐、对齐哪些数据 | **出设备**（经网络到远端） | 决策与增量判断本轮落地；真实网络执行归远端接入 TODO（§8④） |
docs/mobile-offline.md:36:   后续 TODO，配置与文案面本轮就绪）。
docs/mobile-offline.md:88:  设置页 TODO）；语义对齐 alpha-core `platform::KeyValueStore`：后写覆盖、删除幂等。
docs/mobile-offline.md:121:1. 本轮台账记 L390（沿 TODO 行号；行号漂移不作编号依据）。
docs/mobile-offline.md:126:   决策 + 增量判断契约；`SyncDelta.needed=true` 后壳层的对齐动作归远端接入 TODO。
docs/mobile-offline.md:128:   跨版本迁移策略归发布流水线 TODO。
docs/mobile-offline.md:132:   后续 TODO，本轮交付配置面 + `dataScopeSummary` 文案函数。
docs/mobile-offline.md:146:## 10. 后续 TODO 映射
docs/mobile-offline.md:148:| TODO 项 | 本文依托 |
docs/审计-文档一致性.md:5:> 范围：README/README.zh、docs/**、公开门面（GitHub About），加上 TODO 自吹式与 `- [x]` 勾选主张。
docs/审计-文档一致性.md:38:| F-12 | docs/deployment-runbook.md（§3） | 裸机路径 Redis 连接「按 REDIS_URL 默认值自取」 | scripts/deploy-ubuntu.sh:160 仅 `docker-compose up -d clickhouse`，无 Redis 启动；services/data-engine/src/main.rs:814 与 services/real-time-feed/src/main.rs:533 `RedisStreamQueue::connect(&redis_url)?`（Redis 缺失即启动失败） | 不符 | P1 | 文档改「Redis 需自行提供（compose 内含 redis 服务）；缺失则 data-engine/real-time-feed 启动失败」；脚本补 `docker-compose up -d redis` 登记为后续增量（审计轮不动码） |
docs/审计-文档一致性.md:44:| F-18 | docs/docker-deployment.md | 「PR 阶段只验单平台 build 不入门禁」 | .github/workflows/ci.yml 无镜像构建作业（docker 仅 :274-276 e2e 起 redis 容器） | 文档超前于码 | P2 | 删去该半句或标「PR 阶段镜像验证尚未接线」 |
docs/审计-文档一致性.md:49:| F-23 | docs/installation.md | 「macOS `.app`/`.dmg`（公证走 CI secret）」 | grep codesign/notary 在 desktop-release.sh 与 release.yml 无命中；desktop-release.sh:10 仅指 docs/desktop-release.md 设计登记；release.yml:4 登记说明 | 文档超前于码 | P2 | 标注「公证为登记设计，CI 尚未接线」 |
docs/审计-文档一致性.md:51:| F-25 | docs/user-guide.md（数据存放节） | 「服务端只存公开行情，不存个人数据」 | services/api-gateway/src/account.rs:37（`alpha:account:` 存储前缀）、main.rs:409-411（profile/sync 路由）——账户面已落地 | 不符 | P2 | 补限定语「账户同步客户端未接线前」或指向 data-privacy.md |
docs/审计-文档一致性.md:52:| F-26 | docs/user-guide.md（通知节） | 「通知需显式开启（opt-in），见应用内隐私设置」 | mobile/android/app/src/main/AndroidManifest.xml 无 POST_NOTIFICATIONS；web/app/src 无 Notification.requestPermission；checkAlerts/setAlertRules 仅存在于 FFI 桥（AlphaBridge.kt + 生成绑定），无应用内设置 UI | 文档超前于码 | P1 | 改「通知为真机 TODO，无应用内设置面；opt-in 语义待壳层接线」 |
docs/审计-文档一致性.md:57:| F-31 | docs/mobile-privacy.md | 「应用内告警面板随门隐藏」 | MainActivity.kt 无告警面板 UI（无 checkAlerts/setAlertRules 调用）；通知送达仅 PushSyncSeam.kt:30 接口 | 文档超前于码 | P2 | 改「应用内告警面板未实现，随门隐藏语义登记」 |
docs/审计-文档一致性.md:71:| F-45 | docs/analysis.md §1 | api-gateway「纯 stub：health 返回硬编码状态，proxy 返回 mock，无真实转发/鉴权」 | services/api-gateway/src/main.rs:406（/v1/*path 反代）、:432（nest /api）、:935-950（真实探测三上游）、:89-93（JWT）、:156-158（RedisRateLimiter） | 码超前于文档 | P1 | 加现注：网关已落真实反代 + JWT/OIDC + RBAC + 限流 + 探测式 health |
docs/审计-文档一致性.md:75:| F-49 | docs/analysis.md §1 | wasm-analyzer + web「未接任何后端 API/WS」 | web/app.js:14-15（restBase/wsUrl 配置）、:37（fetch REST）、:356（new WebSocket） | 码超前于文档 | P2 | 改「可配置直连 data-engine REST + real-time-feed WS，缺省走演示数据」 |
docs/web-framework-selection.md:27:| 本仓既有投入 | 零（web/ 为 vanilla JS，无迁移负担） | 零 | 零（TODO L428 仅是设想项，无一行代码） |
docs/web-framework-selection.md:28:| 后续 TODO 项契合度 | L428 组件化界面 / L429 图表库 / L430 SQL 编辑器均有现成 React 路线 | L430 需自封装 | 全部需自行开发 |
docs/web-framework-selection.md:41:- **TODO L428**（「基于 Yew/Leptos 开发 Web 端组件化数据分析界面」）：✅
docs/web-framework-selection.md:45:- **TODO L429**（图表库 D3 + Canvas）：✅ **已定论 lightweight-charts 5.2 并落地**
docs/web-framework-selection.md:50:  ~300KB+）落备选。TODO 原文「D3.js」按此定论解释落实。
docs/web-framework-selection.md:112:  front dist（vanilla 演示壳）。接入策略两选项，留后续单定：
docs/web-framework-selection.md:119:  随 A 落地撤销备选。落地分三步（见 TODO 拍板登记）：桥接模块 + 产物接入
docs/web-framework-selection.md:141:1. 记账行号 L427（沿 TODO 行号；行号漂移不作编号依据）。
docs/web-framework-selection.md:151:   重试两次回退演示数据），其余面板仍走样本。
docs/docker-deployment.md:67:## 5. 已知边界（登记为后续项）
docs/docker-deployment.md:71:- **依赖层缓存**：`COPY . .` 后源码任意变更即整编（cargo-chef 分层可解，构建耗时优化归后续）；
docs/docker-deployment.md:72:- **CI 镜像构建接线**（buildx push 进 L470 发布流水线；PR 阶段镜像验证尚未接线，只验单平台 build）；
docs/android-widget.md:31:热路由归单实例改造 TODO）；动态 shortcuts（随用户观察列表生成）归设置
docs/android-widget.md:32:页 TODO。
docs/android-widget.md:39:浅色底（widget 深色适配归主题项后续）。
docs/store-publishing.md:11:| App Store | archive → `.ipa`（ExportOptions app-store-connect） | `scripts/ios-release.sh`——脚本可执行登记，**ipa 产线未实现**：Xcode 工程未入库（`mobile/ios/` 属 L119 交付面），archive 阶段守卫拦截显式报缺；真机在 release.yml macOS 作业时归发布前置项 |
docs/store-publishing.md:12:| Microsoft Store | 见 §4（PWA 备选 / MSIX 后续） | — |
docs/store-publishing.md:39:开发者证书，归 L470 Windows 作业后续。本项不产 MSIX，只登记路径与
docs/store-publishing.md:46:本项交付三店（Play/App Store/MS）流程与元数据，国内市场提交登记为后续
docs/store-publishing.md:59:§1 的 Play 入口由控制台手工操作升级为可自动化提交（后续补充件，与
docs/store-publishing.md:74:- **验证方式**：本机 mock 服务全流程实测（token 表单/edit/bundle 字节
docs/store-publishing.md:77:  mock 复现；
docs/mobile-gestures.md:55:页面范围**不扩**：本轮不新增二级页/导航（导航骨架归后续 TODO，见 §9）。
docs/mobile-gestures.md:109:1. 本轮台账记 L367（沿 TODO 行号；行号漂移不作编号依据）。
docs/mobile-gestures.md:118:6. 页面范围不扩（沿用 L301 单页），导航/二级页归后续 TODO（§9）。
docs/mobile-gestures.md:130:## 9. 后续 TODO 映射
docs/mobile-gestures.md:132:| TODO 项 | 本文依托 |
[0m→ [0mRead services/api-gateway/src/main.rs [90m[offset=350, limit=160][0m
[0m→ [0mRead services/data-engine/src/main.rs [90m[offset=700, limit=240][0m
[0m→ [0mRead docs/market-data-api.md [90m[limit=240][0m
> build · gpt-6-luna
[0m→ [0mRead docs/data-lake-parquet.md [90m[limit=190][0m
[0m→ [0mRead services/data-engine/src/settings.rs [90m[limit=240][0m
[0m→ [0mRead services/api-gateway/src/main.rs [90m[offset=620, limit=115][0m
[0m$ [0mrg -n 'mirror_retr|MIRROR_RETRY|health|persistence|X-Api-Key|api_key' services/data-engine/src/main.rs services/data-engine/src/settings.rs | head -200
services/data-engine/src/settings.rs:17:    /// 第三方集成鉴权（L504）：api_keys 非空时行情数据面统一要求 X-Api-Key
services/data-engine/src/settings.rs:25:/// 运维面 /health、/metrics 不受本配置影响（存活探测与抓取器不带业务凭据）。
services/data-engine/src/settings.rs:28:    pub api_keys: Vec<String>,
services/data-engine/src/settings.rs:67:    pub persistence_enabled: bool,
services/data-engine/src/settings.rs:125:            .set_default("storage.persistence_enabled", false)
services/data-engine/src/settings.rs:126:            .expect("failed to set persistence default")
services/data-engine/src/settings.rs:147:            .set_default("security.api_keys", Vec::<String>::new())
services/data-engine/src/settings.rs:148:            .expect("failed to set security.api_keys default")
services/data-engine/src/settings.rs:203:        assert!(!cfg.storage.persistence_enabled);
services/data-engine/src/settings.rs:210:        assert!(cfg.security.api_keys.is_empty());
services/data-engine/src/settings.rs:230:                  persistence_enabled: true
services/data-engine/src/settings.rs:236:                  api_keys: ["third-party-key-a", "third-party-key-b"]
services/data-engine/src/settings.rs:252:        assert!(cfg.storage.persistence_enabled);
services/data-engine/src/settings.rs:260:            cfg.security.api_keys,
services/data-engine/src/main.rs:92:const MIRROR_RETRY_BUFFER_CAP: usize = 100_000;
services/data-engine/src/main.rs:94:const MIRROR_RETRY_INTERVAL_SECS: u64 = 5;
services/data-engine/src/main.rs:95:const MIRROR_RETRY_BATCH: usize = 500;
services/data-engine/src/main.rs:192:    /// TimescaleDB 持久化镜像（storage.persistence_enabled 装配；None = 仅内存）
services/data-engine/src/main.rs:193:    persistence: Option<Arc<TimescaleTimeSeriesStorage>>,
services/data-engine/src/main.rs:198:    mirror_retries: Arc<MirrorRetryBuffer>,
services/data-engine/src/main.rs:247:        let persistence = initialize_persistence(&config.storage).await;
services/data-engine/src/main.rs:253:            persistence,
services/data-engine/src/main.rs:257:            mirror_retries: Arc::new(MirrorRetryBuffer::with_capacity(MIRROR_RETRY_BUFFER_CAP)),
services/data-engine/src/main.rs:292:        if let Some(persistence) = self.persistence.as_ref() {
services/data-engine/src/main.rs:293:            if let Err(err) = persistence.insert_market_data(market_data).await {
services/data-engine/src/main.rs:296:                self.mirror_retries.push(market_data.clone());
services/data-engine/src/main.rs:299:                    self.mirror_retries.len(),
services/data-engine/src/main.rs:443:    start_mirror_retry_worker(state.clone());
services/data-engine/src/main.rs:650:    // 行情数据面（L504 第三方集成面）：security.api_keys 非空时统一过
services/data-engine/src/main.rs:651:    // API key 门；/health 与 /metrics 运维面豁免（存活探测与抓取器不带
services/data-engine/src/main.rs:668:            require_api_key,
services/data-engine/src/main.rs:672:        .route("/health", get(health_check))
services/data-engine/src/main.rs:690:/// 第三方集成鉴权门（L504）：`security.api_keys` 非空时要求请求携带
services/data-engine/src/main.rs:691:/// `X-Api-Key` 且命中其一，否则 401。空表 = 关闭（内网默认形态，不带
services/data-engine/src/main.rs:693:async fn require_api_key(
services/data-engine/src/main.rs:698:    let keys = state.config.security.api_keys.as_slice();
services/data-engine/src/main.rs:704:        .get("X-Api-Key")
services/data-engine/src/main.rs:707:    if api_key_matches(provided, keys) {
services/data-engine/src/main.rs:712:            "missing or invalid API key (X-Api-Key)",
services/data-engine/src/main.rs:720:fn api_key_matches(candidate: &str, keys: &[String]) -> bool {
services/data-engine/src/main.rs:763:/// - 未启用（persistence_enabled=false）→ None，纯内存；
services/data-engine/src/main.rs:766:async fn initialize_persistence(
services/data-engine/src/main.rs:769:    if !settings.persistence_enabled {
services/data-engine/src/main.rs:782:            tracing::info!("Timescale persistence enabled");
services/data-engine/src/main.rs:894:fn start_mirror_retry_worker(state: Arc<AppState>) {
services/data-engine/src/main.rs:895:    let Some(persistence) = state.persistence.clone() else {
services/data-engine/src/main.rs:896:        tracing::info!("Mirror retry worker not started: persistence disabled");
services/data-engine/src/main.rs:899:    let buffer = state.mirror_retries.clone();
services/data-engine/src/main.rs:901:        let mut ticker = interval(StdDuration::from_secs(MIRROR_RETRY_INTERVAL_SECS));
services/data-engine/src/main.rs:907:            let rewrote = run_mirror_retry_round(&persistence, &buffer).await;
services/data-engine/src/main.rs:983:async fn run_mirror_retry_round(
services/data-engine/src/main.rs:984:    persistence: &Arc<TimescaleTimeSeriesStorage>,
services/data-engine/src/main.rs:987:    let batch = buffer.drain_for_retry(MIRROR_RETRY_BATCH);
services/data-engine/src/main.rs:992:        if let Err(err) = persistence.insert_market_data(market_data).await {
services/data-engine/src/main.rs:1299:async fn health_check(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
services/data-engine/src/main.rs:1303:        "status": "healthy",
services/data-engine/src/main.rs:2294:            persistence: None,
services/data-engine/src/main.rs:2299:            mirror_retries: Arc::new(MirrorRetryBuffer::with_capacity(MIRROR_RETRY_BUFFER_CAP)),
services/data-engine/src/main.rs:2500:    fn api_key_comparison_is_exact_and_full_scan() {
services/data-engine/src/main.rs:2504:        assert!(api_key_matches("secret-1", &keys(&["secret-1"])));
services/data-engine/src/main.rs:2505:        assert!(!api_key_matches("secret-2", &keys(&["secret-1"])));
services/data-engine/src/main.rs:2507:        assert!(!api_key_matches("short", &keys(&["a-much-longer-key"])));
services/data-engine/src/main.rs:2509:        assert!(api_key_matches("b", &keys(&["a", "b"])));
services/data-engine/src/main.rs:2510:        assert!(!api_key_matches("", &keys(&["a"])));
services/data-engine/src/main.rs:2511:        assert!(!api_key_matches("anything", &keys(&[])));
services/data-engine/src/main.rs:2515:    /// /health、/metrics 运维面豁免（不带业务凭据的存活探测不受影响）。
services/data-engine/src/main.rs:2517:    async fn api_key_gate_off_by_default_and_enforced_when_configured() {
services/data-engine/src/main.rs:2524:                builder = builder.header("X-Api-Key", key);
services/data-engine/src/main.rs:2536:        config.security.api_keys = vec!["k1".to_string(), "k2".to_string()];
services/data-engine/src/main.rs:2557:        let res = request(on.clone(), "/health", None).await.unwrap();
services/data-engine/src/main.rs:3082:            persistence_enabled: enabled,
services/data-engine/src/main.rs:3104:    fn mirror_retry_buffer_requeues_in_order_to_front() {
services/data-engine/src/main.rs:3127:    fn mirror_retry_buffer_drops_oldest_and_counts_on_overflow() {
services/data-engine/src/main.rs:3142:    async fn persistence_disabled_writes_memory_only() {
services/data-engine/src/main.rs:3144:        assert!(initialize_persistence(&disabled).await.is_none());
services/data-engine/src/main.rs:3148:            state.persistence.is_none(),
services/data-engine/src/main.rs:3149:            "default config must not assemble persistence"
services/data-engine/src/main.rs:3174:    async fn persistence_enabled_with_bad_url_degrades_to_memory() {
services/data-engine/src/main.rs:3176:        assert!(initialize_persistence(&missing).await.is_none());
services/data-engine/src/main.rs:3184:        assert!(initialize_persistence(&invalid).await.is_none());
services/data-engine/src/main.rs:3192:        assert!(state.persistence.is_none());
services/data-engine/src/main.rs:3288:        let persistence = Arc::new(TimescaleTimeSeriesStorage::connect(&url).await?);
services/data-engine/src/main.rs:3292:            persistence: Some(persistence.clone()),
services/data-engine/src/main.rs:3297:            mirror_retries: Arc::new(MirrorRetryBuffer::with_capacity(MIRROR_RETRY_BUFFER_CAP)),
services/data-engine/src/main.rs:3330:        assert_eq!(persistence.count(&symbol).await?, 2);
services/data-engine/src/main.rs:3331:        let latest = persistence.latest(&symbol).await?.expect("persisted row");
[0m$ [0mrg -n '^\s*- \[x\]' TODO.md | wc -l; rg -n '^\s*- \[ \]' TODO.md | wc -l; git status --short
142
0
 D web/dist/.gitkeep
> build · gpt-6-luna
[0m→ [0mRead services/data-engine/src/main.rs [90m[offset=1288, limit=55][0m
[0m→ [0mRead services/data-engine/src/main.rs [90m[offset=275, limit=36][0m
[0m→ [0mRead services/data-engine/src/main.rs [90m[offset=975, limit=55][0m
[0m→ [0mRead docs/README.md [90m[limit=90][0m
[0m→ [0mRead docs/mobile-core-architecture.md [90m[limit=45][0m
> build · gpt-6-luna
[0m✱ [0mGrep "fn push\(|MIRROR_RETRY_BUFFER_CAP|dropped_total" [90min services/data-engine/src/main.rs · 12 matches[0m
> build · gpt-6-luna
[0m→ [0mRead services/data-engine/src/main.rs [90m[offset=88, limit=73][0m
> build · gpt-6-luna
[0m→ [0mRead services/data-engine/src/main.rs [90m[offset=1023, limit=125][0m
> build · gpt-6-luna
[0m→ [0mRead services/data-engine/src/main.rs [90m[offset=1148, limit=120][0m
> build · gpt-6-luna
**TODO 台账：142 项已勾选，0 项未勾选。** 勾选完成不代表生产风险已消除；以下按风险排序。

### 1）最薄弱的 3 处

1. **高危：服务可绕过网关直接访问，认证默认关闭。** `docker-compose.yml:90-107, 123-149` 直接发布 data-engine、real-time-feed、collector 端口；网关认证默认 `off`（`services/api-gateway/src/main.rs:90-91`），Compose 未启用认证，data-engine API key 默认空（`services/data-engine/src/settings.rs:147`）。因此只加固网关并不能保护上游；Compose 还使用固定数据库口令（`docker-compose.yml:7-9, 29-32`）。**风险：未授权读取/操作及凭据暴露。**
2. **高风险：持久化故障可能被伪装成健康运行。** Timescale 连接失败时服务降级到内存继续运行（`services/data-engine/src/main.rs:762-790`），健康检查仍固定返回 `healthy`（`:1298-1308`）。写入失败的重试队列仅在进程内，重启即丢，达到 100,000 条还会淘汰最旧数据（`:91-101, 117-125`）。**风险：行情已被消费但未持久化，监控仍显示正常。**
3. **高风险：格式合法但字段无效的行情会被直接确认并丢弃。** `normalize_quote()` 返回 `None` 时只记指标、写日志并 ack（`services/data-engine/src/main.rs:1178-1190`），不会进入 DLQ 或等待重试。**风险：坏数据不可恢复，且排查依赖另行发现指标异常。**

### 2）最值得补的 3 项新能力

1. **可安全对外部署的生产模式。** 配套 fail-closed 鉴权、密钥管理、TLS 和上游私网隔离。现在没有，是因为架构定位为自托管内网，Cloudflare Tunnel 尚未配置，认证也刻意默认关闭以保持旧部署行为（`docs/architecture.md:6, 76`；`docs/auth.md:8-18`）。但当前 Compose 暴露上游端口，和“内部服务仅限内网”的部署声明存在落差。
2. **完整的金融数据质量与业务数据域。** 补不可丢弃的坏数据隔离/回放、交易日历、复权行情，以及公告/新闻接入和索引。现在没有，是因为仓库明确将复权、行业映射、批处理和公告/新闻列为未实现，且决策记录称缺少消费方证据（`docs/architecture.md:36-37, 49-57`）。
3. **可复现的研究数据集与实验流程。** 从不可变数据版本一路打通到策略参数、代码版本、随机种子和回测结果。现在只有登记契约/登记表，湖层 API 和消费 UI 尚未形成完整流程；架构评审将这类能力放在 P3，并以没有明确消费方为暂缓理由（`docs/architecture-review.md:137-142, 183`）。

### 3）文档与实现可能对不上的地方

- **异常行情去向不一致：** `docs/architecture.md:47-48` 声称异常值隔离到 DLQ；但可解码、不可规范化的行情实际会 ack 后跳过（`services/data-engine/src/main.rs:1178-1190`）。这是数据可靠性承诺差异。
- **网络暴露边界不一致：** `docs/architecture.md:76` 称内部服务仅限内网；Compose 却直接发布 data-engine、feed、collector 端口（`docker-compose.yml:92-94, 123-149`）。`docs/auth.md:57-60` 也明确说明直连 feed 不受网关认证保护。
- **数据湖文档自相矛盾：** `docs/data-lake-parquet.md:3-4` 仍称“零代码改动、落地实现归后续”，后文却记录 writer、导出接线和 ListingTable 已实现（`:127-145`）。
- **移动端架构图疑似过期：** `docs/mobile-core-architecture.md:14-21` 已记 Android 壳落地，但 `:26-30` 仍把 Android/Swift 平台壳标作“后续 TODO”。
