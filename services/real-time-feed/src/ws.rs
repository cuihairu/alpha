use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};

use alpha_protocols::websocket::{channels, DataMessage, WsMessage};

use crate::sync::{next_versioned_frame, resync_reply};
use crate::AppState;

/// WebSocket 连接处理器
pub(crate) async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<Arc<AppState>>,
) -> Response {
    ws.on_upgrade(|socket| handle_websocket(socket, app_state))
}

/// 处理 WebSocket 连接
pub(crate) async fn handle_websocket(socket: WebSocket, app_state: Arc<AppState>) {
    let connection_id = uuid::Uuid::new_v4().to_string();
    tracing::info!("New WebSocket connection: {}", connection_id);

    // 为这个连接创建数据接收器
    let mut data_receiver = app_state.data_sender.subscribe();
    // 告警事件接收器（L503）：连接即订阅（与行情面同原则）；规则空时
    // 无事件，Data 帧只在命中时出现
    let mut alert_receiver = app_state.alert_sender.subscribe();

    // 将连接添加到管理器
    app_state
        .connection_manager
        .add_connection(connection_id.clone(), app_state.data_sender.clone());

    // 处理连接
    let (mut sender, mut receiver) = socket.split();
    let send_connection_id = connection_id.clone();
    let recv_connection_id = connection_id.clone();

    // 发送数据的任务：广播帧走版本化同步（首帧 Full、后续 Delta），
    // Resync 定向回复（mpsc 带外通道）与业务告警事件（alerts 通道）
    // 合并到同一发送循环。
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
                alert = alert_receiver.recv() => {
                    let event = match alert {
                        Ok(event) => event,
                        // 慢订阅 lagged：丢的是历史告警，连接保活继续收新事件
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => break,
                    };
                    let frame = WsMessage::Data(DataMessage {
                        channel: channels::ALERTS.to_string(),
                        data: serde_json::to_value(&event).unwrap_or_default(),
                        timestamp: event.ts_ms,
                    });
                    let message = match serde_json::to_string(&frame) {
                        Ok(json) => Message::Text(json),
                        Err(e) => {
                            tracing::error!("Failed to serialize alert frame: {}", e);
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

/// 订阅消息
#[derive(Debug, Deserialize)]
pub(crate) struct SubscribeMessage {
    symbols: Vec<String>,
    /// "subscribe" / "unsubscribe"；当前「连接即订阅」，服务端暂不区分动作，仅记日志
    #[allow(dead_code)]
    action: Option<String>,
}
