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
    /// 版本化同步帧（增量更新 + 版本控制，见 [`SyncMessage`]）
    Sync(SyncMessage),
    /// 重新同步请求（客户端检测到丢帧后发起，服务端回 Full 快照）
    Resync(ResyncRequest),
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

/// 同步操作类型（服务端 → 客户端 [`SyncMessage`] 帧）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncOp {
    /// 全量快照：`data` 携带通道当前完整状态（首帧、Resync 响应）
    Full,
    /// 增量更新：`data` 携带相对上一帧的字段级差异（最小口径 = 深度 1 差集）
    Delta,
}

/// 版本化实时同步帧（服务端 → 客户端）
///
/// 版本与增量契约（TODO「实现实时数据同步协议（WebSocket 增量更新 + 版本控制）」的最小口径）：
/// * `seq` 为**通道内**单调递增版本号：客户端校验 `seq == last_seq + 1`，
///   断裂即丢帧 → 发 [`ResyncRequest`] 请求重同步；
/// * 首帧与 Resync 响应为 `Full`，随后逐帧 `Delta`（仅差异字段）；
/// * 增量生成/合并不在本契约层：`build_delta`/`apply_delta` 在
///   `alpha_core::sync`（L0 纯计算，native 单测锁定语义）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncMessage {
    pub channel: String,
    /// 通道内单调版本号
    pub seq: u64,
    pub op: SyncOp,
    pub data: serde_json::Value,
    pub timestamp: i64,
}

/// 重新同步请求（客户端 → 服务端）
///
/// 语义：客户端在通道 `channel` 上缺失 `from_seq` 起的帧，服务端应回复
/// `seq` 不低于当前值的 `Full` 快照帧（见 real-time-feed 的 Resync 处理）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResyncRequest {
    pub channel: String,
    /// 客户端已连续应用到的版本；服务端回全量快照并携带当前最新 seq
    pub from_seq: u64,
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

    /// 创建版本化同步帧（服务端 → 客户端）
    pub fn sync(channel: String, seq: u64, op: SyncOp, data: serde_json::Value) -> Self {
        WsMessage::Sync(SyncMessage {
            channel,
            seq,
            op,
            data,
            timestamp: chrono::Utc::now().timestamp_millis(),
        })
    }

    /// 创建重新同步请求（客户端 → 服务端）
    pub fn resync(channel: String, from_seq: u64) -> Self {
        WsMessage::Resync(ResyncRequest { channel, from_seq })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Sync 全量帧的线上 JSON 形态：`{"type":"Sync",...}`（与 Data 帧同构，serde tag）
    #[test]
    fn sync_full_frame_serializes_with_type_tag() {
        let msg = WsMessage::sync(
            "real_time_quotes".to_string(),
            7,
            SyncOp::Full,
            json!({"symbol": "sz000001", "price": 12.34}),
        );
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "Sync");
        assert_eq!(json["channel"], "real_time_quotes");
        assert_eq!(json["seq"], 7);
        assert_eq!(json["op"], "Full");
        assert_eq!(json["data"]["price"], 12.34);

        let back: WsMessage = serde_json::from_value(json).unwrap();
        match back {
            WsMessage::Sync(s) => {
                assert_eq!(s.channel, "real_time_quotes");
                assert_eq!(s.seq, 7);
                assert_eq!(s.op, SyncOp::Full);
                assert_eq!(s.data["price"], 12.34);
            }
            other => panic!("应为 Sync 帧，实际 {other:?}"),
        }
    }

    /// Delta 帧 op 序列化到 "Delta"，与 Full 区分
    #[test]
    fn sync_delta_frame_op_round_trips() {
        let msg = WsMessage::sync(
            "real_time_quotes".to_string(),
            8,
            SyncOp::Delta,
            json!({"price": 12.35}),
        );
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "Sync");
        assert_eq!(json["op"], "Delta");
        assert_eq!(json["seq"], 8);

        let back: WsMessage = serde_json::from_value(json).unwrap();
        match back {
            WsMessage::Sync(s) => {
                assert_eq!(s.op, SyncOp::Delta);
                assert_eq!(s.data["price"], 12.35);
            }
            other => panic!("应为 Sync 帧，实际 {other:?}"),
        }
    }

    /// Resync 请求（客户端 → 服务端）的 JSON 契约
    #[test]
    fn resync_request_round_trips() {
        let msg = WsMessage::resync("real_time_quotes".to_string(), 42);
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "Resync");
        assert_eq!(json["channel"], "real_time_quotes");
        assert_eq!(json["from_seq"], 42);

        let back: WsMessage = serde_json::from_value(json).unwrap();
        match back {
            WsMessage::Resync(r) => {
                assert_eq!(r.channel, "real_time_quotes");
                assert_eq!(r.from_seq, 42);
            }
            other => panic!("应为 Resync 请求，实际 {other:?}"),
        }
    }

    /// 既有 Data 帧形态不受新变体影响（旧客户端/前端兼容性锚点）
    #[test]
    fn legacy_data_frame_shape_unchanged() {
        let msg = WsMessage::Data(DataMessage {
            channel: "real_time_quotes".to_string(),
            data: json!({"symbol": "sz000001"}),
            timestamp: 1_752_000_000_000,
        });
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "Data");
        assert_eq!(json["channel"], "real_time_quotes");

        let back: WsMessage = serde_json::from_value(json).unwrap();
        assert!(matches!(back, WsMessage::Data(_)));
    }
}
