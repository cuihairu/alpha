//! Alpha Finance 桌面端框架层（TODO.md L112「搭建 Tauri + Rust 桌面应用框架」）
//!
//! 分层（与 docs/cross-platform-architecture.md 的 L2 表现层一致，桌面内部再分两半）：
//!
//! * **框架层（本 crate 的 lib，零 Tauri 依赖）**：目录布局、配置读写、键值存储、
//!   行情/分析编排、本地导出、告警存储、IPC 请求 DTO、错误类型。纯 Rust + std +
//!   alpha-core，可在任意 CI 编译与测试（Linux 无需 WebKitGTK 系统库）。
//! * **接线层（[`gui`]，Tauri 依赖，`gui` 特性门控）**：`#[tauri::command]` 包装、
//!   `Builder`/`generate_context!`、窗口与托盘配置。命令体只做「路径解析 + 委派
//!   框架层」，不写业务逻辑——因此 macOS CI 作业覆盖的仅是极薄的接线代码。
//!
//! 为什么门控 Tauri：Tauri 1.x 在 Linux 链接 WebKitGTK/libsoup 系统库，无法在
//! 无 GUI 依赖的 CI 编译。把 Tauri 设为可选依赖后，框架层进入主测试/规范门禁，
//! GUI 接线仍由 macOS 作业（`cargo test -p alpha-desktop --all-targets`）验证。
//! 详见 docs/desktop-framework.md。
//!
//! 边界：L112 只落「框架」。L113 原生文件集成（用户自选路径导出
//! [`export::export_to_file`]/[`export::export_symbol_request`] + `dialog.save`
//! 链路）与 L114 系统通知/托盘（[`notify`]：通知模型/有界队列/托盘状态/
//! 告警触发文案 + 接线层平台 API 展示）均已落地；平台 API 调用只在接线层。

#![warn(missing_docs)]

pub mod alerts;
pub mod analysis;
pub mod app;
pub mod config;
pub mod error;
pub mod export;
pub mod ipc;
pub mod kv;
pub mod market;
pub mod notify;
pub mod paths;
pub mod state;

#[cfg(feature = "gui")]
pub mod gui;

/// 平台胶水（托盘接线/通知展示；`gui` 特性门控——单测覆盖不了平台调用，
/// 由 check-desktop [5/5] 类型检查 + CI Desktop (macOS) 链接验证）
#[cfg(feature = "gui")]
pub mod platform;

pub use alerts::{upsert_request, Alert, AlertKind};
pub use analysis::{analyze, analyze_request, quotes, quotes_request};
pub use app::{app_info, AppInfo, APP_NAME};
pub use config::{load_or_default, AppConfig, ConfigSource};
pub use error::{DesktopError, DesktopResult};
pub use export::{
    export, export_request, export_symbol_request, export_to_file, ExportFormat, ExportOutcome,
};
pub use ipc::{AnalyzeRequest, ExportRequest, InitPayload};
pub use kv::FileKeyValueStore;
pub use market::{synthetic_quote, synthetic_series, DEFAULT_BARS};
pub use notify::{
    alert_notification, check_request, notification_id, notify_request, tray_action,
    tray_menu_model, tray_status_request, Notification, NotificationLevel, NotificationQueue,
    TrayAction, TrayEntry, TrayState, DEFAULT_QUEUE_CAPACITY,
};
pub use paths::AppPaths;
pub use state::{bootstrap_app, AppState};
