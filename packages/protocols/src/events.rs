//! 管线消息契约（architecture-review §2.2/§3.1：Envelope v2）
//!
//! `EventEnvelope`（别名 StreamEnvelope，见 storage/redis_streams.rs）是
//! collector → data-engine → real-time-feed 全管线的消息契约。契约属于
//! protocols 层（纯 serde、无存储依赖）；storage 只负责持久化它。
//!
//! # v2 加性原则
//!
//! 新增字段一律 `Option<T>` + `#[serde(default)]` + `skip_serializing_if =
//! "Option::is_none"`：
//!
//! - **读老消息**：v1 无新字段 → default → None，解码不破；
//! - **写新消息**：字段为 None 时不写出 → 字节面与 v1 逐字节一致，
//!   老版本消费者照常解码（未知字段宽容）。
//!
//! # 时间三跳模型（§3.3）
//!
//! - `ingest_ts`：Alpha 收到时刻（ingest_time，v1 既有）
//! - `event_time`：数据源侧事件时刻（由 producer 从 payload.timestamp 上提）
//! - `process_time`：本跳处理完成时刻（normalized/derived 各打一笔）
//!
//! 三者齐备后可度量 source/ingestion/processing latency（low-latency 定位的量尺）。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 管线消息契约 v2（v1 字段 + v2 加性字段，见模块文档）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// 队列条目 ID（消费者侧回填；生产者侧为 None）
    #[serde(default)]
    pub id: Option<String>,
    /// 所属 Redis stream（如 quotes.raw / quotes.normalized）
    pub stream: String,
    /// 消息 schema 版本（当前 "v2"；加性演进，老 reader 可读）
    pub version: String,
    /// 事件类型（quote / normalized_quote / invalid_message …）
    pub event_type: String,
    /// 数据源（eastmoney / data-engine …）
    pub source: String,
    /// 标的符号（裸数字或带交易所前缀；与 Instrument 契约对账）
    #[serde(default)]
    pub symbol: Option<String>,
    /// 市场（cn/hk/us；配合 Instrument。用 String 而非 enum：流内消息可被
    /// 旧版本 reader 消费，开放取值域在契约演进中不炸旧解码器）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market: Option<String>,
    /// Alpha 收到时刻（ingest_time）
    pub ingest_ts: DateTime<Utc>,
    /// v2 新增：数据源侧事件时刻（event_time；producer 从 payload.timestamp 上提）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_time: Option<DateTime<Utc>>,
    /// v2 新增：本跳处理完成时刻（process_time；normalized/derived 各写各的）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_time: Option<DateTime<Utc>>,
    /// payload 内容指纹（去重窗口判定用）
    #[serde(default)]
    pub payload_hash: String,
    /// v2 新增：数据源自己的事件 ID（对账/去重锚点）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_id: Option<String>,
    /// v2 新增：per-source 单调序列号（断档检测核心，P2 数据质量接线）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
    /// v2 新增：与 gateway X-Trace-Id 对接的链路 ID
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    /// 事件负载（JSON）
    pub payload: serde_json::Value,
    /// 生产者创建时刻（v1 既有；WS 转发缺省时间戳用它）
    pub created_at: DateTime<Utc>,
}

impl EventEnvelope {
    /// v1 兼容构造（签名与行为不变；v2 字段通过 with_* 链式补充）
    pub fn new(
        stream: impl Into<String>,
        event_type: impl Into<String>,
        source: impl Into<String>,
        symbol: Option<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: None,
            stream: stream.into(),
            version: "v2".to_string(),
            event_type: event_type.into(),
            source: source.into(),
            symbol,
            market: None,
            ingest_ts: Utc::now(),
            event_time: None,
            process_time: None,
            payload_hash: payload_hash(&payload),
            source_event_id: None,
            sequence: None,
            trace_id: None,
            payload,
            created_at: Utc::now(),
        }
    }

    /// 携带数据源侧事件时刻（event_time；由 producer 从 payload.timestamp 上提）
    pub fn with_event_time(mut self, event_time: DateTime<Utc>) -> Self {
        self.event_time = Some(event_time);
        self
    }

    /// 携带数据源自己的事件 ID（对账/去重锚点）
    pub fn with_source_event_id(mut self, source_event_id: impl Into<String>) -> Self {
        self.source_event_id = Some(source_event_id.into());
        self
    }

    /// 携带市场标注（cn/hk/us）
    pub fn with_market(mut self, market: impl Into<String>) -> Self {
        self.market = Some(market.into());
        self
    }

    /// 携带 per-source 单调序列号（断档检测数据源）
    pub fn with_sequence(mut self, sequence: u64) -> Self {
        self.sequence = Some(sequence);
        self
    }

    /// 携带链路 ID（与 gateway X-Trace-Id 对接）
    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }

    /// 打上本跳处理完成时刻（process_time；写方在完成处理后调用）
    pub fn with_process_time(mut self, process_time: DateTime<Utc>) -> Self {
        self.process_time = Some(process_time);
        self
    }
}

