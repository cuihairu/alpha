//! Alpha Finance Real-Time Feed Service
//!
//! 实时数据流推送服务，支持 WebSocket 连接和广播

use alpha_core::sync::build_delta;
use alpha_protocols::websocket::{channels, SyncOp, WsMessage};
use alpha_storage::{InvalidMessage, RedisStreamQueue, StreamEnvelope, StreamMessage};
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
    time::Duration,
};
use tokio::{
    sync::{broadcast, mpsc},
    time::{interval, MissedTickBehavior},
};

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
    /// 逐通道版本化发布状态：seq 递增 + 最近全量快照（增量基准）
    sync_state: Arc<Mutex<HashMap<String, ChannelSyncState>>>,
}

/// 服务端单通道同步状态（版本控制的服务端半边，见 [`next_versioned_frame`]）
#[derive(Debug, Clone)]
struct ChannelSyncState {
    /// 通道内已发布的帧数（= 最新版本号），全连接共享同一序列
    seq: u64,
    /// 最近一次广播的全量快照：非空时下一帧发 Delta（相对此快照的字段差）
    last_snapshot: Option<serde_json::Value>,
}

impl ChannelSyncState {
    fn new() -> Self {
        Self {
            seq: 0,
            last_snapshot: None,
        }
    }
}

/// 构造版本化同步帧：通道 seq 递增；首帧 `Full` 全量，后续 `Delta` 差异（只含变化字段）。
/// seq 与快照在锁内更新——全连接共享同一序列，客户端据此做丢帧检测/Resync。
fn next_versioned_frame(
    hub: &Mutex<HashMap<String, ChannelSyncState>>,
    channel: &str,
    data: &RealTimeData,
) -> WsMessage {
    let value = serde_json::to_value(data).unwrap_or_default();
    let mut hub = hub.lock().unwrap();
    let state = hub
        .entry(channel.to_string())
        .or_insert_with(ChannelSyncState::new);
    state.seq += 1;
    let payload = match &state.last_snapshot {
        Some(prev) => build_delta(prev, &value),
        None => value.clone(),
    };
    let op = if state.last_snapshot.is_some() {
        SyncOp::Delta
    } else {
        SyncOp::Full
    };
    state.last_snapshot = Some(value);
    WsMessage::sync(channel.to_string(), state.seq, op, payload)
}

/// Resync 响应：返回通道当前版本的 `Full` 快照帧（无快照/未知通道返回 None）
fn resync_full_snapshot(
    hub: &Mutex<HashMap<String, ChannelSyncState>>,
    channel: &str,
) -> Option<WsMessage> {
    let hub = hub.lock().unwrap();
    let state = hub.get(channel)?;
    let snapshot = state.last_snapshot.clone()?;
    Some(WsMessage::sync(
        channel.to_string(),
        state.seq,
        SyncOp::Full,
        snapshot,
    ))
}

/// Resync 打到未知通道/尚无快照时的错误码（HTTP 语义沿用：not found）
const RESYNC_UNKNOWN_CHANNEL_CODE: i32 = 404;

