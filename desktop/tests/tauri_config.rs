//! `tauri.conf.json` 契约测试（TODO L112 构建门禁的可执行部分）
//!
//! 为什么需要它：Tauri 的 `Config` 反序列化带 `deny_unknown_fields`——字段名写错或
//! 放错段（例如把 v1 的 `build.withGlobalTauri` 误放进 `tauri` 段）**只在
//! `tauri_build::build()` 运行时**报错，而那一步发生在 `gui` 特性开启的构建里
//! （Linux runner 缺 WebKitGTK，跑不了），于是只能等 CI 的 Desktop (macOS) 作业变红。
//! 本测试用纯 Rust 的 `tauri-utils` 走一遍与 `tauri-build` 完全相同的解析路径，
//! 让这类错误在本地与常规 CI（Desktop Framework 作业）就暴露。
//!
//! 同时锁定「兜底壳 ↔ Tauri 全局 API」的契约：v1 的全局 API 是
//! `window.__TAURI__.invoke`，`ipcRenderer` 是 v2 的东西——用错则窗口静默无响应。

use std::path::{Path, PathBuf};
use tauri_utils::config::{parse, Config};

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 与 tauri-build::build() 同路径解析配置（read_from → serde_json::from_value）
fn load_config() -> Config {
    let value = parse::read_from(crate_dir()).expect("读取 tauri.conf.json");
    serde_json::from_value(value).expect("tauri.conf.json 必须匹配 Tauri 1.x 的 Config schema")
}

#[test]
fn config_matches_tauri_schema() {
    // 最关键的一条：字段名/层级与 Tauri 1.x schema 完全一致（deny_unknown_fields）
    let config = load_config();
    assert!(config.tauri.bundle.active, "打包应处于启用状态");
}

#[test]
fn global_api_is_enabled_for_fallback_shell() {
    // v1 字段在 build 段；兜底壳依赖 window.__TAURI__ 调 Rust 命令
    let config = load_config();
    assert!(
        config.build.with_global_tauri,
        "兜底壳走 window.__TAURI__，必须开启 build.withGlobalTauri"
    );
}

#[test]
fn dist_dir_contains_window_entrypoint() {
    let config = load_config();
    let dist_dir = config.build.dist_dir;
    let resolved = crate_dir().join(dist_dir.to_string());
    assert!(
        resolved.join("index.html").is_file(),
        "distDir {} 必须含 index.html，否则窗口全白",
        resolved.display()
    );
}

#[test]
fn bundle_icons_exist() {
    let config = load_config();
    // v1 的字段名是 `icon`（单数、字符串数组），v2 才叫 `icons`
    let icons = &config.tauri.bundle.icon;
    assert!(!icons.is_empty(), "bundle.icon 不应为空");
    for icon in icons {
        let path = crate_dir().join(icon);
        assert!(path.is_file(), "图标缺失: {}", path.display());
    }
}

#[test]
fn window_definition_is_usable() {
    let config = load_config();
    // v1 里 `tauri.windows` 是必填数组（非 Option），尺寸是平铺字段（无 `size` 子对象）
    let windows = &config.tauri.windows;
    let window = windows.first().expect("应至少定义一个窗口");
    assert!(
        window.width > 0.0 && window.height > 0.0,
        "窗口尺寸非法: {}x{}",
        window.width,
        window.height
    );
    assert!(
        window.min_width.unwrap_or(0.0) <= window.width
            && window.min_height.unwrap_or(0.0) <= window.height,
        "最小尺寸大于初始尺寸，窗口无法按配置尺寸显示: {}x{} vs min {}x{}",
        window.width,
        window.height,
        window.min_width.unwrap_or(0.0),
        window.min_height.unwrap_or(0.0)
    );
}

#[test]
fn bundle_identifier_is_reverse_dns() {
    let config = load_config();
    let id = config.tauri.bundle.identifier.clone();
    assert!(id.contains('.'), "identifier 应为反向域名格式: {id}");
}