/// payload 内容指纹（v1 既有实现随迁；同 producer 进程内去重一致即可）
fn payload_hash(payload: &serde_json::Value) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    payload.to_string().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// v1 老消息（无 v2 字段）必须解码：新字段全为 None/default
    #[test]
    fn decodes_legacy_v1_message_without_v2_fields() {
        let v1 = json!({
            "id": null,
            "stream": "quotes.raw",
            "version": "v1",
            "event_type": "quote",
            "source": "eastmoney",
            "symbol": "600519",
            "ingest_ts": "2026-10-04T09:30:00Z",
            "payload_hash": "abc123",
            "payload": {"symbol": "600519", "price": 1500.0, "volume": 100},
            "created_at": "2026-10-04T09:30:01Z",
        });
        let envelope: EventEnvelope = serde_json::from_value(v1).expect("v1 消息解码");
        assert_eq!(envelope.version, "v1", "读老消息保留原版本号");
        assert_eq!(envelope.symbol.as_deref(), Some("600519"));
        assert!(envelope.market.is_none());
        assert!(envelope.event_time.is_none());
        assert!(envelope.process_time.is_none());
        assert!(envelope.source_event_id.is_none());
        assert!(envelope.sequence.is_none());
        assert!(envelope.trace_id.is_none());
    }

    /// 新字段全缺省时，序列化字节面必须与 v1 完全一致（无多余键）——
    /// 老版本消费者可原样解码。
    #[test]
    fn serializes_identical_to_v1_when_v2_fields_unset() {
        let envelope = EventEnvelope::new(
            "quotes.raw",
            "quote",
            "eastmoney",
            None,
            json!({"symbol": "600519", "price": 1500.0, "volume": 100}),
        );
        let value: serde_json::Value = serde_json::to_value(&envelope).expect("序列化");
        let obj = value.as_object().expect("对象");
        // v2 字段全未设置 → 键不存在（不被写出）
        for key in [
            "market",
            "event_time",
            "process_time",
            "source_event_id",
            "sequence",
            "trace_id",
        ] {
            assert!(!obj.contains_key(key), "{key} 在缺省时不得写出");
        }
        // v1 既有键集齐全
        assert!(obj.contains_key("id"));
        assert!(obj.contains_key("stream"));
        assert!(obj.contains_key("version"));
        assert!(obj.contains_key("ingest_ts"));
        assert!(obj.contains_key("payload_hash"));
        assert!(obj.contains_key("payload"));
        assert!(obj.contains_key("created_at"));
        // 新字段全 None 时可按 v1 结构回读
        let v1_only: serde_json::Value = value.clone();
        let _: EventEnvelope = serde_json::from_value(v1_only).expect("v1 结构回读");
        // 版本号已升 v2
        assert_eq!(value["version"], "v2");
    }

    /// v2 字段设置后 roundtrip 完整
    #[test]
    fn v2_fields_roundtrip_when_set() {
        let now = Utc::now();
        let envelope = EventEnvelope::new(
            "quotes.normalized",
            "normalized_quote",
            "data-engine",
            Some("600519".into()),
            json!({"price": 1500.5}),
        )
        .with_event_time(now)
        .with_process_time(now)
        .with_source_event_id("src-42")
        .with_market("cn")
        .with_sequence(7)
        .with_trace_id("trace-9");
        let value = serde_json::to_value(&envelope).expect("序列化");
        // serde chrono 输出 Z 后缀（timestamp_micros 精度），与 to_rfc3339
        // 的 +00:00 写法不同但同一时刻；用解析回比（语义断言不锁格式）
        assert_eq!(
            serde_json::from_value::<DateTime<Utc>>(value["event_time"].clone()).expect("解析"),
            now
        );
        assert_eq!(
            serde_json::from_value::<DateTime<Utc>>(value["process_time"].clone()).expect("解析"),
            now
        );
        assert_eq!(value["source_event_id"], "src-42");
        assert_eq!(value["market"], "cn");
        assert_eq!(value["sequence"], 7);
        assert_eq!(value["trace_id"], "trace-9");

        let back: EventEnvelope = serde_json::from_value(value).expect("回读");
        assert_eq!(back.event_time, Some(now));
        assert_eq!(back.process_time, Some(now));
        assert_eq!(back.source_event_id.as_deref(), Some("src-42"));
        assert_eq!(back.market.as_deref(), Some("cn"));
        assert_eq!(back.sequence, Some(7));
        assert_eq!(back.trace_id.as_deref(), Some("trace-9"));
    }

    /// 时间三跳模型：三种时间戳字段各自独立可携带
    #[test]
    fn three_timestamps_are_independent() {
        let event = Utc::now();
        let envelope = EventEnvelope::new("t", "t", "s", None, json!({})).with_event_time(event);
        assert_eq!(envelope.event_time, Some(event));
        assert!(
            envelope.process_time.is_none(),
            "process_time 由处理跳自己打"
        );
        assert!(
            envelope.ingest_ts <= Utc::now(),
            "ingest_ts 由 new() 打为当下"
        );
    }
}
