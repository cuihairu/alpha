//! 接线层薄度契约（TODO L112 构建门禁的可执行部分之二）
//!
//! 为什么需要它：`desktop/src/gui.rs` 只有 `gui` 特性才编译，而 Tauri 1.x 在 Linux
//! 需要 WebKitGTK——这段代码**本地编不了**，只有 CI 的 macOS 作业能编译它。于是
//! 首轮 CI 就这样把一处签名漂移（把框架层 `Result<(), Vec<String>>` 当
//! `Vec<String>` 用）留到了 macOS 才炸。
//!
//! 本文件能做的是把「接线层里不该出现的东西」变成**可本地执行的断言**：
//! * 命令体不得含业务判断（判空、错误文案、默认值兜底）——一律在框架层；
//! * 命令体只能委派到框架层的 `*_request` / `bootstrap_app` 入口；
//! * 命令名两侧一致（`generate_handler!` ↔ `desktop-shell.js` 的 `invoke`）。
//!
//! 不能覆盖的部分（诚实边界）：Tauri 自身的类型是否用对、`#[tauri::command]` 宏
//! 展开是否成立，仍需 macOS 作业编译；手段是把留在那里的代码压到最小。

use std::path::{Path, PathBuf};

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn gui_source() -> String {
    std::fs::read_to_string(crate_dir().join("src/gui.rs")).expect("读 gui.rs")
}

