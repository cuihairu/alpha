//! Tauri 平台胶水（`gui` 特性；L114 托盘接线 + 通知展示）
//!
//! 与 [`crate::gui`] 同口径：**业务判断全在框架层** [`crate::notify`]（菜单模型、
//! id→动作映射、通知文案），本模块只把框架层模型机械翻译成 Tauri 类型并执行
//! 平台调用（窗口显示/隐藏、退出进程、`Notification::show`、`set_tooltip`）。
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

/// 展示单条通知（`send_notification` 用：显式用户动作，失败要报给前端）
pub fn show_notification(identifier: &str, notification: &Notification) -> Result<(), String> {
    tauri::api::notification::Notification::new(identifier)
        .title(&notification.title)
        .body(&notification.body)
        .show()
        .map_err(|e| e.to_string())
}

/// 批量展示（`check_alerts` 触发链用：尽力而为，单条失败仅告警不阻断其余）
pub fn show_notifications(identifier: &str, notifications: &[Notification]) {
    for notification in notifications {
        if let Err(e) = show_notification(identifier, notification) {
            tracing::warn!(id = %notification.id, error = %e, "系统通知展示失败");
        }
    }
}