/// Resync 应答：已知通道回当前版本 `Full` 快照；未知通道/尚无快照回 `Error` 帧——
/// 客户端需要显式失败信号才能重订阅，静默丢弃只会让它空等重传。
fn resync_reply(hub: &Mutex<HashMap<String, ChannelSyncState>>, channel: &str) -> WsMessage {
    resync_full_snapshot(hub, channel).unwrap_or_else(|| {
        WsMessage::error(
            RESYNC_UNKNOWN_CHANNEL_CODE,
            format!("resync 失败：通道 {channel} 无快照或不存在"),
            None,
        )
    })
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

    // 创建应用状态
    let app_state = Arc::new(AppState {
        connection_manager: ConnectionManager::new(),
        data_sender,
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
    app_state
        .connection_manager
        .add_connection(connection_id.clone(), app_state.data_sender.clone());

    // 处理连接
    let (mut sender, mut receiver) = socket.split();
    let send_connection_id = connection_id.clone();
    let recv_connection_id = connection_id.clone();

    // 发送数据的任务：广播帧走版本化同步（首帧 Full、后续 Delta），
    // 与 Resync 定向回复（mpsc 带外通道）合并到同一发送循环。
    let send_sync_state = app_state.sync_state.clone();
    let recv_sync_state = app_state.sync_state.clone();
    let (resync_tx, mut resync_rx) = mpsc::unbounded_channel::<WsMessage>();
    let send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                res = resync_rx.recv() => {
                    match res {
                        Some(frame) => {
                            let message = match serde_json::to_string(&frame) {
                                Ok(json) => Message::Text(json),
                                Err(e) => {
                                    tracing::error!("Failed to serialize resync frame: {}", e);
                                    continue;
                                }
                            };
                            if sender.send(message).await.is_err() {
                                tracing::debug!("Send loop closed for {}", send_connection_id);
                                break;
                            }
                        }
                        None => break,
                    }
                }
                data = data_receiver.recv() => {
                    let data = match data {
                        Ok(data) => data,
                        Err(e) => {
                            // 广播 lagged（订阅过慢被丢弃）或通道关闭：与旧实现同等退出语义
                            tracing::debug!("Broadcast recv failed for {}: {}", send_connection_id, e);
                            break;
                        }
                    };
                    let frame = next_versioned_frame(
                        &send_sync_state,
                        channels::REAL_TIME_QUOTES,
                        &data,
                    );
                    let message = match serde_json::to_string(&frame) {
                        Ok(json) => Message::Text(json),
                        Err(e) => {
                            tracing::error!("Failed to serialize sync frame: {}", e);
                            continue;
                        }
                    };
                    if sender.send(message).await.is_err() {
                        tracing::debug!("Send loop closed for {}", send_connection_id);
                        break;
                    }
                }
            }
        }
    });

    // 接收消息的任务（处理心跳、订阅与 Resync）
    let receive_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    tracing::debug!(
                        "Received text message from {}: {}",
                        recv_connection_id,
                        text
                    );

                    // 版本化同步协议：Resync（丢帧恢复）与订阅请求
                    if let Ok(ws_msg) = serde_json::from_str::<WsMessage>(&text) {
                        match ws_msg {
                            WsMessage::Resync(req) => {
                                tracing::info!(
                                    "Client {} resync channel {} from seq {}",
                                    recv_connection_id,
                                    req.channel,
                                    req.from_seq
                                );
                                // 无论通道是否存在都必须回帧（Full 或 Error）：
                                // 静默丢弃会让客户端无从判断 Resync 结果
                                let _ =
                                    resync_tx.send(resync_reply(&recv_sync_state, &req.channel));
                            }
                            WsMessage::Subscribe(sub) => {
                                tracing::info!(
                                    "Client {} subscribed to: {:?} (channels {:?})",
                                    recv_connection_id,
                                    sub.symbols,
                                    sub.channels
                                );
                            }
                            _ => {}
                        }
                    }

                    // 处理订阅请求（旧版兼容）
                    if let Ok(subscribe_msg) = serde_json::from_str::<SubscribeMessage>(&text) {
                        tracing::info!(
                            "Client {} subscribed to: {:?}",
                            recv_connection_id,
                            subscribe_msg.symbols
                        );
                    }
                }
                Ok(Message::Ping(payload)) => {
                    tracing::debug!(
                        "Received ping from {}, payload: {:?}",
                        recv_connection_id,
                        payload
                    );
                }
                Ok(Message::Close(_)) => {
                    break;
                }
                Err(e) => {
                    tracing::debug!("WebSocket error for {}: {}", recv_connection_id, e);
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
    app_state
        .connection_manager
        .remove_connection(&connection_id);
    tracing::info!("WebSocket connection closed: {}", connection_id);
}

