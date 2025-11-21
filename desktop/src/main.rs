//! Alpha Finance 桌面应用
//!
//! 基于 Tauri 的跨平台桌面金融分析应用

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use alpha_core::{models::*, analytics::AnalysisEngine};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tauri::{Manager, State};

/// 应用状态
#[derive(Debug)]
struct AppState {
    analysis_engine: AnalysisEngine,
    config_dir: PathBuf,
    data_dir: PathBuf,
}

/// 配置结构
#[derive(Debug, Serialize, Deserialize)]
struct AppConfig {
    api_url: String,
    symbols: Vec<String>,
    theme: String,
    auto_update: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            api_url: "http://localhost:8080".to_string(),
            symbols: vec!["AAPL".to_string(), "GOOGL".to_string(), "MSFT".to_string()],
            theme: "light".to_string(),
            auto_update: true,
        }
    }
}

/// 分析请求
#[derive(Debug, Deserialize)]
struct AnalyzeRequest {
    symbol: String,
    timeframe: String,
    indicators: Vec<String>,
}

/// 导出请求
#[derive(Debug, Deserialize)]
struct ExportRequest {
    symbols: Vec<String>,
    format: String, // "csv", "json", "excel"
    date_range: Option<DateRange>,
}

/// Tauri 命令实现

