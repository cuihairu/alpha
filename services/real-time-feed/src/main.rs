//! Alpha Finance Real-Time Feed Service
//!
//! 实时数据流推送服务，支持 WebSocket 连接和广播

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    sync::broadcast,
    time::{interval, MissedTickBehavior},
};

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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 初始化日志
    tracing_subscriber::fmt::init();

    tracing::info!("Starting Alpha Finance Real-Time Feed Service");

    // 创建广播通道
    let (data_sender, _data_receiver) = broadcast::channel(1000);

    // 创建应用状态
    let app_state = Arc::new(AppState {
        connection_manager: ConnectionManager::new(),
        data_sender,
    });

    // 启动数据生成器
    start_data_generator(app_state.clone());

    // 构建 HTTP 路由
    let app = Router::new()
        .route("/ws", get(websocket_handler))
        .route("/health", get(health_check))
        .route("/stats", get(get_stats))
        .with_state(app_state);

    // 启动服务器
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8082").await?;
    tracing::info!("Real-Time Feed service listening on 0.0.0.0:8082");

    axum::serve(listener, app).await?;

    Ok(())
}

/// 启动数据生成器（模拟实时数据）
fn start_data_generator(app_state: Arc<AppState>) {
    let sender = app_state.data_sender.clone();

    tokio::spawn(async move {
        let mut interval = interval(Duration::from_millis(100)); // 每100ms发送一次数据
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        let symbols = vec!["AAPL", "GOOGL", "MSFT", "AMZN", "TSLA"];
        let mut last_prices: HashMap<String, f64> = symbols.iter()
            .map(|s| (s.to_string(), 100.0 + rand::random::<f64>() * 900.0))
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

/// WebSocket 连接处理器
async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<Arc<AppState>>,
) -> Response {
    ws.on_upgrade(|socket| handle_websocket(socket, app_state))
}

/// 处理 WebSocket 连接
async fn handle_websocket(socket: WebSocket, app_state: Arc<AppState>) {
    let connection_id = uuid::Uuid::new_v4().to_string();
    tracing::info!("New WebSocket connection: {}", connection_id);

    // 为这个连接创建数据接收器
    let mut data_receiver = app_state.data_sender.subscribe();

    // 将连接添加到管理器
    app_state.connection_manager.add_connection(
        connection_id.clone(),
        app_state.data_sender.clone(),
    );

    // 处理连接
    let (mut sender, mut receiver) = socket.split();

    // 发送数据的任务
    let send_task = tokio::spawn(async move {
        while let Ok(data) = data_receiver.recv().await {
            let message = match serde_json::to_string(&data) {
                Ok(json) => Message::Text(json),
                Err(e) => {
                    tracing::error!("Failed to serialize real-time data: {}", e);
                    continue;
                }
            };

            if sender.send(message).await.is_err() {
                break;
            }
        }
    });

    // 接收消息的任务（处理心跳等）
    let receive_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    tracing::debug!("Received text message from {}: {}", connection_id, text);

                    // 处理订阅请求
                    if let Ok(subscribe_msg) = serde_json::from_str::<SubscribeMessage>(&text) {
                        tracing::info!("Client {} subscribed to: {:?}", connection_id, subscribe_msg.symbols);
                    }
                }
                Ok(Message::Ping(payload)) => {
                    // 响应 ping
                    if let Err(e) = sender.send(Message::Pong(payload)).await {
                        tracing::debug!("Failed to send pong: {}", e);
                        break;
                    }
                }
                Ok(Message::Close(_)) => {
                    break;
                }
                Err(e) => {
                    tracing::debug!("WebSocket error for {}: {}", connection_id, e);
                    break;
                }
                _ => {}
            }
        }
    });

    // 等待任一任务完成
    tokio::select! {
        _ = send_task => {},
        _ = receive_task => {},
    }

    // 清理连接
    app_state.connection_manager.remove_connection(&connection_id);
    tracing::info!("WebSocket connection closed: {}", connection_id);
}

/// 订阅消息
#[derive(Debug, Deserialize)]
struct SubscribeMessage {
    symbols: Vec<String>,
    action: Option<String>, // "subscribe" or "unsubscribe"
}

/// 健康检查
async fn health_check() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "healthy",
        "service": "real-time-feed",
        "timestamp": chrono::Utc::now(),
    }))
}

/// 获取服务统计信息
async fn get_stats(State(app_state): State<Arc<AppState>>) -> axum::Json<serde_json::Value> {
    let connection_count = app_state.connection_manager.get_connection_count();

    axum::Json(serde_json::json!({
        "active_connections": connection_count,
        "service": "real-time-feed",
        "timestamp": chrono::Utc::now(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connection_manager() {
        let manager = ConnectionManager::new();
        let (tx, _rx) = broadcast::channel(10);

        manager.add_connection("test".to_string(), tx);
        assert_eq!(manager.get_connection_count(), 1);

        manager.remove_connection("test");
        assert_eq!(manager.get_connection_count(), 0);
    }

    #[tokio::test]
    async fn test_real_time_data_serialization() {
        let data = RealTimeData {
            symbol: "AAPL".to_string(),
            price: 150.0,
            volume: 1000,
            change: 1.5,
            change_percent: 1.0,
            timestamp: chrono::Utc::now(),
        };

        let json = serde_json::to_string(&data).unwrap();
        let deserialized: RealTimeData = serde_json::from_str(&json).unwrap();

        assert_eq!(data.symbol, deserialized.symbol);
        assert_eq!(data.price, deserialized.price);
    }
}