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
| 接线层 | `desktop/src/gui.rs`、`main.rs`、`tauri.conf.json` | Tauri（`gui` 特性） | Linux：`scripts/check-desktop.sh` [4/5][5/5]（假 pkg-config 做类型检查与 lint，不链接）；链接与运行：CI `Desktop (macOS)` |

接线层的职责只有三件：解析平台路径/句柄 → 委派框架层 → 映射错误给前端。
所有业务判断都在框架层，因此 macOS 作业覆盖的是极薄的胶水代码，而逻辑正确性
由 124 个可在 Linux 上跑的框架层单测 + 10 个配置契约测试 + 9 个接线薄度契约测试
保证（详见 §4.1；L113 落地后计数，见 §6）。

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
| `ipc` | 前后端 DTO | 请求（`AnalyzeRequest`/`ExportRequest`）与应答（`InitPayload`）；字段名即前端契约，`serde` 往返 + 前端 JSON 负载解析由单测锁定 |
| `app` | 应用元信息 | 名称/版本/平台/架构 + `identifier`（只能从 Tauri 运行时读，故由接线层注入，框架层不编造） |
| `state` | `manage` 的载荷 + `bootstrap_app` | 分析引擎 + 目录布局，只含纯 Rust 类型 ⇒ 可在无 GUI 环境构造与测试；`bootstrap_app` 是 `initialize_app` 的实现体（配置自举落盘 + 降级提示都在这里） |

各模块对命令层暴露的 `*_request` / `bootstrap_app` 入口是**唯一**的接线面——命令体里
不应再出现任何判断，见 §4.1 的薄度契约。

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
且终点的分析结果是真实计算而非桩数据。L113 另加导出卡片：`dialog.save` 拿路径 →
`export_symbol_to_file` 落盘（见 §6）。壳用外部 JS 文件（不放行 CSP 的
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

### 4.1 接线层薄度契约（首轮 CI 红灯的直接产物）

首轮 CI 的第二个红灯是**业务代码写在了编译不了的地方**：命令体里把框架层的
`validate() -> Result<(), Vec<String>>` 当成 `Vec<String>` 用（`is_empty()` 直接编译
不过）。这类错误在 macOS 作业编译 gui 层之前没有任何本地手段能发现，而 macOS 作业一
轮要几分钟——红灯是往返延迟，不是反馈。

因此把命令体改写成「只委派」：所有判断（空标的、未知方向串、未知导出格式、空符号
列表、配置自举与降级提示）都下沉到框架层的 `*_request` / `bootstrap_app` 入口，
每个入口在 Linux 门禁里有单测：

| 命令 | 委派入口 | 判断在哪 |
| --- | --- | --- |
| `initialize_app` | `state::bootstrap_app` | 配置自举落盘、降级日志、问题列表 |
| `analyze_symbol` | `analysis::analyze_request` | 空标的 |
| `get_real_time_quotes` | `analysis::quotes_request` | 空标的列表 |
| `set_price_alert` | `alerts::upsert_request` | 方向串解析、价格校验 |
| `export_data` | `export::export_request` | 格式解析、空列表 |
| `export_symbol_to_file`（L113） | `export::export_symbol_request` | 格式解析、空标的、后缀一致性（见 §6） |
| `get_app_info` | `app::app_info` | 纯读取（唯一无失败路径的命令） |

`desktop/tests/wiring_contract.rs`（9 例）把这个约定变成可本地执行的断言：命令体不得
出现判空/兜底/自造错误串；每个命令必须委派到上表的入口并 `map_err`；接线层不得绕过
入口直接调底层（`config::load_or_default`、`AlertKind::parse` 等）；`generate_handler!`
注册的命令与兜底壳 `invoke` 一致；`lib.rs` 的重导出都指向真实存在的项；文件行数上限
（防止接线层重新长胖）；兜底壳导出走原生 `dialog.save`（L113，见 §6）。反向用例已实测：在 `get_app_info` 里塞回 `validate().unwrap_or_default()`
这类判断，9 例中 2 例转红。

诚实边界：`tests/wiring_contract.rs` 是**源码契约**断言，它能守住「接线层不该干什么」，
但不能替代编译。补上编译的那一步见下一节。

### 4.2 假 pkg-config：把 gui.rs 的**类型检查**搬回 Linux

`cargo check`/`cargo clippy` 不链接。Tauri 1.x 的 sys crate（`webkit2gtk-sys`、
`soup2-sys`、`javascriptcore-rs-sys`）只在 build 期跑 `pkg-config` 查
`webkit2gtk-4.0` / `javascriptcoregtk-4.0` / `libsoup-2.4`。于是
`scripts/desktop-fake-pc/` 里放一组「版本号给足、`Libs`/`Cflags` 留空」的 `.pc`，
配合 `PKG_CONFIG_PATH` 与 `PKG_CONFIG_ALLOW_SYSTEM_CFLAGS=1`，依赖图就能在任意 Linux
runner 上完整编译：