async fn start_stream_consumer(app_state: Arc<AppState>) -> anyhow::Result<()> {
    let redis_url = std::env::var("ALPHA_REDIS_URL")
        .or_else(|_| std::env::var("REDIS_URL"))
        .unwrap_or_else(|_| DEFAULT_REDIS_URL.to_string());
    let queue = RedisStreamQueue::connect(&redis_url)?;
    queue
        .ensure_consumer_group(QUOTES_STREAM, REALTIME_GROUP)
        .await?;
    let _ = queue
        .ensure_consumer_group(NORMALIZED_QUOTES_STREAM, REALTIME_GROUP)
        .await;
    let consumer = format!("rtf-{}", uuid::Uuid::new_v4());

    let (min_idle_ms, sweep_secs, max_delivery) = claim_sweep_config();
    let sweeper_state = app_state.clone();
    let sweeper_queue = queue.clone();

    let loop_queue = queue.clone();
    let loop_state = app_state.clone();
    tokio::spawn(async move {
        loop {
            let normalized_result = loop_queue
                .read_group(
                    NORMALIZED_QUOTES_STREAM,
                    REALTIME_GROUP,
                    &consumer,
                    20,
                    1000,
                )
                .await;
            let result = match normalized_result {
                Ok(result) if !result.is_empty() => result,
                _ => match loop_queue
                    .read_group(QUOTES_STREAM, REALTIME_GROUP, &consumer, 20, 1000)
                    .await
                {
                    Ok(result) => result,
                    Err(err) => {
                        tracing::warn!("Failed to poll Redis stream: {}", err);
                        continue;
                    }
                },
            };

            for invalid in result.invalid {
                quarantine_invalid(&loop_queue, &invalid).await;
            }
            for message in result.messages {
                process_realtime_message(&loop_state, &loop_queue, &message).await;
            }
        }
    });

    // 周期兜底：消费端崩溃遗留的「已投递、未 ack」孤儿 pending 由独立的 sweeper 认领后
    // 重放，处理路径与实时消费完全一致；投递次数达到封顶（ALPHA_CLAIM_MAX_DELIVERY）仍
    // pending 的「毒消息」不再重投，走 DLQ 契约隔离。注意：这与解码失败 → DLQ 是两条
    // 不同路径，后者发生在读取时（见 quarantine_invalid/process_realtime_message）。
    let sweeper_consumer = format!("rtf-sweep-{}", uuid::Uuid::new_v4());
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(sweep_secs.max(1)));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        ticker.tick().await; // interval 首个 tick 立即返回，跳过以保证先等一个周期

        loop {
            ticker.tick().await;
            for stream in [NORMALIZED_QUOTES_STREAM, QUOTES_STREAM] {
                match sweeper_queue
                    .claim_stale(
                        stream,
                        REALTIME_GROUP,
                        &sweeper_consumer,
                        min_idle_ms,
                        100,
                        max_delivery,
                    )
                    .await
                {
                    Ok(result) => {
                        for invalid in result.invalid {
                            quarantine_invalid(&sweeper_queue, &invalid).await;
                        }
                        for message in result.messages {
                            tracing::info!(
                                "Reprocessing orphaned pending message {} on {}",
                                message.id,
                                message.envelope.stream
                            );
                            process_realtime_message(&sweeper_state, &sweeper_queue, &message)
                                .await;
                        }
                    }
                    // Redis < 6.2 无 XPENDING IDLE/XCLAIM 认领：兜底不可用属预期降级，debug 级避免刷屏
                    Err(err) => {
                        tracing::debug!("claim_stale sweep skipped for {}: {}", stream, err)
                    }
                }
            }
        }
    });

    Ok(())
}

