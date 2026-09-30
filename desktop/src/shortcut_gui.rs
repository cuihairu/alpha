//! 全局快捷键接线（`gui` 特性；L117）
//!
//! 与 [`crate::window_gui`] 同口径：**表与判定全在框架层**
//! [`crate::shortcuts`]，本模块只把默认表注册进 Tauri 的
//! `GlobalShortcutManager`，触发时向所有窗口广播 [`SHORTCUT_EVENT`]（载荷 =
//! 动作 id），动作分发在壳层 `ACTIONS` 表（复用既有命令流）。
//!
//! 注册失败（组合键被系统或其它应用占用）只 `tracing::warn` 并跳过该键：
//! 快捷键是体验增强，不允许阻断启动或弹错误窗——降级为无此快捷键，右键
//! 菜单与按钮路径不受影响。
//!
//! 验证边界：单测覆盖不了平台注册——类型用法由 `check-desktop.sh` [5/5] 假
//! pkg-config 在 Linux 门禁类型检查，链接与运行由 CI Desktop (macOS) 作业验证；
//! 行数上限由 `wiring_contract.rs` 锁定。

use crate::shortcuts;
use tauri::{GlobalShortcutManager, Manager};

/// 快捷键事件名（壳层 `event.listen("shortcut", …)` 对应；载荷为动作 id）
pub const SHORTCUT_EVENT: &str = "shortcut";

/// 注册默认快捷键表（`Builder::setup` 用）：逐键注册，触发时广播
/// [`SHORTCUT_EVENT`]。单键注册失败只告警降级，其余键继续。
pub fn register_global_shortcuts(app: &tauri::AppHandle) {
    let mut manager = app.global_shortcut_manager();
    for spec in shortcuts::DEFAULT_SHORTCUTS {
        let handle = app.clone();
        if let Err(e) = manager.register(spec.combo, move || {
            let _ = handle.emit_all(SHORTCUT_EVENT, spec.action);
        }) {
            tracing::warn!(
                combo = spec.combo,
                action = spec.action,
                error = %e,
                "全局快捷键注册失败，降级为无此快捷键"
            );
        }
    }
}
