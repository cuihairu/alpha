# 桌面端框架（Tauri + Rust）

> TODO.md L112「搭建 Tauri + Rust 桌面应用框架」的落地说明。
> 目标口径：**可编译、可启动窗口、与现有 Web 前端集成的最小闭环**——
> 框架可测、接线极薄、构建门禁在无 GUI 依赖的 CI 里可跑。

## 1. 为什么分两层

Tauri 1.x 在 Linux 需要 WebKitGTK + libsoup 系统库（`soup2-sys`/`webkit2gtk-sys`
在 build 期跑 pkg-config），macOS/Windows 则免外部系统依赖。于是同一个 crate 若把
Tauri 写死为直接依赖，整个包在 Linux CI 上无法编译——本仓此前正是因此把
`alpha-desktop` 从 lint/test 门禁里整体排除（等于放弃了对它的任何自动化校验）。

本仓的做法：把 Tauri 降为 **可选依赖（`gui` 特性）**，桌面 crate 内部再分两层。

| 层 | 位置 | 依赖 | 验证方式 |
| --- | --- | --- | --- |
| 框架层 | `desktop/src/lib.rs` + 各模块 | std + alpha-core + chrono/csv/serde | 任意 Linux runner：`cargo test -p alpha-desktop --no-default-features --all-targets` |
| 接线层 | `desktop/src/gui.rs`、`main.rs`、`tauri.conf.json` | Tauri（`gui` 特性） | CI `Desktop (macOS)`：`cargo test -p alpha-desktop --all-targets` |

接线层的职责只有三件：解析平台路径 → 委派框架层 → 映射错误给前端。
所有业务逻辑都在框架层，因此 macOS 作业覆盖的是极薄的胶水代码，而逻辑正确性
由 91 个可在 Linux 上跑的框架层单测 + 10 个配置契约测试保证。

```
frontend (web/dist)  ──invoke──▶  gui.rs（#[tauri::command] 薄包装）
                                        │ 委派
                                        ▼
              config / paths / kv / market / analysis / export / alerts / app
                                        │ 计算
                                        ▼
                        alpha-core：AnalysisEngine、platform::KeyValueStore
```

## 2. 框架层模块

| 模块 | 职责 | 关键口径 |
| --- | --- | --- |
| `paths` | 配置/数据/导出/键值四类目录布局 | 幂等建目录；路径由接线层注入（平台差异不出框架层） |
| `config` | 应用配置读写 | 缺失→默认值并落盘自举；损坏→回退默认并上报 `ConfigSource::Recovered`；写盘走「临时文件 + rename」 |
| `kv` | `platform::KeyValueStore` 的桌面实现 | 键→**十六进制文件名**，阻断 `../` 路径穿越；每键一文件，值为原始字节 |
| `market` | 演示行情 | LCG 伪随机游走 + symbol 派生种子 + 可注入结束时刻 ⇒ 同输入同输出，可断言 |
| `analysis` | 编排 | 委托 `alpha_core::analytics::AnalysisEngine`，桌面端不重复实现指标 |
| `export` | CSV/JSON 导出 | CSV 表头与既有 web 导出口径一致；缺失可选字段写空单元格；导出时刻由调用方注入（文件名可断言） |
| `alerts` | 价格告警持久化 | 与 config 同口径（原子写 + 损坏回退空表）；方向用 `above/below` 可读标签落盘 |
| `ipc` | 前后端请求 DTO | 字段名即前端契约，`serde` 往返 + 前端 JSON 负载解析由单测锁定 |
| `app` | 应用元信息 | 名称/版本/平台/架构 |
| `state` | `manage` 的载荷 | 分析引擎 + 目录布局，只含纯 Rust 类型 ⇒ 可在无 GUI 环境构造与测试 |

## 3. 前端集成与「不白屏」

`desktop/tauri.conf.json` 的 `build.distDir` 指向 `../web/dist`（真实 Web 前端的
构建产物），`build.devPath` 指向 `http://localhost:8080`（`cd web && npm start`
的本地服务）。问题在于 `web/dist` 是 gitignore 的构建产物目录：新克隆仓库里它是空的，
Tauri 窗口会全白——骨架阶段就该堵住。

做法：把兜底壳 `web/dist/index.html` + `web/dist/desktop-shell.js` 作为**受版本控制**
的文件提交（`.gitignore` 加例外），并在 `tauri.conf.json` 的 **`build` 段**打开
`withGlobalTauri`，让壳通过 `window.__TAURI__.invoke(...)` 调用 Rust 命令：