/// 孤儿 pending 兜底参数（env 可调便于测试/演练；生产默认闲置 30s、每 30s 扫一轮、
/// 单条累计投递封顶 5 次）。
fn claim_sweep_config() -> (u64, u64, u32) {
    let min_idle_ms = std::env::var("ALPHA_CLAIM_MIN_IDLE_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30_000);
    let sweep_secs = std::env::var("ALPHA_CLAIM_SWEEP_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let max_delivery = std::env::var("ALPHA_CLAIM_MAX_DELIVERY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    (min_idle_ms, sweep_secs, max_delivery)
}

/// 处理一条行情 stream 消息：转换 → 广播 → ack；
/// 转换失败则按 DLQ 契约转发 quotes.dlq 后 ack。
async fn process_realtime_message(
    app_state: &Arc<AppState>,
    queue: &RedisStreamQueue,
    message: &StreamMessage,
) {
    let stream_name = message.envelope.stream.clone();
    if let Some(data) = envelope_to_realtime(&message.envelope) {
        if let Err(err) = app_state.data_sender.send(data) {
            tracing::debug!("Failed to fan out realtime data: {}", err);
        }
        if let Err(err) = queue.ack(&stream_name, REALTIME_GROUP, &message.id).await {
            tracing::warn!("Failed to ack stream message {}: {}", message.id, err);
        }
    } else if let Err(err) =
        send_to_dlq(queue, &message.envelope, "invalid realtime quote payload").await
    {
        tracing::warn!("Failed to send message {} to DLQ: {}", message.id, err);
    } else if let Err(err) = queue.ack(&stream_name, REALTIME_GROUP, &message.id).await {
        tracing::warn!("Failed to ack DLQ'd message {}: {}", message.id, err);
    }
}

/// 无法解码的条目：按 DLQ 契约隔离（quotes.dlq）并 ack，
/// 避免滞留消费组 PEL 永不清理；DLQ 发布失败则不 ack，留待下轮重试。
async fn quarantine_invalid(queue: &RedisStreamQueue, invalid: &InvalidMessage) {
    tracing::warn!(
        "Quarantining undecodable message {} on {}: {}",
        invalid.id,
        invalid.stream,
        invalid.reason
    );
    if let Err(err) = queue.publish_dlq(QUOTES_DLQ_STREAM, invalid).await {
        tracing::warn!("Failed to publish undecodable message to DLQ: {}", err);
        return;
    }
    if let Err(err) = queue
        .ack(&invalid.stream, REALTIME_GROUP, &invalid.id)
        .await
    {
        tracing::warn!("Failed to ack quarantined message {}: {}", invalid.id, err);
    }
}

fn envelope_to_realtime(envelope: &StreamEnvelope) -> Option<RealTimeData> {
    let payload = envelope.payload.as_object()?;
    Some(RealTimeData {
        symbol: payload.get("symbol")?.as_str()?.to_string(),
        price: payload.get("price")?.as_f64()?,
        volume: payload.get("volume")?.as_u64()?,
        // data-engine 的 normalized payload 可能不含涨跌幅字段（如非行情类事件），
        // 这里缺省为 0 而不是把整条消息打进 DLQ。
        change: payload
            .get("change")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        change_percent: payload
            .get("change_percent")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        timestamp: envelope.created_at,
    })
}

async fn send_to_dlq(
    queue: &RedisStreamQueue,
    envelope: &StreamEnvelope,
    reason: &str,
) -> anyhow::Result<()> {
    let mut payload = envelope.payload.clone();
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("dlq_reason".to_string(), serde_json::json!(reason));
    }

    let dlq_envelope = StreamEnvelope::new(
        QUOTES_DLQ_STREAM,
        envelope.event_type.clone(),
        envelope.source.clone(),
        envelope.symbol.clone(),
        payload,
    );
    queue.publish(QUOTES_DLQ_STREAM, &dlq_envelope).await?;
    Ok(())
}

/// 订阅消息
#[derive(Debug, Deserialize)]
struct SubscribeMessage {
    symbols: Vec<String>,
    /// "subscribe" / "unsubscribe"；当前「连接即订阅」，服务端暂不区分动作，仅记日志
    #[allow(dead_code)]
    action: Option<String>,
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

    #[test]
    fn test_parse_log_level_defaults_to_info_and_matches_data_engine() {
        assert_eq!(parse_log_level("info"), tracing::Level::INFO);
        assert_eq!(parse_log_level("DEBUG"), tracing::Level::DEBUG);
        assert_eq!(parse_log_level("Warn"), tracing::Level::WARN);
        assert_eq!(parse_log_level("error"), tracing::Level::ERROR);
        assert_eq!(parse_log_level("trace"), tracing::Level::TRACE);
        // 未设/未知取值兜底 INFO：保证 WARN 级兜底日志可见
        assert_eq!(parse_log_level(""), tracing::Level::INFO);
        assert_eq!(parse_log_level("bogus"), tracing::Level::INFO);
    }

    #[test]
    fn test_envelope_to_realtime() {
        let envelope = StreamEnvelope::new(
            QUOTES_STREAM,
            "quote",
            "collector",
            Some("sz000001".to_string()),
            serde_json::json!({
                "symbol": "sz000001",
                "price": 12.34,
                "volume": 5000,
                "change": 0.12,
                "change_percent": 0.98
            }),
        );

        let data = envelope_to_realtime(&envelope).unwrap();
        assert_eq!(data.symbol, "sz000001");
        assert_eq!(data.price, 12.34);
    }

    #[test]
    fn test_envelope_to_realtime_without_change_fields() {
        // data-engine 的 normalized payload 即 MarketData 序列化，不含 change/change_percent，
        // 不应被误判为无效消息进入 DLQ。
        let envelope = StreamEnvelope::new(
            NORMALIZED_QUOTES_STREAM,
            "normalized_quote",
            "data-engine",
            Some("sz000001".to_string()),
            serde_json::json!({
                "symbol": "sz000001",
                "timestamp": "2026-09-27T08:00:00Z",
                "price": 12.34,
                "volume": 5000,
                "bid": 12.3,
                "ask": 12.4,
                "open": 12.1,
                "high": 12.5,
                "low": 12.0
            }),
        );

        let data = envelope_to_realtime(&envelope).unwrap();
        assert_eq!(data.symbol, "sz000001");
        assert_eq!(data.price, 12.34);
        assert_eq!(data.volume, 5000);
        assert_eq!(data.change, 0.0);
        assert_eq!(data.change_percent, 0.0);
    }

    #[test]
    fn test_envelope_to_realtime_rejects_missing_price() {
        let envelope = StreamEnvelope::new(
            QUOTES_STREAM,
            "quote",
            "collector",
            None,
            serde_json::json!({ "symbol": "sz000001", "volume": 5000 }),
        );

        assert!(envelope_to_realtime(&envelope).is_none());
    }

    /// 合成一条实时数据（price/volume 可变，其余字段固定；timestamp 固定保证
    /// Delta 断言精确——增量只含受控变化字段）
    fn sample_data(price: f64, volume: u64) -> RealTimeData {
        RealTimeData {
            symbol: "sz000001".to_string(),
            price,
            volume,
            change: 0.5,
            change_percent: 0.98,
            timestamp: chrono::DateTime::parse_from_rfc3339("2026-09-29T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        }
    }

    /// 首帧 Full（seq=1，含全字段），第二帧 Delta（seq=2，只含变化字段）
    #[test]
    fn test_versioned_frame_starts_full_then_delta() {
        let hub = Mutex::new(HashMap::new());

        let first = next_versioned_frame(&hub, channels::REAL_TIME_QUOTES, &sample_data(10.0, 100));
        match first {
            WsMessage::Sync(s) => {
                assert_eq!(s.seq, 1);
                assert_eq!(s.op, SyncOp::Full);
                assert_eq!(s.data["symbol"], "sz000001");
                assert_eq!(s.data["price"], 10.0);
                assert_eq!(s.data["volume"], 100);
            }
            other => panic!("首帧应为 Sync::Full，实际 {other:?}"),
        }

        // 价格变化：Delta 只携带变化字段；未变化字段（symbol/change 等）不重复传输
        let second =
            next_versioned_frame(&hub, channels::REAL_TIME_QUOTES, &sample_data(10.5, 100));
        match second {
            WsMessage::Sync(s) => {
                assert_eq!(s.seq, 2);
                assert_eq!(s.op, SyncOp::Delta);
                assert_eq!(s.data, serde_json::json!({"price": 10.5}));
            }
            other => panic!("第二帧应为 Sync::Delta，实际 {other:?}"),
        }

        // 多字段变化：Delta 含全部差异字段
        let third = next_versioned_frame(&hub, channels::REAL_TIME_QUOTES, &sample_data(11.0, 250));
        match third {
            WsMessage::Sync(s) => {
                assert_eq!(s.seq, 3);
                assert_eq!(s.op, SyncOp::Delta);
                assert_eq!(s.data, serde_json::json!({"price": 11.0, "volume": 250}));
            }
            other => panic!("第三帧应为 Sync::Delta，实际 {other:?}"),
        }
    }

    /// seq 逐通道独立递增（channel 维度隔离，互不串号）
    #[test]
    fn test_versioned_seq_increments_per_channel() {
        let hub = Mutex::new(HashMap::new());
        for _ in 0..3 {
            next_versioned_frame(&hub, channels::REAL_TIME_QUOTES, &sample_data(1.0, 1));
        }
        next_versioned_frame(&hub, channels::MARKET_DEPTH, &sample_data(2.0, 2));

        let hub = hub.lock().unwrap();
        assert_eq!(hub[channels::REAL_TIME_QUOTES].seq, 3);
        assert_eq!(hub[channels::MARKET_DEPTH].seq, 1);
    }

    /// Resync：未知通道/无快照返回 None；已广播过则回当前版本的 Full 快照
    #[test]
    fn test_resync_returns_current_full_snapshot() {
        let hub = Mutex::new(HashMap::new());
        // 未知通道
        assert!(resync_full_snapshot(&hub, "nope").is_none());

        // 广播两帧后 Resync：全量恢复 + 当前 seq
        next_versioned_frame(&hub, channels::REAL_TIME_QUOTES, &sample_data(10.0, 100));
        next_versioned_frame(&hub, channels::REAL_TIME_QUOTES, &sample_data(10.5, 100));
        let full = resync_full_snapshot(&hub, channels::REAL_TIME_QUOTES).unwrap();
        match full {
            WsMessage::Sync(s) => {
                assert_eq!(s.seq, 2);
                assert_eq!(s.op, SyncOp::Full);
                assert_eq!(s.data["price"], 10.5);
                // Full 快照必须含全字段（客户端替换本地快照的基线）
                assert_eq!(s.data["symbol"], "sz000001");
                assert_eq!(s.data["volume"], 100);
            }
            other => panic!("Resync 响应应为 Sync::Full，实际 {other:?}"),
        }
    }

    /// Resync 应答：未知通道/尚无快照必须显式回 Error 帧（客户端据此重订阅
    /// 而非空等）；已广播通道照常回 Full 快照
    #[test]
    fn test_resync_reply_errors_on_unknown_channel() {
        let hub = Mutex::new(HashMap::new());
        match resync_reply(&hub, "nope") {
            WsMessage::Error(e) => {
                assert_eq!(e.code, RESYNC_UNKNOWN_CHANNEL_CODE);
                assert!(
                    e.message.contains("nope"),
                    "错误信息应包含通道名: {}",
                    e.message
                );
            }
            other => panic!("未知通道 Resync 应回 Error 帧，实际 {other:?}"),
        }

        // 已广播通道：回当前版本 Full 快照
        next_versioned_frame(&hub, channels::REAL_TIME_QUOTES, &sample_data(9.0, 10));
        match resync_reply(&hub, channels::REAL_TIME_QUOTES) {
            WsMessage::Sync(s) => {
                assert_eq!(s.op, SyncOp::Full);
                assert_eq!(s.seq, 1);
            }
            other => panic!("已知通道 Resync 应回 Sync::Full，实际 {other:?}"),
        }
    }

    /// 服务端 Delta 与客户端 (alpha_core::sync) 合成为闭环：服务端发的差异
    /// 在客户端引擎里应用后，快照收敛到服务端最新全量（端到端一致）
    #[test]
    fn test_server_delta_applies_on_client_engine() {
        use alpha_core::sync::SyncEngine;
        use alpha_protocols::websocket::SyncMessage;

        let hub = Mutex::new(HashMap::new());
        let mut engine = SyncEngine::new();

        // 服务端广播两帧（Full + Delta），模拟客户端逐帧应用
        for data in [sample_data(10.0, 100), sample_data(10.5, 150)] {
            match next_versioned_frame(&hub, channels::REAL_TIME_QUOTES, &data) {
                WsMessage::Sync(SyncMessage {
                    channel,
                    seq,
                    op,
                    data,
                    ..
                }) => match op {
                    SyncOp::Full => {
                        engine.apply_full(&channel, seq, data);
                    }
                    SyncOp::Delta => {
                        engine.apply_delta(&channel, seq, data).unwrap();
                    }
                },
                other => panic!("应为 Sync 帧，实际 {other:?}"),
            }
        }

        // 客户端快照与服务端最新快照一致（字段逐项核对）
        let client_snapshot = engine.snapshot(channels::REAL_TIME_QUOTES).unwrap();
        let hub_guard = hub.lock().unwrap();
        let server_snapshot = hub_guard[channels::REAL_TIME_QUOTES]
            .last_snapshot
            .as_ref()
            .unwrap();
        assert_eq!(client_snapshot, server_snapshot);
        assert_eq!(client_snapshot["price"], 10.5);
        assert_eq!(client_snapshot["volume"], 150);
        assert_eq!(engine.last_seq(channels::REAL_TIME_QUOTES), Some(2));
    }
}
