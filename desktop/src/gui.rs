//! Tauri 接线层（`gui` 特性）
//!
//! 每个 `#[tauri::command]` 只做三件事：从 `AppHandle`/`State` 取平台句柄或状态 →
//! 委派框架层 → 把 `Result` 的错误映射给前端。**命令体里没有业务判断**：参数校验、
//! 配置自举、批量语义、错误文案全在框架层（有 Linux 可跑的单测），此处只剩
//! `.map_err(|e| e.to_string())` 这类映射。
//!
//! 这样切分的原因是可验证性：接线层只有 `gui` 特性（拉入 `tauri` 依赖）才编译，
//! 而 Tauri 1.x 在 Linux 需要 WebKitGTK 系统库——所以这段代码历史上只能由 CI 的
//! macOS 作业编译，签名漂移要等一次推送后才暴露（首轮就是这样撞上把
//! `Result<(), Vec<String>>` 当 `Vec<String>` 用的编译错误）。命令体越薄，留在
//! macOS 作业里、无法本地验证的代码量越少。
//!
//! 命令清单与 web 前端调用方一一对应（见 web/app.js 与
//! `web/dist/desktop-shell.js` 的 `invoke`；一致性由
//! `desktop/tests/wiring_contract.rs` 锁定）。

use crate::alerts;
use crate::analysis;
use crate::app;
use crate::error::DesktopError;
use crate::export;
use crate::ipc::{AnalyzeRequest, ExportRequest, InitPayload, NotificationRequest};
use crate::notify::{self, Notification, TrayState};
use crate::state::AppState;
use alpha_core::models::{AnalysisResult, MarketData};
use chrono::Utc;
use tauri::{Manager, State};

/// 从运行时读取反向域名标识符（`tauri.conf.json` 的 `tauri.bundle.identifier`）
///
/// Tauri 1.x 的 Config 带 `deny_unknown_fields` 且 identifier 只有一个（v2 才是
/// 标识符数组），故这里按单值取；框架层零 Tauri 依赖，只能由本层注入。
fn identifier_from(app: &tauri::AppHandle) -> String {
    app.config().tauri.bundle.identifier.clone()
}

/// 初始化应用：解析平台目录 → 框架层自举配置/建目录 → 注入状态
#[tauri::command]
async fn initialize_app(app_handle: tauri::AppHandle) -> Result<InitPayload, String> {
    let config_dir = app_handle
        .path_resolver()
        .app_config_dir()
        .ok_or("无法获取配置目录")?;
    let data_dir = app_handle
        .path_resolver()
        .app_data_dir()
        .ok_or("无法获取数据目录")?;

    let (state, payload) = crate::state::bootstrap_app(&config_dir, &data_dir)
        .map_err(|e: DesktopError| e.to_string())?;
    app_handle.manage(state);

    Ok(payload)
}

/// 分析单个标的
#[tauri::command]
async fn analyze_symbol(
    request: AnalyzeRequest,
    state: State<'_, AppState>,
) -> Result<AnalysisResult, String> {
    tracing::debug!(
        symbol = %request.symbol,
        timeframe = %request.timeframe,
        indicators = request.indicators.len(),
        "收到分析请求"
    );
    analysis::analyze_request(state.engine(), &request)
        .await
        .map_err(|e| e.to_string())
}

/// 批量快照行情
#[tauri::command]
async fn get_real_time_quotes(symbols: Vec<String>) -> Result<Vec<MarketData>, String> {
    analysis::quotes_request(&symbols).map_err(|e| e.to_string())
}

/// 新增价格告警，返回告警 id
#[tauri::command]
async fn set_price_alert(
    symbol: String,
    target_price: f64,
    alert_type: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    alerts::upsert_request(
        &state.paths().alerts_file(),
        &symbol,
        target_price,
        &alert_type,
        Utc::now(),
    )
    .map_err(|e: DesktopError| e.to_string())
}

/// 导出数据，返回导出的文件名列表
#[tauri::command]
async fn export_data(
    request: ExportRequest,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    export::export_request(&request, &state.paths().exports_dir(), Utc::now())
        .map_err(|e: DesktopError| e.to_string())
}

/// 导出单个标的到用户自选路径（L113 原生「另存为」链路：前端经 `dialog.save`
/// 拿到路径后传入；格式解析/标的校验/后缀一致性全在框架层 `export_symbol_request`）
#[tauri::command]
async fn export_symbol_to_file(
    symbol: String,
    format: String,
    file_path: String,
) -> Result<crate::export::ExportOutcome, String> {
    export::export_symbol_request(&symbol, &format, std::path::Path::new(&file_path))
        .map_err(|e: DesktopError| e.to_string())
}

/// 应用信息
#[tauri::command]
async fn get_app_info(app_handle: tauri::AppHandle) -> Result<app::AppInfo, String> {
    Ok(app::app_info(identifier_from(&app_handle)))
}