```
PKG_CONFIG_PATH="$PWD/scripts/desktop-fake-pc" \
    PKG_CONFIG_LIBDIR="$PWD/target/desktop-fake-pc-system" \
    PKG_CONFIG_ALLOW_SYSTEM_CFLAGS=1 \
    cargo clippy -p alpha-desktop --features gui --all-targets -- -D warnings
```

`PKG_CONFIG_LIBDIR` 那一行是必需的，不是保险：pkg-config **同时**搜 `PKG_CONFIG_PATH`
和 `PKG_CONFIG_LIBDIR`（后者默认是系统 `.pc` 目录），所以目录里缺的包会被本机装着的
真件悄悄兜住——最初的 8 个 `.pc` 在本机（装了 GTK3）怎么都绿，推到 CI 的
`ubuntu-latest` 就红在 `The system library gobject-2.0 required by crate glib-sys was
not found`。指到空目录后 `.pc` 集合必须**自包含**（现 18 个，含
`gobject-2.0`/`gio-2.0`/`gmodule-2.0`/`gdk-3.0`/`atk`/`pango`/`cairo-gobject`/
`gdk-x11-3.0`/`x11`），`Requires` 也要照抄真实依赖关系，于是任何 Linux 机器结果
一致。反向验证过：删掉 `gobject-2.0.pc` 并 `cargo clean -p glib-sys -p gobject-sys
-p gdk-sys -p atk-sys`（不 clean 就命中 clippy 缓存，看不出变化）→ [5/5] 转红。

这一步（门禁 [5/5]）把 `#[tauri::command]` 宏展开、`AppHandle`/`State` 用法、
`generate_context!`、以及接线层对框架层的全部调用签名都纳入 Linux 门禁。首轮的
编译错误（`validate() -> Result<(), Vec<String>>` 被当 `Vec<String>` 用）就是在这条
命令下复现并拦下的——修复前手动跑一次即得完全相同的 `E0599: no method named
is_empty found for unit type ()`。

**不能覆盖的**：链接与启动窗口仍然需要真实 WebKitGTK（或 CI 的 macOS 作业，那里
WKWebView 内置）。假 `.pc` 只让类型系统与 lint 在 Linux 上干活。

顺带修掉一个真实缺陷：为了在 Linux 上编 gui 特性，暴露出上游的 semver 破坏。
依赖链是 `tauri` → `notify-rust 4`（声明 `zbus = "5"`，**仅 Linux/BSD**）→
`zbus 5.11.0`（再声明 `zbus_macros = "^5.11.0"`，caret）。caret 区间允许解析到
`zbus_macros 5.19.0`，而 5.19 的 `#[interface]` / `DBusError` 宏生成的代码引用
zbus 5.11 未导出的 `DispatchResult2` 等符号，`cargo check` 到 `zbus` 本体就报
9 个错误（`E0425` / `E0433` / `E0599`）——已实测复现，不是推测。

处理办法与理由：

* 不能靠锁文件兜底：本仓 `.gitignore` 忽略 `Cargo.lock`（库惯例），CI 每次全新解析；
* 因此在 `desktop/Cargo.toml` 里加一个**代码不引用**的 optional 直接依赖
  `zbus-macros-pin = { package = "zbus_macros", version = "=5.11.0" }`（挂 `gui` 特性），
  把这条链钉在已知可编译的组合上；
* 代价要认：zbus 本身也被锁在这条链上。`cargo update -p zbus --precise 5.19.0`
  会在**解析期**硬失败（zbus 5.19 要求 `zbus_macros ^5.19.0`），想升级得先解钉版
  ——这是有意的：响亮的解析失败好过只有 Linux 能撞上的静默编译失败。

这个坑对 macOS CI 天然不可见（`zbus` 是 Linux/BSD 专属依赖，macOS 作业从不编译
它），所以只能靠 Linux 上的 `[5/5]` 和上面的钉版兜住。

## 5. 本轮不做的（留给后续 TODO）

| TODO | 现状 | 下一步 |
| --- | --- | --- |
| L113 文件系统集成与本地导出 | ✅ 已落地（见 §6）：用户自选路径导出闭环 | 覆盖已存在文件直接替换（写前确认未做，留待后续） |
| L114 系统通知与托盘 | ✅ 已闭环（见 §7）：通知模型/队列/托盘菜单状态机 + 告警检查链（check_alerts）+ 托盘接线（platform.rs） | 通知点击唤起主窗；托盘图标随状态换图（需多套图标资产）；定时轮询取数 |
| L115 本地数据库同步与离线模式 | `FileKeyValueStore` 是雏形（KV 语义够用，非查询型） | 换 SQLite/本地缓存并做同步冲突策略 |
| L116 快捷键与右键菜单 | `global-shortcut-all` 特性已在 allowlist | 注册快捷键与菜单事件 |

