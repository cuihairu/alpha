//! 窗口几何恢复/持久化接线（`gui` 特性；L115）
//!
//! 与 [`crate::platform`] 同口径：**放置决策全在框架层** [`crate::window`]
//! （清洗、跨显示器钳制、节流判定），本模块只把 Tauri 平台读数翻译成
//! [`crate::window::MonitorRect`]/[`crate::window::WindowGeometry`] 并执行
//! 平台调用（`set_position`/`set_size`/`maximize`）。任何一步拿不到（无窗口、
//! 无目录、读数失败、状态未注入）都静默跳过——窗口管理是体验优化，不允许
//! 因为它阻断启动或报错弹窗。
//!
//! 验证边界：单测覆盖不了平台调用——类型用法由 `check-desktop.sh` [5/5] 假
//! pkg-config 在 Linux 门禁类型检查，链接与运行由 CI Desktop (macOS) 作业验证；
//! 行数上限由 `wiring_contract.rs` 锁定。

use crate::state::AppState;
use crate::window;
use crate::{notify, paths};
use tauri::{GlobalWindowEvent, Manager, Runtime};

/// 启动恢复（`Builder::setup` 用）：读上次会话的窗口几何 → 框架层
/// `resolve_placement` 决策（含显示器拔出后的钳制）→ 平台调用还原
pub fn restore_window<R: Runtime>(app: &tauri::App<R>) {
    let Some(window) = app.get_window(notify::MAIN_WINDOW_LABEL) else {
        return;
    };
    let Some(config_dir) = app.path_resolver().app_config_dir() else {
        return;
    };
    let saved = window::load_window_state(&config_dir.join(paths::WINDOW_STATE_FILE_NAME));
    let monitors = current_monitors(&window);
    let Some(target) = window::resolve_placement(saved, &monitors) else {
        return;
    };
    restore_target(&window, &target, &monitors);
}

/// 应用还原目标（尺寸先行、位置收尾，最终位置为准）
///
/// 最大化：先按框架层落屏提示 [`window::maximized_position_hint`] 定位再
/// `maximize`——否则 maximize 只填 OS 默认放置的那块屏，记忆显示器不生效
fn restore_target<R: Runtime>(
    window: &tauri::Window<R>,
    target: &window::WindowGeometry,
    monitors: &[window::MonitorRect],
) {
    if target.maximized {
        if let Some((x, y)) = window::maximized_position_hint(target, monitors) {
            let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
        }
        let _ = window.maximize();
        return;
    }
    let _ = window.set_size(tauri::PhysicalSize::new(target.width, target.height));
    let _ = window.set_position(tauri::PhysicalPosition::new(target.x, target.y));
}

/// 枚举当前显示器并让主屏排首（`resolve_placement` 把 `monitors[0]` 当钳制目标）
fn current_monitors<R: Runtime>(window: &tauri::Window<R>) -> Vec<window::MonitorRect> {
    let rect = |m: &tauri::Monitor| window::MonitorRect {
        x: m.position().x,
        y: m.position().y,
        width: m.size().width,
        height: m.size().height,
        name: m.name().cloned(),
    };
    let mut monitors: Vec<window::MonitorRect> = window
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(rect)
        .collect();
    if let Ok(Some(primary)) = window.primary_monitor() {
        let primary = rect(&primary);
        if let Some(index) = monitors.iter().position(|m| m == &primary) {
            let found = monitors.remove(index);
            monitors.insert(0, found);
        }
    }
    monitors
}

/// 采集当前窗口几何；`outer_position`/`inner_size` 任一失败 → `None`。
/// `is_maximized` 失败按 `false` 处理（保守回退：只影响恢复方式，不影响落盘）。
/// 显示器名取 `current_monitor`（窗口所在屏），供下次恢复「记忆上次所用显示器」。
fn current_geometry<R: Runtime>(window: &tauri::Window<R>) -> Option<window::WindowGeometry> {
    let position = window.outer_position().ok()?;
    let size = window.inner_size().ok()?;
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .and_then(|m| m.name().cloned());
    Some(window::WindowGeometry {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
        maximized: window.is_maximized().unwrap_or(false),
        monitor,
    })
}

/// 窗口事件入口（`Builder::on_window_event` 用）：移动/缩放走节流落盘，
/// 关闭请求走强制兜底落盘；其余事件忽略
pub fn on_window_event<R: Runtime>(event: GlobalWindowEvent<R>) {
    match event.event() {
        tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
            persist(event.window(), false);
        }
        tauri::WindowEvent::CloseRequested { .. } => persist(event.window(), true),
        _ => {}
    }
}

/// 采集 + 框架层节流判定 + 原子落盘（拿不到状态/读数则静默跳过）
fn persist<R: Runtime>(window: &tauri::Window<R>, force: bool) {
    let Some(geometry) = current_geometry(window) else {
        return;
    };
    let Some(state) = window.try_state::<AppState>() else {
        return;
    };
    let decision = {
        let mut tracker = state.window_tracker().lock().expect("窗口节流锁中毒");
        let at = chrono::Utc::now();
        if force {
            tracker.flush(at, geometry)
        } else {
            tracker.observe(at, geometry)
        }
    };
    if let Some(geometry) = decision {
        let path = state.paths().window_state_file();
        if let Err(e) = window::save_window_state(&path, &geometry) {
            tracing::warn!(error = %e, path = %path.display(), "窗口状态落盘失败");
        }
    }
}