```
initialize_app → get_app_info → get_real_time_quotes → analyze_symbol
```

四个命令串起来正好证明「窗口 → 前端 → Rust 命令 → alpha-core 计算」闭环成立，
且终点的分析结果是真实计算而非桩数据。壳用外部 JS 文件（不放行 CSP 的
`script-src 'unsafe-inline'`）。执行 `cd web && npm run build` 后真实前端会覆盖这两个
文件，届时 `git status` 显示该文件变更属预期。

### 3.1 v1 的两个 API 约定（写错只会让 CI 变红）

| 事项 | Tauri 1.x（本仓口径） | Tauri 2.x（易误用） |
| --- | --- | --- |
| 全局 API 开关 | `build.withGlobalTauri`（**build 段**） | `app.withGlobalTauri`（app 段） |
| 前端 IPC 入口 | `window.__TAURI__.invoke(cmd, args)` | `window.__TAURI__.ipcRenderer.invoke(...)` |

两处都不是类型系统能拦的：字段段写错由 `Config` 的 `deny_unknown_fields` 在
`tauri_build::build()` **运行时**抛出，而那一步需要 `gui` 特性（Linux runner 缺
WebKitGTK，跑不了）——于是只能等 CI 的 `Desktop (macOS)` 作业变红才暴露；
`ipcRenderer` 更隐蔽，是运行期 `undefined is not a function`，窗口静默无响应。
两者都被下节的契约测试锁住。

## 4. 构建门禁

`scripts/check-desktop.sh`（非交互，CI 作业 `Desktop Framework` 与本地一致）：

1. **配置自洽性**（`tauri.conf.json`，纯 Python 快速检查）：必填字段（identifier /
   productName / devPath / 窗口尺寸）、`bundle.icon` 文件真实存在、`distDir` 存在且含
   `index.html`、**`build.withGlobalTauri` 已开且不在 `tauri` 段**、兜底壳代码用
   `window.__TAURI__` 且无 `ipcRenderer`、**allowlist 放开的 API 在
   `desktop/Cargo.toml` 里确实启用了对应 tauri 特性**（两者漂移是运行期 panic 的
   经典来源）、`desktop/src-tauri/` 不存在。
2. **无孤儿配置**：真正的 crate 根是 `desktop/`，早期脚手架残留的
   `desktop/src-tauri/tauri.conf.json` 已删除（它 allowlist/devPath 与生效配置不同，
   留着只会让人改错文件）。
3. **框架层 clippy**：`cargo clippy -p alpha-desktop --no-default-features --all-targets -- -D warnings`。
4. **框架层单测 + 配置契约测试**：`cargo test -p alpha-desktop --no-default-features --all-targets`。

`desktop/tests/tauri_config.rs`（10 例）是第 1 步的**编译期加强版**：用
`tauri-utils` 走 `tauri_build::build()` 完全相同的解析路径
（`config::parse::read_from` → `serde_json::from_value::<Config>`），把字段名/段位置
是否匹配 Tauri 1.x schema 从「CI macOS 运行时才发现」前移到本地与常规 CI；另加
兜底壳 ↔ 接线层契约（命令名两侧一致、v1 IPC 入口、distDir 有入口文件、无孤儿目录）。

第 1 步与契约测试均已用反向用例验证：把 `withGlobalTauri` 挪回 `tauri` 段、把兜底壳
的 `api.invoke` 改成 `api.ipcRenderer.invoke`、给 allowlist 加未启用的 `fs-exists`、
抽走 `distDir/index.html` —— 均被拦下（改配置时 10 例中 9 例转红）。

## 5. 本轮不做的（留给后续 TODO）

| TODO | 现状 | 下一步 |
| --- | --- | --- |
| L113 文件系统集成与本地导出 | 导出逻辑已可测，落点在应用数据目录 `exports/` | 接原生「另存为」对话框与任意路径、写前确认 |
| L114 系统通知与托盘 | `AlertKind::matches` 已给出触发判定 | 接 Tauri notification/tray API 与定时轮询 |
| L115 本地数据库同步与离线模式 | `FileKeyValueStore` 是雏形（KV 语义够用，非查询型） | 换 SQLite/本地缓存并做同步冲突策略 |
| L116 快捷键与右键菜单 | `global-shortcut-all` 特性已在 allowlist | 注册快捷键与菜单事件 |

另：演示行情是确定性生成的占位数据，接真实后端（api-gateway）时只需替换
`market` 模块的取数实现，分析/导出链路不动。