/// 发送系统通知（L114）：框架层校验+入队，接线层只负责平台 API 展示
#[tauri::command]
async fn send_notification(
    app_handle: tauri::AppHandle,
    request: NotificationRequest,
    state: State<'_, AppState>,
) -> Result<Notification, String> {
    let notification = {
        let mut queue = state.notification_queue().lock().expect("通知队列锁中毒");
        notify::notify_request(&request, &mut queue, Utc::now())
            .map_err(|e: DesktopError| e.to_string())?
    };
    // identifier 是应用标识（bundle id）；平台胶水收拢在 platform.rs
    crate::platform::show_notification(&identifier_from(&app_handle), &notification)?;
    Ok(notification)
}

/// 最近通知（L114）：框架层队列读取，接线层只透传
#[tauri::command]
async fn list_notifications(
    state: State<'_, AppState>,
    limit: usize,
) -> Result<Vec<Notification>, String> {
    Ok(state
        .notification_queue()
        .lock()
        .expect("通知队列锁中毒")
        .recent(limit))
}

/// 更新托盘状态（L114）：框架层由告警集合算状态文本，接线层只负责 set_tooltip
#[tauri::command]
async fn set_tray_status(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<TrayState, String> {
    let status = notify::tray_status_request(&state.paths().alerts_file());
    if let Some(tray) = app_handle.tray_handle_by_id(notify::TRAY_ID) {
        tray.set_tooltip(&status.status_text)
            .map_err(|e| e.to_string())?;
    }
    Ok(status)
}

/// 检查告警并弹出触发通知（L114 闭环）：框架层判定/入队/停用落盘
/// （`check_request`），接线层只负责平台通知展示；返回本次触发的通知
#[tauri::command]
async fn check_alerts(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<Notification>, String> {
    let fired = {
        let mut queue = state.notification_queue().lock().expect("通知队列锁中毒");
        notify::check_request(&state.paths().alerts_file(), &mut queue, Utc::now())
            .map_err(|e: DesktopError| e.to_string())?
    };
    crate::platform::show_notifications(&identifier_from(&app_handle), &fired);
    Ok(fired)
}

/// 离线感知行情读取（L116）：探测 → 在线拉取+写穿落库 / 离线读缓存，
/// 降级矩阵（在线拉取失败落缓存、缓存缺失入 missing）全在框架层 `quotes_request`
#[tauri::command]
async fn get_offline_quotes(
    symbols: Vec<String>,
    state: State<'_, AppState>,
) -> Result<crate::offline::QuotesPayload, String> {
    crate::offline::quotes_request(
        state.kv(),
        state.remote().as_ref(),
        &symbols,
        state.api_url(),
        Utc::now(),
    )
    .await
    .map_err(|e: DesktopError| e.to_string())
}

/// 联网恢复增量同步（L116）：内容指纹比对只落变更，离线返回空报告（降级非错误），
/// 语义全在框架层 `sync_request`；同步范围由前端传观察列表（与 `get_real_time_quotes`
/// 同构，配置已在 `initialize_app` 下行）
#[tauri::command]
async fn sync_offline_data(
    symbols: Vec<String>,
    state: State<'_, AppState>,
) -> Result<crate::offline::SyncReport, String> {
    crate::offline::sync_request(
        state.kv(),
        state.remote().as_ref(),
        &symbols,
        state.api_url(),
        Utc::now(),
    )
    .await
    .map_err(|e: DesktopError| e.to_string())
}

/// 右键菜单模型（L117）：菜单项/文案/可用性/加速器提示全在框架层
/// `context_menu`（含 macOS 与其它平台的提示差异），接线层只透传壳层上报的
/// 界面状态；纯模型构造无失败路径，与 `get_app_info` 同不映射错误
#[tauri::command]
async fn get_context_menu(
    has_quote: bool,
    has_symbols: bool,
) -> Result<Vec<crate::shortcuts::ContextMenuItem>, String> {
    Ok(crate::shortcuts::context_menu(has_quote, has_symbols))
}

/// 启动 Tauri 应用（进程入口，供 `main.rs` 调用）
pub fn run() {
    tracing_subscriber::fmt::init();
    tauri::Builder::default()
        .setup(|app| {
            let info = app::app_info(identifier_from(&app.handle()));
            tracing::info!(?info, "桌面应用启动");
            // L115：恢复上次会话窗口几何（含显示器钳制），失败静默走 OS 默认
            crate::window_gui::restore_window(app);
            // L117：注册全局快捷键（组合键被占用等失败在接线层降级为告警）
            crate::shortcut_gui::register_global_shortcuts(&app.handle());
            Ok(())
        })
        .system_tray(crate::platform::system_tray())
        .on_system_tray_event(crate::platform::on_tray_event)
        .on_window_event(crate::window_gui::on_window_event)
        .invoke_handler(tauri::generate_handler![
            initialize_app,
            analyze_symbol,
            get_real_time_quotes,
            set_price_alert,
            export_data,
            export_symbol_to_file,
            get_app_info,
            send_notification,
            list_notifications,
            set_tray_status,
            check_alerts,
            get_offline_quotes,
            sync_offline_data,
            get_context_menu
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