/// 初始化应用
#[tauri::command]
async fn initialize_app(app_handle: tauri::AppHandle) -> Result<AppConfig, String> {
    // 获取应用目录
    let app_dir = app_handle.path_resolver().app_config_dir()
        .ok_or("无法获取配置目录")?;
    let data_dir = app_handle.path_resolver().app_data_dir()
        .ok_or("无法获取数据目录")?;

    // 确保目录存在
    fs::create_dir_all(&app_dir).map_err(|e| format!("创建配置目录失败: {}", e))?;
    fs::create_dir_all(&data_dir).map_err(|e| format!("创建数据目录失败: {}", e))?;

    // 读取或创建配置文件
    let config_path = app_dir.join("config.json");
    let config = if config_path.exists() {
        let content = fs::read_to_string(&config_path)
            .map_err(|e| format!("读取配置文件失败: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("解析配置文件失败: {}", e))?
    } else {
        let config = AppConfig::default();
        let content = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("序列化配置失败: {}", e))?;
        fs::write(&config_path, content)
            .map_err(|e| format!("写入配置文件失败: {}", e))?;
        config
    };

    // 初始化应用状态
    let state = AppState {
        analysis_engine: AnalysisEngine::new(),
        config_dir: app_dir,
        data_dir,
    };

    app_handle.manage(state);

    Ok(config)
}

/// 分析股票数据
#[tauri::command]
async fn analyze_symbol(
    request: AnalyzeRequest,
    state: State<'_, AppState>,
) -> Result<AnalysisResult, String> {
    // 这里应该从 API 或本地缓存获取数据
    let market_data = fetch_market_data(&request.symbol).await
        .map_err(|e| format!("获取市场数据失败: {}", e))?;

    if market_data.is_empty() {
        return Err("没有找到市场数据".to_string());
    }

    // 执行分析
    let analysis_result = state.analysis_engine
        .analyze_symbol(&market_data, None)
        .await
        .map_err(|e| format!("分析失败: {}", e))?;

    Ok(analysis_result)
}

/// 获取实时行情
#[tauri::command]
async fn get_real_time_quotes(symbols: Vec<String>) -> Result<Vec<MarketData>, String> {
    let mut quotes = Vec::new();

    for symbol in symbols {
        let quote = fetch_single_quote(&symbol).await
            .map_err(|e| format!("获取 {} 行情失败: {}", symbol, e))?;
        quotes.push(quote);
    }

    Ok(quotes)
}

/// 设置价格告警
#[tauri::command]
async fn set_price_alert(
    symbol: String,
    target_price: f64,
    alert_type: String, // "above" or "below"
    state: State<'_, AppState>,
) -> Result<String, String> {
    // 保存告警配置到本地文件
    let alerts_path = state.config_dir.join("alerts.json");

    let mut alerts: HashMap<String, AlertConfig> = if alerts_path.exists() {
        let content = fs::read_to_string(&alerts_path)
            .map_err(|e| format!("读取告警配置失败: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("解析告警配置失败: {}", e))?
    } else {
        HashMap::new()
    };

    let alert_id = format!("{}_{}", symbol, chrono::Utc::now().timestamp());

    alerts.insert(alert_id.clone(), AlertConfig {
        symbol,
        target_price,
        alert_type,
        created_at: chrono::Utc::now(),
        active: true,
    });

    let content = serde_json::to_string_pretty(&alerts)
        .map_err(|e| format!("序列化告警配置失败: {}", e))?;
    fs::write(alerts_path, content)
        .map_err(|e| format!("保存告警配置失败: {}", e))?;

    Ok(alert_id)
}

/// 导出数据
#[tauri::command]
async fn export_data(
    request: ExportRequest,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let export_dir = state.data_dir.join("exports");
    fs::create_dir_all(&export_dir)
        .map_err(|e| format!("创建导出目录失败: {}", e))?;

    // 为每个符号生成文件
    let mut exported_files = Vec::new();
    for symbol in &request.symbols {
        let market_data = fetch_market_data(symbol).await
            .map_err(|e| format!("获取 {} 数据失败: {}", symbol, e))?;

        let filename = match request.format.as_str() {
            "csv" => export_to_csv(&market_data, &export_dir, symbol)?,
            "json" => export_to_json(&market_data, &export_dir, symbol)?,
            _ => return Err("不支持的导出格式".to_string()),
        };

        exported_files.push(filename);
    }

    Ok(format!("成功导出 {} 个文件", exported_files.len()))
}

/// 获取应用信息
#[tauri::command]
async fn get_app_info() -> Result<AppInfo, String> {
    Ok(AppInfo {
        name: "Alpha Finance".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    })
}

// 辅助结构和函数

#[derive(Debug, Serialize, Deserialize)]
struct AlertConfig {
    symbol: String,
    target_price: f64,
    alert_type: String,
    created_at: chrono::DateTime<chrono::Utc>,
    active: bool,
}

#[derive(Debug, Serialize)]
struct AppInfo {
    name: String,
    version: String,
    platform: String,
    arch: String,
}

/// 模拟获取市场数据
async fn fetch_market_data(symbol: &str) -> Result<Vec<MarketData>, anyhow::Error> {
    // 这里应该实现真实的数据获取逻辑
    let mut data = Vec::new();
    let base_price = 100.0 + (symbol.len() as f64 * 10.0);

    for i in 0..100 {
        let price = base_price + (i as f64 * 0.5) + (rand::random::<f64>() - 0.5) * 2.0;
        let volume = 1000 + rand::random::<u64>() % 90000;

        let market_data = MarketData {
            symbol: symbol.to_string(),
            timestamp: chrono::Utc::now() - chrono::Duration::minutes((100 - i) as i64),
            price,
            volume,
            bid: Some(price - 0.01),
            ask: Some(price + 0.01),
            open: Some(price - 0.1),
            high: Some(price + 0.2),
            low: Some(price - 0.3),
        };

        data.push(market_data);
    }

    Ok(data)
}

/// 模拟获取单个行情
async fn fetch_single_quote(symbol: &str) -> Result<MarketData, anyhow::Error> {
    let base_price = 100.0 + (symbol.len() as f64 * 10.0);
    let price = base_price + (rand::random::<f64>() - 0.5) * 10.0;

    Ok(MarketData {
        symbol: symbol.to_string(),
        timestamp: chrono::Utc::now(),
        price,
        volume: 1000 + rand::random::<u64>() % 90000,
        bid: Some(price - 0.01),
        ask: Some(price + 0.01),
        open: Some(price - 0.1),
        high: Some(price + 0.2),
        low: Some(price - 0.3),
    })
}

/// 导出到 CSV
fn export_to_csv(data: &[MarketData], export_dir: &PathBuf, symbol: &str) -> Result<String, anyhow::Error> {
    let filename = format!("{}_{}.csv", symbol, chrono::Utc::now().format("%Y%m%d_%H%M%S"));
    let filepath = export_dir.join(&filename);

    let mut wtr = csv::Writer::from_path(&filepath)?;

    // 写入标题行
    wtr.write_record(&["symbol", "timestamp", "price", "volume", "open", "high", "low"])?;

    // 写入数据行
    for item in data {
        wtr.write_record(&[
            &item.symbol,
            &item.timestamp.to_rfc3339(),
            &item.price.to_string(),
            &item.volume.to_string(),
            &item.open.map(|v| v.to_string()).unwrap_or_default(),
            &item.high.map(|v| v.to_string()).unwrap_or_default(),
            &item.low.map(|v| v.to_string()).unwrap_or_default(),
        ])?;
    }

    wtr.flush()?;
    Ok(filename)
}

/// 导出到 JSON
fn export_to_json(data: &[MarketData], export_dir: &PathBuf, symbol: &str) -> Result<String, anyhow::Error> {
    let filename = format!("{}_{}.json", symbol, chrono::Utc::now().format("%Y%m%d_%H%M%S"));
    let filepath = export_dir.join(&filename);

    let content = serde_json::to_string_pretty(data)?;
    fs::write(filepath, content)?;

    Ok(filename)
}

fn main() {
    // 初始化日志
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .setup(|app| {
            // 这里可以进行应用初始化
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            initialize_app,
            analyze_symbol,
            get_real_time_quotes,
            set_price_alert,
            export_data,
            get_app_info,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_market_data() {
        let data = fetch_market_data("AAPL").await.unwrap();
        assert!(!data.is_empty());
        assert_eq!(data[0].symbol, "AAPL");
    }

    #[test]
    fn test_app_config_serialization() {
        let config = AppConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.api_url, deserialized.api_url);
    }
}