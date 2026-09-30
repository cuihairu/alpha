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
use crate::ipc::{AnalyzeRequest, ExportRequest, InitPayload};
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

/// 应用信息
#[tauri::command]
async fn get_app_info(app_handle: tauri::AppHandle) -> Result<app::AppInfo, String> {
    Ok(app::app_info(identifier_from(&app_handle)))
}

/// 启动 Tauri 应用（进程入口，供 `main.rs` 调用）
pub fn run() {
    tracing_subscriber::fmt::init();
    tauri::Builder::default()
        .setup(|app| {
            let info = app::app_info(identifier_from(&app.handle()));
            tracing::info!(?info, "桌面应用启动");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            initialize_app,
            analyze_symbol,
            get_real_time_quotes,
            set_price_alert,
            export_data,
            get_app_info
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
