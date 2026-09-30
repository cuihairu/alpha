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

## 这套 .pc 不干什么

* 不能构建可执行文件，不能链接（`-lwebkit2gtk-4.0` 之类根本不存在）；
* 不能启动窗口。链接与运行仍然需要真实 WebKitGTK，或直接用 CI 的
  `Desktop (macOS)` 作业（那里 WKWebView 内置、无外部系统依赖）。

用法见 `scripts/check-desktop.sh` 的 [5/5] 步；改动 `.pc` 时注意：加新文件 = 加一个
新的假依赖，删文件会让依赖图在 Linux 上编不过（那时说明该 crate 真的需要系统库，
应该走 macOS 作业而不是造假）。
