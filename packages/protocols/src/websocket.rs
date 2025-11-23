//! WebSocket 协议定义

use serde::{Deserialize, Serialize};

/// WebSocket 消息类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    /// 心跳消息
    Ping,
    /// 心跳响应
    Pong,
    /// 订阅请求
    Subscribe(SubscribeRequest),
    /// 取消订阅请求
    Unsubscribe(UnsubscribeRequest),
    /// 认证请求
    Auth(AuthRequest),
    /// 实时数据推送
    Data(DataMessage),
    /// 错误消息
    Error(ErrorMessage),
    /// 连接确认
    Connected(ConnectedMessage),
}

/// 订阅请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeRequest {
    pub id: String,
    pub channels: Vec<String>,
    pub symbols: Option<Vec<String>>,
}

/// 取消订阅请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsubscribeRequest {
    pub id: Option<String>, // 订阅ID，如果为None则取消所有订阅
    pub channel: Option<String>,
}

/// 认证请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthRequest {
    pub token: String,
}

/// 数据消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMessage {
    pub channel: String,
    pub data: serde_json::Value,
    pub timestamp: i64,
}

/// 错误消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorMessage {
    pub code: i32,
    pub message: String,
    pub details: Option<String>,
}

/// 连接确认消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectedMessage {
    pub session_id: String,
    pub server_time: i64,
    pub supported_channels: Vec<String>,
}

/// 预定义的 WebSocket 频道
pub mod channels {
    pub const REAL_TIME_QUOTES: &str = "real_time_quotes";
    pub const MARKET_DEPTH: &str = "market_depth";
    pub const TECHNICAL_INDICATORS: &str = "technical_indicators";
    pub const NEWS_FEED: &str = "news_feed";
    pub const ANNOUNCEMENTS: &str = "announcements";
    pub const ALERTS: &str = "alerts";
}

/// 实时报价消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealTimeQuote {
    pub symbol: String,
    pub price: f64,
    pub volume: u64,
    pub bid: Option<f64>,
    pub ask: Option<f64>,
    pub timestamp: i64,
}

/// 市场深度消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketDepth {
    pub symbol: String,
    pub bids: Vec<PriceLevel>,
    pub asks: Vec<PriceLevel>,
    pub timestamp: i64,
}

/// 价格档位
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceLevel {
    pub price: f64,
    pub size: u64,
    pub orders_count: Option<u32>,
}

/// 技术指标更新消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndicatorUpdate {
    pub symbol: String,
    pub indicator: String,
    pub value: f64,
    pub timestamp: i64,
}

impl WsMessage {
    /// 创建心跳消息
    pub fn ping() -> Self {
        WsMessage::Ping
    }

    /// 创建心跳响应消息
    pub fn pong() -> Self {
        WsMessage::Pong
    }

    /// 创建订阅消息
    pub fn subscribe(id: String, channels: Vec<String>, symbols: Option<Vec<String>>) -> Self {
        WsMessage::Subscribe(SubscribeRequest {
            id,
            channels,
            symbols,
        })
    }

    /// 创建错误消息
    pub fn error(code: i32, message: String, details: Option<String>) -> Self {
        WsMessage::Error(ErrorMessage {
            code,
            message,
            details,
        })
    }
}
