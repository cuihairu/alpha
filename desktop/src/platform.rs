//! Tauri 平台胶水（`gui` 特性；L114 托盘接线 + 通知展示）
//!
//! 与 [`crate::gui`] 同口径：**业务判断全在框架层** [`crate::notify`]（菜单模型、
//! id→动作映射、通知文案、点击动作映射），本模块只把框架层模型机械翻译成 Tauri
//! 类型并执行平台调用（窗口显示/隐藏、退出进程、通知展示与点击监听、
//! `set_tooltip`）。
//!
//! 验证边界：单测覆盖不了平台调用——类型用法由 `check-desktop.sh` [5/5] 假
//! pkg-config 在 Linux 门禁类型检查，链接与运行由 CI Desktop (macOS) 作业验证；
//! 因此本文件保持「无判断」的翻译层，行数上限由 `wiring_contract.rs` 锁定。

use crate::notify::{self, Notification, TrayAction, TrayEntry};
use tauri::{
    AppHandle, CustomMenuItem, Manager, SystemTray, SystemTrayEvent, SystemTrayMenu,
    SystemTrayMenuItem,
};

/// 框架层菜单模型 → Tauri 托盘菜单（机械翻译；`enabled` 映射为可用态）
pub fn tray_menu(window_visible: bool) -> SystemTrayMenu {
    let mut menu = SystemTrayMenu::new();
    for entry in notify::tray_menu_model(window_visible) {
        menu = match entry {
            TrayEntry::Item { id, label, enabled } => {
                let item = CustomMenuItem::new(id, label);
                menu.add_item(if enabled { item } else { item.disabled() })
            }
            TrayEntry::Separator => menu.add_native_item(SystemTrayMenuItem::Separator),
        };
    }
    menu
}

/// 构建托盘（`Builder::system_tray` 用）。`with_id(TRAY_ID)` 必须显式：默认 id 是
/// 随机串，`tray_handle_by_id(TRAY_ID)` 会找不到句柄；图标为 None 时
/// `SystemTray::build` 回退到 tauri.conf.json `tauri.systemTray` 注入的图标。
pub fn system_tray() -> SystemTray {
    SystemTray::new()
        .with_id(notify::TRAY_ID)
        .with_menu(tray_menu(true))
}

/// 托盘事件分发（`Builder::on_system_tray_event` 用）：菜单点击 → 框架层
/// `tray_action` 映射 → 平台调用；显示/隐藏后按真实可见性回写菜单（状态机闭环）。
pub fn on_tray_event(app: &AppHandle, event: SystemTrayEvent) {
    if let SystemTrayEvent::MenuItemClick { id, .. } = event {
        match notify::tray_action(&id) {
            Some(TrayAction::Quit) => app.exit(0),
            Some(action) => {
                if let Some(window) = app.get_window(notify::MAIN_WINDOW_LABEL) {
                    match action {
                        TrayAction::ShowWindow => {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                        TrayAction::HideWindow => {
                            let _ = window.hide();
                        }
                        // 上面分支已处理，仅为穷尽
                        TrayAction::Quit => {}
                    }
                    if let Some(tray) = app.tray_handle_by_id(notify::TRAY_ID) {
                        // 可见性读取失败按隐藏处理（菜单回到「可显示」态，可恢复）
                        let visible = window.is_visible().unwrap_or(false);
                        let _ = tray.set_menu(tray_menu(visible));
                    }
                }
            }
            // 未知 id（非本应用菜单项）：忽略
            None => {}
        }
    }
}

/// 展示单条通知（`send_notification` 显式动作 / `check_alerts` 触发链共用）。
///
/// tauri 包装层的 `show()` 丢弃 notify-rust 句柄（tauri 1.x api/notification.rs
/// spawn 后弃返回值），点击事件拿不到——XDG 侧直用 notify-rust 注册
/// `default` 动作，点击通知本体经框架层 [`notify::notification_click_action`]
/// 唤起主窗（与托盘显示同路径）；`wait_for_action` 阻塞至通知关闭（zbus
/// block_on），监听放独立线程，进程退出即随之终结。非 XDG 平台无此回调面，
/// 维持 tauri 展示路径（边界登记 docs/desktop-framework.md §7）。
#[cfg(all(unix, not(target_os = "macos")))]
pub fn show_notification(
    app: &AppHandle,
    _identifier: &str,
    notification: &Notification,
) -> Result<(), String> {
    let handle = notify_rust::Notification::new()
        .summary(&notification.title)
        .body(&notification.body)
        .auto_icon()
        .action(notify::NOTIFY_ACTION_OPEN, "打开主窗口")
        .show()
        .map_err(|e| e.to_string())?;
    let app = app.clone();
    std::thread::spawn(move || {
        handle.wait_for_action(move |action| {
            if let Some(TrayAction::ShowWindow) = notify::notification_click_action(action) {
                if let Some(window) = app.get_window(notify::MAIN_WINDOW_LABEL) {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        });
    });
    Ok(())
}

/// macOS/Windows 展示（无点击回调面，见 [`show_notification`] doc）
#[cfg(any(target_os = "macos", windows))]
pub fn show_notification(
    _app: &AppHandle,
    identifier: &str,
    notification: &Notification,
) -> Result<(), String> {
    tauri::api::notification::Notification::new(identifier)
        .title(&notification.title)
        .body(&notification.body)
        .show()
        .map_err(|e| e.to_string())
}

/// 覆盖确认（L113 收尾）：原生 yes/no 阻塞对话框。
///
/// 文案（标题/提示）由框架层 [`crate::export::OVERWRITE_TITLE`] /
/// [`crate::export::overwrite_prompt`] 定稿，本函数只做平台调用；调用方是
/// **async 命令**（跑在 tauri 异步运行时，非主线程），故用 blocking 变体——
/// 主线程上下文（`App::run` 闭包、非 async 命令）禁用，见 tauri dialog 文档。
pub fn confirm_overwrite(app: &AppHandle, dest: &std::path::Path) -> bool {
    let window = app.get_window(notify::MAIN_WINDOW_LABEL);
    tauri::api::dialog::blocking::ask(
        window.as_ref(),
        crate::export::OVERWRITE_TITLE,
        crate::export::overwrite_prompt(dest),
    )
}

/// 批量展示（`check_alerts` 触发链用：尽力而为，单条失败仅告警不阻断其余）
pub fn show_notifications(app: &AppHandle, identifier: &str, notifications: &[Notification]) {
    for notification in notifications {
        if let Err(e) = show_notification(app, identifier, notification) {
            tracing::warn!(id = %notification.id, error = %e, "系统通知展示失败");
        }
    }
}