另：演示行情是确定性生成的占位数据，接真实后端（api-gateway）时只需替换
`market` 模块的取数实现，分析/导出链路不动。

## 6. L113 原生文件集成与本地导出（2026-09-30）

口径：**用户自选路径导出闭环**——前端经原生「另存为」对话框拿到路径，
调新命令落盘；原 `export_data`（写应用数据目录 `exports/` 的快速导出）保留，
两条路径复用同一 CSV/JSON 序列化口径。

```
兜底壳导出卡片 ──dialog.save──▶ 用户选路径 ──invoke──▶ gui::export_symbol_to_file
        │ 薄委派                                              │ 无判断
        ▼                                                     ▼
            export::export_symbol_request（格式解析/空标的/取数）
                        │ export::export_to_file（后缀一致性/原子落盘）
                        ▼
                磁盘任意路径（父目录自建、tmp + rename 原子替换）
```

框架层断言（`desktop/src/export.rs`，Linux 门禁可跑）：

* 路径后缀必须与格式一致（大小写不敏感），不符/缺文件名/空序列/空标的/
  未知格式一律先拒绝且不留任何文件（含临时文件）；
* 父目录不存在自动创建；写盘走临时文件 + rename（与 `config::save` 同口径，
  崩溃不断半截文件；覆盖已存在文件直接替换——写前确认未做，见本节末假设）；
* `ExportOutcome` 补 `Serialize`（命令返回值经 Tauri 序列化给前端，字段
  `path`/`filename`/`rows` 由单测锁定前端契约）。

接线与前端：`gui::export_symbol_to_file` 只做「透传三参 → 委派 →
`map_err`」（注册命令 6 → 7，`gui.rs` 仍在 160 行薄度上限内）；
兜底壳加导出卡片（`dialog.save` 取路径、后缀定格式、取消显示"已取消"，
非 Tauri 环境按钮禁用并注明）；`dialog`/`fs` 的 allowlist 与 Cargo 特性
在 L112 已对齐，本轮无需改配置。

门禁增量：`check-desktop.sh` [1/5] 加 `node --check`（手写 JS 无构建期
检查）；`wiring_contract.rs` 8 → 9 例（新命令的无判断/委派/注册一致性 +
壳 `dialog.save` 链路断言）；[5/5] 假 pkg-config 下新命令的宏展开与
`Serialize` 界自动被类型检查。

非交互假设（自行判定，已注明）：单文件单标的（批量多标的仍走 `exports/`
目录导出）；覆盖写直接替换；`dialog.save` 取消返回 null（v1 约定）按"已取消"
处理；演示行情仍是确定性占位数据（换真实取数只动 `market` 模块）。
## 7. L114 系统通知与托盘集成（2026-09-30）

口径：**通知/托盘的纯逻辑下沉框架层，平台 API 留在接线层**——与 L112/L113
同一条可验证性主线：能在 Linux 门禁跑单测的部分全部下沉，留在 macOS 作业
的只剩「取句柄 → 委派 → `map_err`」的薄胶水。

```
告警触发/手动发送 ──invoke──▶ gui::send_notification（薄委派）
        │                          │ 无判断
        ▼                          ▼
    notify::notify_request（级别解析/判空/id 生成/入队）
        │ notify::NotificationQueue（有界 FIFO + 同标题正文去重）
        ▼
    tauri::api::notification::Notification::show()（接线层平台胶水）

告警集合变化 ──invoke──▶ gui::set_tray_status（薄委派）
        │                          │
        ▼                          ▼
    notify::tray_status_request（读告警文件 → 生效数/最近触发 → 状态文本）
        ▼
    tray_handle_by_id("main").set_tooltip()（接线层平台胶水）
```

框架层断言（`desktop/src/notify.rs`，Linux 门禁可跑）：

* `NotificationLevel` 解析大小写不敏感，未知级别拒绝；
* `notify_request` 空标的/空标题/未知级别一律先拒绝且不入队；
* `NotificationQueue` 同标题正文去重（窗口内重复触发只保留一条）、超容量
  驱逐最旧、`recent(n)` 新→旧；
* `tray_status_request` 停用告警不计入、无告警时状态文本为「无告警」、
  损坏告警文件回退空表（与 `alerts::load` 同口径）；