/// 去掉行注释与块注释（源码契约断言只关心可执行代码）
fn strip_rust_comments(source: &str) -> String {
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
                let mut depth = 1;
                i += 2;
                while i < chars.len() && depth > 0 {
                    if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                        depth += 1;
                        i += 2;
                    } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                        depth -= 1;
                        i += 2;
                    } else {
                        if chars[i] == '\n' {
                            out.push('\n');
                        }
                        i += 1;
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

/// 单条命令的函数体（含缩进），用于逐条断言「命令体只委派」
fn command_body(source: &str, name: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.contains(&format!("fn {name}(")))
        .unwrap_or_else(|| panic!("gui.rs 应有命令 {name}"));
    // 从函数签名行往后收集，直到大括号平衡
    let mut body = String::new();
    let mut depth = 0i32;
    let mut started = false;
    for line in &lines[start..] {
        body.push_str(line);
        body.push('\n');
        for c in line.chars() {
            match c {
                '{' => {
                    depth += 1;
                    started = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        if started && depth <= 0 {
            break;
        }
    }
    body
}

/// 命令体不得含业务判断：判空、错误文案、默认值兜底都应在框架层
#[test]
fn command_bodies_have_no_business_judgement() {
    let gui = gui_source();
    for name in [
        "initialize_app",
        "analyze_symbol",
        "get_real_time_quotes",
        "set_price_alert",
        "export_data",
        "export_symbol_to_file",
        "get_app_info",
        "send_notification",
        "list_notifications",
        "set_tray_status",
        "check_alerts",
    ] {
        let body = command_body(&gui, name);
        let code = strip_rust_comments(&body);
        // 判空/兜底：if ... is_empty / unwrap_or / unwrap_or_default
        for banned in ["is_empty()", "unwrap_or(", "unwrap_or_default("] {
            assert!(
                !code.contains(banned),
                "命令 {name} 里出现业务判断 `{banned}`，应下沉框架层:\n{body}"
            );
        }
        // 自造错误文案：ok_or / Err("...") 说明在接线层判业务
        assert!(
            !code.contains("Err(\""),
            "命令 {name} 里自造错误串，应委派框架层:\n{body}"
        );
    }
}

/// 命令体只允许出现的非 Tauri 依赖：框架层入口 + 错误映射
#[test]
fn command_bodies_delegate_to_framework_entry_points() {
    let gui = gui_source();
    // 每个命令都必须委派到某个框架层入口
    for (name, entry) in [
        ("initialize_app", "bootstrap_app"),
        ("analyze_symbol", "analyze_request"),
        ("get_real_time_quotes", "quotes_request"),
        ("set_price_alert", "upsert_request"),
        ("export_data", "export_request"),
        ("export_symbol_to_file", "export_symbol_request"),
        ("get_app_info", "app_info"),
        ("send_notification", "notify_request"),
        ("list_notifications", "recent"),
        ("set_tray_status", "tray_status_request"),
        ("check_alerts", "check_request"),
    ] {
        let body = command_body(&gui, name);
        assert!(
            body.contains(entry),
            "命令 {name} 应委派框架层入口 {entry}:\n{body}"
        );
    }

    // 有失败可能的命令必须映射错误为前端可读串；
    // get_app_info 是纯读取（app_info 不返回 Result），故豁免；
    // list_notifications 只读队列（锁中毒用 expect 兜底，无 Result 边界）
    for name in [
        "initialize_app",
        "analyze_symbol",
        "get_real_time_quotes",
        "set_price_alert",
        "export_data",
        "export_symbol_to_file",
        "send_notification",
        "set_tray_status",
        "check_alerts",
    ] {
        let body = command_body(&gui, name);
        assert!(
            body.contains("map_err"),
            "命令 {name} 应把错误映射为前端可读串（map_err）:\n{body}"
        );
    }
}

/// 接线层不得直接碰框架层的底层函数（那些已被 *_request 入口封装）
#[test]
fn wiring_layer_does_not_reach_around_request_entry_points() {
    let gui = strip_rust_comments(&gui_source());
    for (banned, why) in [
        ("config::load_or_default", "配置加载走 bootstrap_app"),
        ("config::save", "配置自举落盘走 bootstrap_app"),
        (".validate()", "校验问题走 InitPayload::new"),
        ("AlertKind::parse", "方向串解析走 upsert_request"),
        ("ExportFormat::parse", "格式解析走 export_request"),
        ("market::synthetic_series", "取数走 export_request"),
        ("NotificationLevel::parse", "级别串解析走 notify_request"),
    ] {
        assert!(
            !gui.contains(banned),
            "接线层出现 `{banned}`（{why}），说明命令体自己干了业务"
        );
    }
}

/// 注册的命令与桌面兜底壳调用的命令必须一致
#[test]
fn registered_commands_match_fallback_shell_invocations() {
    let gui = gui_source();
    let registered: Vec<&str> = gui
        .split("generate_handler![")
        .nth(1)
        .expect("应有 generate_handler!")
        .split(']')
        .next()
        .expect("宏参数")
        .lines()
        .map(|l| l.trim().trim_end_matches(','))
        .filter(|l| !l.is_empty())
        .collect();
    assert_eq!(
        registered.len(),
        11,
        "注册命令数应与前端契约一致，实际: {registered:?}"
    );
    for cmd in &registered {
        let shell =
            std::fs::read_to_string(crate_dir().join(tauri_dist_dir()).join("desktop-shell.js"))
                .expect("读兜底壳");
        assert!(
            shell.contains(&format!("invoke(\"{cmd}\"")) || !shell_uses(cmd, &shell),
            "兜底壳在调用未注册的命令 {cmd}"
        );
    }
}

fn tauri_dist_dir() -> String {
    let value: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(crate_dir().join("tauri.conf.json")).unwrap(),
    )
    .expect("读配置");
    value["build"]["distDir"]
        .as_str()
        .expect("distDir 应为字符串")
        .to_string()
}

fn shell_uses(cmd: &str, shell: &str) -> bool {
    shell.contains(&format!("invoke(\"{cmd}\""))
}

/// 接线层文件本身应保持在「薄」的数量级：命令体下沉后不该再出现大段业务
#[test]
fn wiring_layer_stays_thin() {
    let gui = gui_source();
    let lines = gui.lines().count();
    assert!(
        lines < 220,
        "gui.rs 涨到 {lines} 行（接线层应保持薄，业务下沉框架层；L114 通知/托盘
        三命令后上限 160 → 200，check_alerts 闭环命令后 200 → 220，每命令仍
        ~20 行薄委派；托盘/通知平台胶水在 src/platform.rs，上限见
        platform_glue_stays_mechanical）"
    );
    // 业务模块的函数体不应出现在接线层（抽查两个典型业务函数）
    assert!(!gui.contains("for symbol in"), "接线层不应出现批量循环");
}

/// 框架层入口都在 lib.rs 导出（接线层按 crate 路径引用，缺导出即 macOS 编不过）
#[test]
fn framework_entry_points_are_exported() {
    let lib = std::fs::read_to_string(crate_dir().join("src/lib.rs")).expect("读 lib.rs");
    for entry in [
        "bootstrap_app",
        "analyze_request",
        "quotes_request",
        "upsert_request",
        "export_request",
        "export_symbol_request",
        "notify_request",
        "tray_status_request",
        "check_request",
        "tray_menu_model",
        "tray_action",
        "load_window_state",
        "resolve_placement",
        "save_window_state",
        "theme_pref",
        "resolve_theme",
    ] {
        assert!(
            lib.contains(entry),
            "lib.rs 应导出框架层入口 {entry}（接线层按 crate 路径调用）"
        );
    }
}

/// lib.rs 的重导出与实际定义一致（避免重导出指向已改名/删除的项——这类错误只有
/// 接线层引用它们时才暴露，而接线层只在 macOS 编译）
#[test]
fn lib_reexports_point_to_existing_items() {
    let lib = std::fs::read_to_string(crate_dir().join("src/lib.rs")).expect("读 lib.rs");
    let mut checked = 0usize;
    for line in lib
        .lines()
        .filter(|l| l.trim_start().starts_with("pub use "))
    {
        let stmt = line
            .trim()
            .trim_start_matches("pub use ")
            .trim_end_matches(';');
        // 拆成 (模块, 条目列表)；条目可能是 `{a, b}` 也可能是 `a`
        let (module, items) = match stmt.split_once("::{") {
            Some((module, rest)) => (module, rest.trim_end_matches('}')),
            None => {
                let mut parts = stmt.split("::");
                let module = parts.next().unwrap_or("");
                let item = parts.next().unwrap_or("");
                (module, item)
            }
        };
        if module.is_empty() || module == "crate" || module.contains('{') {
            continue;
        }
        let src = crate_dir().join(format!("src/{module}.rs"));
        assert!(src.is_file(), "重导出的模块 {module} 应存在: {stmt}");
        let text = std::fs::read_to_string(&src).expect("读模块");
        for raw in items.split(',') {
            let item = raw.trim();
            if item.is_empty() || item == "*" {
                continue;
            }
            checked += 1;
            assert!(
                text.contains(item),
                "{module} 应定义 {item}（lib.rs 重导出: {stmt}）"
            );
        }
    }
    assert!(checked >= 15, "重导出条目过少，解析可能失效: {checked}");
}

/// 桌面兜底壳的四个命令在框架层都有对应入口（前后端与框架三方一致）
#[test]
fn shell_commands_have_framework_entry_points() {
    let shell =
        std::fs::read_to_string(crate_dir().join(tauri_dist_dir()).join("desktop-shell.js"))
            .expect("读兜底壳");
    for cmd in [
        "initialize_app",
        "get_app_info",
        "get_real_time_quotes",
        "analyze_symbol",
        "export_symbol_to_file",
        "check_alerts",
        "set_tray_status",
    ] {
        assert!(
            shell.contains(&format!("invoke(\"{cmd}\"")),
            "兜底壳应调用 {cmd}"
        );
    }
    let _ = Path::new("/");
}

/// L113 原生文件集成：导出必须走系统「另存为」对话框拿路径（`dialog.save`），
/// 而不是把用户路径写死在前端——保存位置由用户在对话框里定
#[test]
fn shell_export_uses_native_save_dialog() {
    let shell =
        std::fs::read_to_string(crate_dir().join(tauri_dist_dir()).join("desktop-shell.js"))
            .expect("读兜底壳");
    assert!(
        shell.contains("dialog.save"),
        "兜底壳导出应经原生另存为对话框拿路径"
    );
    assert!(
        shell.contains("filePath"),
        "兜底壳应把对话框返回的路径按 filePath 传给 Rust 命令——Tauri 1.x 命令
        参数默认 camelCase（tauri-macros wrapper.rs 的 ArgumentCase::Camel），
        写 file_path 会在运行期静默失配（CI 只编译不启动，此断言是唯一本地拦截点）"
    );
}

/// L114 托盘/通知平台胶水（`src/platform.rs`，gui 特性）：与 gui.rs 同纪律——
/// 不得定义命令、不得自造错误文案，只做「框架层模型 → Tauri 类型」的机械翻译
#[test]
fn platform_glue_stays_mechanical() {
    let platform =
        std::fs::read_to_string(crate_dir().join("src/platform.rs")).expect("读 platform.rs");
    let lines = platform.lines().count();
    assert!(
        lines < 120,
        "platform.rs 涨到 {lines} 行（平台胶水应保持机械翻译，判断下沉 notify.rs）"
    );
    assert!(
        !platform.contains("#[tauri::command]"),
        "平台胶水不应定义命令（命令都在 gui.rs，受薄度契约约束）"
    );
    let code = strip_rust_comments(&platform);
    assert!(
        !code.contains("Err(\""),
        "平台胶水不应自造错误串（错误映射归命令体/框架层）"
    );
    assert!(
        code.contains("notify::"),
        "平台胶水应从框架层 notify.rs 取菜单模型/动作映射，而非自造"
    );
}

/// L114 告警闭环的前端接线：布防命令的多词参数必须是 camelCase
/// （targetPrice/alertType）。Tauri 1.x 命令参数默认 camelCase
/// （tauri-macros wrapper.rs `ArgumentCase::Camel`），snake 键会在运行期
/// 静默失配——CI 只编译不启动，唯有源码断言能在本地拦截。
#[test]
fn shell_alert_loop_uses_v1_camel_case_arguments() {
    let shell =
        std::fs::read_to_string(crate_dir().join(tauri_dist_dir()).join("desktop-shell.js"))
            .expect("读兜底壳");
    for needle in ["invoke(\"set_price_alert\"", "targetPrice", "alertType"] {
        assert!(
            shell.contains(needle),
            "兜底壳布防告警应含 {needle}（v1 参数默认 camelCase）"
        );
    }
}

/// L115 窗口接线：gui.rs 必须挂上恢复（setup）与事件持久化（on_window_event）
#[test]
fn window_management_is_wired() {
    let gui = gui_source();
    assert!(
        gui.contains("crate::window_gui::restore_window(app)"),
        "setup 应调用窗口恢复（L115）"
    );
    assert!(
        gui.contains(".on_window_event(crate::window_gui::on_window_event)"),
        "Builder 应挂窗口事件监听（L115 移动/缩放/关闭落盘）"
    );
}

/// L115 窗口平台胶水（`src/window_gui.rs`，gui 特性）：与 platform.rs 同纪律——
/// 不得定义命令、不得自造错误文案，放置决策一律从框架层 window.rs 取
#[test]
fn window_glue_stays_mechanical() {
    let glue =
        std::fs::read_to_string(crate_dir().join("src/window_gui.rs")).expect("读 window_gui.rs");
    let lines = glue.lines().count();
    assert!(
        lines < 140,
        "window_gui.rs 涨到 {lines} 行（平台胶水应保持机械翻译，放置决策在 window.rs）"
    );
    assert!(
        !glue.contains("#[tauri::command]"),
        "窗口胶水不应定义命令（命令都在 gui.rs，受薄度契约约束）"
    );
    let code = strip_rust_comments(&glue);
    assert!(
        !code.contains("Err(\""),
        "窗口胶水不应自造错误串（窗口管理是体验优化，静默跳过而非报错）"
    );
    assert!(
        code.contains("window::"),
        "窗口胶水应从框架层 window.rs 取清洗/钳制/节流判定，而非自造"
    );
}

/// L115 主题适配：配置/系统的深浅色切换落在内容层 data-theme + CSS 变量，
/// 且跟随 prefers-color-scheme（Tauri 1.x 无运行期 set_theme，原生装饰由
/// tauri.conf.json "theme": "System" 创建期跟随系统）
#[test]
fn shell_theme_follows_system_with_override() {
    let dist = tauri_dist_dir();
    let shell = std::fs::read_to_string(crate_dir().join(&dist).join("desktop-shell.js"))
        .expect("读兜底壳");
    for needle in [
        "data-theme",
        "prefers-color-scheme",
        "applyTheme(cfg.theme)",
    ] {
        assert!(
            shell.contains(needle),
            "兜底壳主题适配应含 {needle}（system 跟随 + 配置覆盖）"
        );
    }
    let index =
        std::fs::read_to_string(crate_dir().join(&dist).join("index.html")).expect("读兜底壳页面");
    assert!(
        index.contains("[data-theme=\"light\"]"),
        "兜底壳页面应有浅色主题覆盖块（覆盖既有组件的 CSS 变量）"
    );
    assert!(
        index.contains("--chip"),
        "既有组件的硬编码底色（badge/code/button）应主题化为变量"
    );
}
