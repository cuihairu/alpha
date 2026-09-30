//! Tauri 代码生成：读取 tauri.conf.json 并注入构建期宏。
//!
//! 仅在 gui 特性开启时执行——框架层（lib）在无 GUI 系统库的环境（Linux CI）
//! 单独编译测试时无需解析 Tauri 上下文。

fn main() {
    println!("cargo:rerun-if-changed=tauri.conf.json");
    if std::env::var_os("CARGO_FEATURE_GUI").is_some() {
        tauri_build::build();
    }
}