/// 去掉 JS 注释（字符串字面量原样保留）。
///
/// 用于「源码级 API 契约」断言：注释里可以自由解释「为什么用 v1 的 invoke 而非
/// v2 的 ipcRenderer」，但可执行代码里出现 v2 API 就必须报错。
fn strip_js_comments(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    if chars[i] == '\n' {
                        out.push('\n');
                    }
                    i += 1;
                }
                i = (i + 2).min(chars.len());
            }
            quote @ ('\'' | '"' | '`') => {
                out.push(quote);
                i += 1;
                while i < chars.len() {
                    if chars[i] == '\\' {
                        out.push(chars[i]);
                        if let Some(next) = chars.get(i + 1) {
                            out.push(*next);
                        }
                        i += 2;
                        continue;
                    }
                    let c = chars[i];
                    out.push(c);
                    i += 1;
                    if c == quote || c == '\n' {
                        break; // 字符串字面量结束（或未闭合——不追究）
                    }
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

#[test]
fn fallback_shell_uses_v1_global_invoke_api() {
    let config = load_config();
    if !config.build.with_global_tauri {
        return; // 上面的用例已断言该开关必须开启
    }
    let shell = crate_dir()
        .join(config.build.dist_dir.to_string())
        .join("desktop-shell.js");
    assert!(shell.is_file(), "兜底壳脚本缺失: {}", shell.display());
    let raw = std::fs::read_to_string(&shell).expect("读兜底壳");
    let code = strip_js_comments(&raw);

    assert!(
        code.contains("window.__TAURI__"),
        "兜底壳应通过 window.__TAURI__ 调 Rust 命令"
    );
    assert!(
        !code.contains("ipcRenderer"),
        "ipcRenderer 是 Tauri v2 的 API，v1 全局 API 直接提供 invoke"
    );
    assert!(
        code.contains("api.invoke"),
        "兜底壳应从 window.__TAURI__ 上取出 invoke（v1 全局 API 的唯一 IPC 入口）"
    );
}

#[test]
fn committed_command_names_match_wiring_layer() {
    // 兜底壳调用的命令必须在 gui.rs 的 generate_handler! 里注册，否则窗口静默失败
    let gui = std::fs::read_to_string(crate_dir().join("src/gui.rs")).expect("读 gui.rs");
    let raw_shell = std::fs::read_to_string(
        crate_dir()
            .join(load_config().build.dist_dir.to_string())
            .join("desktop-shell.js"),
    )
    .expect("读兜底壳");
    let shell = strip_js_comments(&raw_shell);

    for cmd in [
        "initialize_app",
        "get_app_info",
        "get_real_time_quotes",
        "analyze_symbol",
    ] {
        assert!(
            gui.contains(cmd),
            "gui.rs 未注册命令 {cmd}（兜底壳在调用它）"
        );
        assert!(
            shell.contains(&format!("invoke(\"{cmd}\"")),
            "兜底壳未通过 invoke 调用命令 {cmd}"
        );
    }
    assert!(
        gui.contains("tauri::generate_handler!"),
        "应存在 generate_handler! 注册"
    );
}

#[test]
fn no_orphan_src_tauri_directory() {
    // crate 根是 desktop/；src-tauri/ 里的 tauri.conf.json 是早期脚手架残留，
    // 其 allowlist/devPath 与生效配置不同，留着会让人改错文件
    assert!(
        !crate_dir().join("src-tauri").exists(),
        "desktop/src-tauri 应删除（crate 根是 desktop/）"
    );
}

#[test]
fn dev_path_and_dist_dir_are_distinct() {
    let config = load_config();
    let dev = config.build.dev_path.to_string();
    let dist = config.build.dist_dir.to_string();
    assert_ne!(
        dev, dist,
        "开发模式 URL 与生产 distDir 应不同（否则 dev 模式加载不到本地产物）"
    );
    assert!(
        Path::new(&dist).is_absolute() || dist.starts_with(".."),
        "distDir 应是相对 crate 根的路径: {dist}"
    );
}
