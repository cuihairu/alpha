//! Alpha Finance 桌面应用（可执行入口）
//!
//! 全部业务逻辑在框架层 [`alpha_desktop`]（零 Tauri 依赖，可在任意 CI 编译测试）；
//! 本文件只做进程入口：gui 特性开启时交给 Tauri 接线层，否则说明原因后退出。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(feature = "gui")]
    alpha_desktop::gui::run();

    #[cfg(not(feature = "gui"))]
    eprintln!(
        "alpha-desktop 以 --no-default-features 构建：Tauri 的窗口/命令接线需要 gui 特性 \
         （Linux 还需 WebKitGTK + libsoup 系统库）。框架层库测试请用 \
         `cargo test -p alpha-desktop --no-default-features --lib`；\
         完整应用请用 `cargo run -p alpha-desktop` 或 `tauri dev`。"
    );
}