* `alert_notification` 复用 `AlertKind::matches` 判定，未触发/已停用返回
  `None`，触发时级别为 `Critical`（L112 注释标明的复用点）。

接线与状态：`gui.rs` 新增三命令（`send_notification`/`list_notifications`/
`set_tray_status`，注册命令 7 → 10，薄度上限 160 → 200 行）；`AppState` 内嵌
`Mutex<NotificationQueue>`（`notification_queue()` 访问器，与 `engine()`/
`paths()` 同模式）；`Notification`/`TrayState` 的字段名即前端契约（单测锁定
serde 往返）。`tauri::api::notification::Notification` 与
`tray_handle_by_id().set_tooltip()` 的类型用法由 `check-desktop.sh` [5/5]
（假 pkg-config）在 Linux 门禁检查；链接与运行仍由 macOS 作业验证。

非交互假设（自行判定，已注明）：通知队列容量 50（会话内历史，非持久化）；
托盘 tooltip 只反映告警状态（不显示行情）。

### 7.1 闭环补全（同日第二轮）：托盘接线 + 告警检查链

首轮留了两条缝：**托盘从未被创建**（tauri.conf 的 `systemTray` 段只负责
把图标嵌进 Context，`Builder` 不调 `.system_tray()` 就没有托盘，
`tray_handle_by_id` 永远拿不到句柄）与 **`alert_notification` 备而未接**
（无生产调用方）。本轮补上，分层不变：

```
托盘点击 ──▶ platform::on_tray_event（机械翻译）
                 │ notify::tray_action(id)（框架层映射，未知 id 忽略）
                 ▼
      ShowWindow: show+focus │ HideWindow: hide │ Quit: exit(0)
      动作后按真实可见性 tray.set_menu(tray_menu(visible)) 回写菜单

check_alerts ──▶ notify::check_request（框架层判定/入队/停用落盘）
                 │ 生效告警 × market::synthetic_quote（与 quotes_request 同口径）
                 ▼
      触发 → 入队（同文去重→不重复弹窗）→ alerts::deactivate（停用保留记录）
      返回「新入队」的通知 → platform::show_notifications（平台弹窗）
```

* **`src/platform.rs`**（gui 门控）：`Builder.system_tray(platform::system_tray())`
  显式 `with_id("main")`（默认 id 是随机串，`tray_handle_by_id` 会找不到）；
  菜单模型 `tray_menu_model(visible)`（显示/隐藏随主窗可见性互斥可用，
  分隔线隔开退出）与 `tray_action(id→动作)` 全在框架层 `notify.rs`（单测
  锁定状态机），platform.rs 只做 `TrayEntry → SystemTrayMenu` 的机械翻译，
  行数上限与「无命令/无自造错误串」由 wiring_contract
  `platform_glue_stays_mechanical` 锁定。
* **`check_request`**（框架层）：生效告警 → 确定性行情判定 → 触发即入队 +
  `alerts::deactivate` 停用落盘（沿用既有持久化语义：停用保留记录；一次性
  告警避免确定性恒价行情下反复触发）。单测覆盖：触发即停用且二次检查空手、
  未触发保持生效、同文重复布防被队列去重但状态机照常停用、缺文件按空表。
* **前端闭环**（`desktop-shell.js` + `index.html` 告警卡片）：布防
  （`set_price_alert`，演示目标价＝现价−1%）→ `check_alerts`（Rust 弹系统
  通知 + 入队 + 停用）→ `set_tray_status`（tooltip 同步）。注册命令 10 → 11，
  gui.rs 薄度上限 200 → 220 行。
* **两个真缺陷修复**：①Tauri 1.x 命令参数键默认 **camelCase**
  （tauri-macros `wrapper.rs` 的 `ArgumentCase::Camel`），兜底壳原来把
  `file_path` 写成 snake——运行期静默失配（L113 导出按钮真机会失败；CI 只
  编译不启动，从未暴露），改 `filePath` 并由 wiring_contract 新增源码断言
  拦截 `targetPrice`/`alertType`/`filePath`；②`send_notification` 误把通知
  id 当应用 identifier 传 `Notification::new`（notify-rust 会拿错误应用名），
  改回 bundle identifier。

真机验收边界（CI Desktop 只编译+链接，需真实桌面人工观察）：系统通知真实
弹出与权限授予；托盘图标出现与 `iconAsTemplate` 深浅色适配；菜单点击
显示/隐藏/退出真实生效（macOS `menuOnLeftClick=false` → 右键出菜单，Linux
行为另有差异）；tooltip 随 `set_tray_status` 变化；菜单可用态随窗口可见性
翻转；通知点击唤起主窗未做（v1 通知点击事件属后续）。
