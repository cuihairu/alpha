# 桌面端 GUI 接线层的「假 pkg-config」

Tauri 1.x 在 Linux 需要 WebKitGTK 4.0 + libsoup2 的**系统库**（`webkit2gtk-sys` /
`soup2-sys` / `javascriptcore-rs-sys` 的 build 脚本跑 `pkg-config` 查这些包）。
本机（Ubuntu 26.04）只提供 webkit2gtk-**4.1**（soup3 ABI），4.0 已从源里下架，
于是 `desktop` 的 `gui` 特性在 Linux 上连编译都做不到——历史上这段代码只能由
CI 的 macOS 作业编译，签名漂移要等一次推送往返才暴露（TODO L112 连续两次 CI 红灯）。

## 这套 .pc 干什么

`cargo check` / `cargo clippy` **不链接**，sys crate 只需要 pkg-config 在 build 期
给出编译期 flag。目录里这些 `.pc` 因此是够用的：版本号给足（满足
`webkit2gtk >= 2.24` 之类约束），`Libs`/`Cflags` 留空 —— 于是依赖图能完整编译，
`desktop/src/gui.rs`（含 `#[tauri::command]` 宏展开、`AppHandle`/`State` 用法、
以及全部框架层调用签名）可以在任意 Linux runner 上做**类型检查与 lint**。

## 必须**自包含**：连系统上碰巧装着的包也要自己造

`pkg-config` 同时搜 `PKG_CONFIG_PATH` 与 `PKG_CONFIG_LIBDIR`（后者默认是系统 `.pc`
目录）。所以只设 `PATH` 时，目录里缺的那几个包会被**系统真件悄悄兜住**——本机装了
GTK3 的一切，于是最初的 8 个 `.pc` 在本地怎么都绿；CI 的 `ubuntu-latest` 没有 GTK，
没有兜底，第一次推送就红在 `The system library gobject-2.0 required by crate
glib-sys was not found`。

因此 `scripts/check-desktop.sh` 的 [5/5] 步额外把 `PKG_CONFIG_LIBDIR` 指到一个空目录
（`target/desktop-fake-pc-system/`），彻底屏蔽系统 `.pc`：**这个目录列全了，任何
Linux 机器（包括 CI）结果都一样**。代价是目录从 8 个涨到 18 个。

`Requires` 也要照抄真实关系（如 `atk` → `glib-2.0, gobject-2.0`）：pkg-config 会
递归解析 `Requires`，写漏了就会在屏蔽系统 `.pc` 后报出「`Package 'gobject-2.0',
required by 'atk', not found」这种绕一圈的错误。列全后的 `Requires` 仅用于让
pkg-config 自己满意，不产生真实链接需求。

反向验证过：删掉 `gobject-2.0.pc`（并 `cargo clean -p glib-sys -p gobject-sys -p
gdk-sys -p atk-sys` 让 build script 重跑，否则命中缓存看不出来）→ [5/5] 转红在
`atk` 找不到依赖。注意 clippy 的缓存会掩盖 `.pc` 的增删，验证时务必 clean。

## 这套 .pc 不干什么

* 不能构建可执行文件，不能链接（`-lwebkit2gtk-4.0` 之类根本不存在）；
* 不能启动窗口。链接与运行仍然需要真实 WebKitGTK，或直接用 CI 的
  `Desktop (macOS)` 作业（那里 WKWebView 内置、无外部系统依赖）。

用法见 `scripts/check-desktop.sh` 的 [5/5] 步。改动 `.pc` 时：加新文件 = 加一个
新的假依赖；删文件会让依赖图在 Linux 上编不过（那时说明该 crate 真的需要系统库，
应该走 macOS 作业而不是造假）。
