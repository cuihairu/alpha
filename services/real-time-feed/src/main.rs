//! Alpha Finance Real-Time Feed Service
//!
//! 实时数据流推送服务，支持 WebSocket 连接和广播

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use alpha_core::alerts::{AlertEngine, AlertEvent};
use axum::{routing::get, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::time::{interval, MissedTickBehavior};

mod alerts;
mod http;
mod stream;
mod sync;
#[cfg(test)]
mod tests;
mod ws;

use crate::sync::ChannelSyncState;
use alerts::load_alert_rules;
use http::{get_stats, health_check, metrics_endpoint};
use stream::start_stream_consumer;
use ws::websocket_handler;

/// 显式日志级别初始化：`tracing_subscriber::fmt::init()` 在未设 RUST_LOG 时只放行
/// ERROR，WARN 级兜底日志（DLQ 隔离失败、毒消息告警等）会全部不可见。与 data-engine
/// 的 telemetry.level 口径一致：默认 info，ALPHA_LOG_LEVEL 可调，RUST_LOG 兼容保留。
fn init_tracing() {
    let level = std::env::var("ALPHA_LOG_LEVEL")
        .or_else(|_| std::env::var("RUST_LOG"))
        .unwrap_or_else(|_| "info".to_string());
    tracing_subscriber::fmt()
        .with_max_level(parse_log_level(&level))
        .with_target(false)
        .init();
}

/// 日志级别字符串 → tracing::Level（未知取值回退 INFO，与 data-engine 同口径）。
fn parse_log_level(level: &str) -> tracing::Level {
    match level.to_lowercase().as_str() {
        "debug" => tracing::Level::DEBUG,
        "warn" => tracing::Level::WARN,
        "error" => tracing::Level::ERROR,
        "trace" => tracing::Level::TRACE,
        _ => tracing::Level::INFO,
    }
}

/// 实时数据消息
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RealTimeData {
    symbol: String,
    price: f64,
    volume: u64,
    change: f64,
    change_percent: f64,
    timestamp: chrono::DateTime<chrono::Utc>,
}

/// WebSocket 连接管理器
#[derive(Debug)]
struct ConnectionManager {
    connections: Arc<Mutex<HashMap<String, broadcast::Sender<RealTimeData>>>>,
}

impl ConnectionManager {
    fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn add_connection(&self, id: String, sender: broadcast::Sender<RealTimeData>) {
        self.connections.lock().unwrap().insert(id, sender);
    }

    fn remove_connection(&self, id: &str) {
        self.connections.lock().unwrap().remove(id);
    }

    fn get_connection_count(&self) -> usize {
        self.connections.lock().unwrap().len()
    }
}

/// 应用状态
#[derive(Debug)]
struct AppState {
    connection_manager: ConnectionManager,
    data_sender: broadcast::Sender<RealTimeData>,
    /// 业务告警引擎（L503）：规则评估 + 冷却/交叉推进状态
    alert_engine: Arc<Mutex<AlertEngine>>,
    /// 每符号滚动收盘窗口（PctChange/SmaCross 的 closes 数据源，封顶
    /// ALERT_CLOSES_CAP）
    alert_closes: Arc<Mutex<HashMap<String, Vec<f64>>>>,
    /// 告警事件广播发送端（WS 侧 alerts 通道）
    alert_sender: broadcast::Sender<AlertEvent>,
    /// 逐通道版本化发布状态：seq 递增 + 最近全量快照（增量基准）
    sync_state: Arc<Mutex<HashMap<String, ChannelSyncState>>>,
}

const DEFAULT_REDIS_URL: &str = "redis://localhost:6379";
const QUOTES_STREAM: &str = "quotes.raw";
const NORMALIZED_QUOTES_STREAM: &str = "quotes.normalized";
const QUOTES_DLQ_STREAM: &str = "quotes.dlq";
const REALTIME_GROUP: &str = "real-time-feed";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 初始化日志
    init_tracing();

    tracing::info!("Starting Alpha Finance Real-Time Feed Service");

    // 创建广播通道
    let (data_sender, _data_receiver) = broadcast::channel(1000);
    // 业务告警事件广播（L503）：与行情扇出平行的通道，WS 侧以 Data 帧
    // （channel=alerts）推送；容量给足，无订阅者时 send 报错即丢弃。
    let (alert_sender, _alert_receiver) = broadcast::channel(256);

    // 创建应用状态
    let app_state = Arc::new(AppState {
        connection_manager: ConnectionManager::new(),
        data_sender,
        alert_engine: Arc::new(Mutex::new(AlertEngine::new(load_alert_rules()))),
        alert_closes: Arc::new(Mutex::new(HashMap::new())),
        alert_sender,
        sync_state: Arc::new(Mutex::new(HashMap::new())),
    });

    // 优先从 Redis Streams 消费；不可用时回退到本地模拟数据。
    if let Err(err) = start_stream_consumer(app_state.clone()).await {
        tracing::warn!(
            "Redis stream consumer unavailable ({}), falling back to simulated feed",
            err
        );
        start_data_generator(app_state.clone());
    }

    // 构建 HTTP 路由
    let app = Router::new()
        .route("/ws", get(websocket_handler))
        .route("/health", get(health_check))
        .route("/metrics", get(metrics_endpoint))
        .route("/stats", get(get_stats))
        .with_state(app_state);

    // 启动服务器
    // 与 docker-compose/Dockerfile/dev-start 的约定一致（8082），可用 ALPHA_REALTIME_FEED_BIND
    // 覆盖（e2e 测试与多实例部署需要地址可配，默认值保持既有行为）
    let bind =
        std::env::var("ALPHA_REALTIME_FEED_BIND").unwrap_or_else(|_| "0.0.0.0:8082".to_string());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!("Real-Time Feed service listening on {}", bind);

    axum::serve(listener, app).await?;

    Ok(())
}

/// 启动数据生成器（模拟实时数据）
fn start_data_generator(app_state: Arc<AppState>) {
    let sender = app_state.data_sender.clone();

    tokio::spawn(async move {
        let mut interval = interval(Duration::from_millis(100)); // 每100ms发送一次数据
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        let symbols: Vec<String> = vec!["AAPL", "GOOGL", "MSFT", "AMZN", "TSLA"]
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        let mut last_prices: HashMap<String, f64> = symbols
            .iter()
            .map(|s| (s.clone(), 100.0 + rand::random::<f64>() * 900.0))
            .collect();

        loop {
            interval.tick().await;

            for symbol in &symbols {
                let last_price = *last_prices.get(symbol).unwrap_or(&100.0);
                let change = (rand::random::<f64>() - 0.5) * 10.0;
                let new_price = (last_price + change).max(1.0);
                let change_percent = ((new_price - last_price) / last_price) * 100.0;

                let data = RealTimeData {
                    symbol: symbol.clone(),
                    price: new_price,
                    volume: (1000 + rand::random::<u64>() % 90000) as u64,
                    change,
                    change_percent,
                    timestamp: chrono::Utc::now(),
                };

                // 更新最新价格
                last_prices.insert(symbol.clone(), new_price);

                // 广播数据
                if let Err(e) = sender.send(data.clone()) {
                    tracing::debug!("Failed to send real-time data: {}", e);
                }
            }
        }
    });
}
