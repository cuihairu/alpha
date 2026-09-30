//! Tauri 接线层（`gui` 特性）
//!
//! 每个 `#[tauri::command]` 只做三件事：解析路径/参数 → 委派 [`crate`] 框架层 →
//! 把结果或错误映射给前端。业务逻辑全在框架层，因此本文件（连同 `generate_context!`
//! 生成的窗口/托盘配置）由 macOS CI 作业编译验证，框架层则在无 GUI 环境的主门禁里测试。
//!
//! 命令清单与 web 前端调用方一一对应（见 web/app.js 的 `invoke`）。

use crate::alerts::{self, AlertKind};
use crate::analysis;
use crate::app;
use crate::config::{self, ConfigSource};
use crate::error::DesktopError;
use crate::export::{self, ExportFormat};
use crate::ipc::{AnalyzeRequest, ExportRequest};
use crate::market;
use crate::state::AppState;
use alpha_core::models::{AnalysisResult, MarketData};
use chrono::Utc;
use serde::Serialize;
use tauri::{Manager, State};

/// 初始化结果：配置 + 加载来源（前端据此提示「已回退默认配置」）
#[derive(Debug, Clone, Serialize)]
pub struct InitPayload {
    config: crate::config::AppConfig,
    source: &'static str,
    validation: Vec<String>,
}

/// 初始化应用：解析平台目录 → 加载/自举配置 → 建目录 → 注册状态
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

    let config_path = config_dir.join(crate::paths::CONFIG_FILE_NAME);
    let (cfg, source) = config::load_or_default(&config_path);
    if source != ConfigSource::File {
        // 首次启动/配置损坏：把可用配置落盘，保证下次走 File 路径
        config::save(&config_path, &cfg).map_err(|e| e.to_string())?;
    }
    let validation = cfg.validate().unwrap_or_default();
    if !validation.is_empty() {
        tracing::warn!(?validation, "应用配置不完整，按当前值启动");
    }

    let state = AppState::bootstrap(config_dir, data_dir).map_err(|e| e.to_string())?;
    app_handle.manage(state);

    Ok(InitPayload {
        config: cfg,
        source: source.as_str(),
        validation,
    })
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
    analysis::analyze(state.engine(), &request.symbol)
        .await
        .map_err(|e| e.to_string())
}

/// 批量快照行情
#[tauri::command]
async fn get_real_time_quotes(symbols: Vec<String>) -> Result<Vec<MarketData>, String> {
    if symbols.is_empty() {
        return Err("symbols 不能为空".to_string());
    }
    Ok(analysis::quotes(&symbols))
}

/// 新增价格告警，返回告警 id
#[tauri::command]
async fn set_price_alert(
    symbol: String,
    target_price: f64,
    alert_type: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let kind = AlertKind::parse(&alert_type).map_err(|e| e.to_string())?;
    alerts::upsert(
        &state.paths().alerts_file(),
        &symbol,
        target_price,
        kind,
        Utc::now(),
    )
    .map_err(|e| e.to_string())
}

/// 导出数据，返回导出的文件名列表
#[tauri::command]
async fn export_data(
    request: ExportRequest,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let format = ExportFormat::parse(&request.format).map_err(|e| e.to_string())?;
    let dir = state.paths().exports_dir();
    let mut files = Vec::with_capacity(request.symbols.len());
    for symbol in &request.symbols {
        let series = market::synthetic_series(symbol, market::DEFAULT_BARS);
        let outcome = export::export(&series, &dir, symbol, format, Utc::now())
            .map_err(|e: DesktopError| e.to_string())?;
        files.push(outcome.filename);
    }
    Ok(files)
}

/// 应用信息
#[tauri::command]
async fn get_app_info() -> Result<app::AppInfo, String> {
    Ok(app::app_info())
}

/// 启动 Tauri 应用（进程入口，供 `main.rs` 调用）
pub fn run() {
    tracing_subscriber::fmt::init();
    tauri::Builder::default()
        .setup(|_app| {
            tracing::info!(info = ?app::app_info(), "桌面应用启动");
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
