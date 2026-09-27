//! Redis Streams 队列支持

use alpha_core::errors::{AlphaError, AlphaResult};
use chrono::{DateTime, Utc};
use redis::{
    streams::{StreamRangeReply, StreamReadOptions, StreamReadReply},
    AsyncCommands,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamEnvelope {
    pub id: Option<String>,
    pub stream: String,
    pub version: String,
    pub event_type: String,
    pub source: String,
    pub symbol: Option<String>,
    pub ingest_ts: DateTime<Utc>,
    pub payload_hash: String,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

impl StreamEnvelope {
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
            version: "v1".to_string(),
            event_type: event_type.into(),
            source: source.into(),
            symbol,
            ingest_ts: Utc::now(),
            payload_hash: payload_hash(&payload),
            payload,
            created_at: Utc::now(),
        }
    }
}

#[derive(Clone)]
pub struct RedisStreamQueue {
    client: redis::Client,
}

#[derive(Debug, Clone)]
pub struct StreamMessage {
    pub id: String,
    pub envelope: StreamEnvelope,
}

/// 一条无法解码为 `StreamEnvelope` 的流条目。
/// 由消费者按 DLQ 契约处置（publish_dlq + ack），避免滞留消费组 PEL。
#[derive(Debug, Clone)]
pub struct InvalidMessage {
    /// Redis stream 条目 ID
    pub id: String,
    /// 来源 stream
    pub stream: String,
    /// 解码失败原因（人类可读，写入 DLQ 载荷的 dlq_reason）
    pub reason: String,
    /// 原始 payload 内容（存在且为字符串时保留，便于回溯）
    pub raw_payload: Option<String>,
    /// 条目其余原始字段（source/symbol 等顶层字段仍可提取）
    pub fields: BTreeMap<String, String>,
}

/// 一次消费组读取的结果：可正常解码的消息 + 无法解码的条目。
#[derive(Debug, Clone)]
pub struct GroupRead {
    pub messages: Vec<StreamMessage>,
    pub invalid: Vec<InvalidMessage>,
}

impl GroupRead {
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty() && self.invalid.is_empty()
    }
}

impl RedisStreamQueue {
    pub fn connect(connection_string: &str) -> AlphaResult<Self> {
        let client = redis::Client::open(connection_string)
            .map_err(|e| AlphaError::ConfigurationError(format!("invalid redis URL: {e}")))?;
        Ok(Self { client })
    }

    async fn get_conn(&self) -> AlphaResult<redis::aio::ConnectionManager> {
        self.client
            .get_connection_manager()
            .await
            .map_err(|e| AlphaError::StorageError(format!("redis connect failed: {e}")))
    }

    pub async fn publish(&self, stream: &str, envelope: &StreamEnvelope) -> AlphaResult<String> {
        let mut conn = self.get_conn().await?;
        let payload = serde_json::to_string(envelope)
            .map_err(|e| AlphaError::StorageError(format!("serialize stream envelope failed: {e}")))?;

        let mut fields = BTreeMap::new();
        fields.insert("event_type", envelope.event_type.clone());
        fields.insert("source", envelope.source.clone());
        fields.insert("payload", payload);
        fields.insert("created_at", envelope.created_at.to_rfc3339());
        fields.insert("version", envelope.version.clone());
        fields.insert("ingest_ts", envelope.ingest_ts.to_rfc3339());
        fields.insert("payload_hash", envelope.payload_hash.clone());
        fields.insert("stream", envelope.stream.clone());
        if let Some(symbol) = &envelope.symbol {
            fields.insert("symbol", symbol.clone());
        }

        let mut cmd = redis::cmd("XADD");
        cmd.arg(stream).arg("*");
        for (key, value) in fields {
            cmd.arg(key).arg(value);
        }

        let id: String = cmd
            .query_async(&mut conn)
            .await
            .map_err(|e| AlphaError::StorageError(format!("redis XADD failed: {e}")))?;

        Ok(id)
    }

    pub async fn read_latest(
        &self,
        stream: &str,
        count: usize,
    ) -> AlphaResult<Vec<StreamMessage>> {
        let mut conn = self.get_conn().await?;
        let entries: StreamRangeReply = redis::cmd("XREVRANGE")
            .arg(stream)
            .arg("+")
            .arg("-")
            .arg("COUNT")
            .arg(count)
            .query_async(&mut conn)
            .await
            .map_err(|e| AlphaError::StorageError(format!("redis XREVRANGE failed: {e}")))?;

        let mut result = Vec::new();
        for entry in entries.ids {
            let fields = entry
                .map
                .into_iter()
                .filter_map(|(k, v)| redis::from_redis_value::<String>(&v).ok().map(|vv| (k, vv)))
                .collect::<Vec<_>>();
            if let Some(envelope) = Self::decode_envelope(stream, &entry.id, fields)? {
                result.push(StreamMessage {
                    id: entry.id,
                    envelope,
                });
            }
        }
        Ok(result)
    }

    pub async fn ensure_consumer_group(
        &self,
        stream: &str,
        group: &str,
    ) -> AlphaResult<()> {
        let mut conn = self.get_conn().await?;
        let result: Result<String, redis::RedisError> = redis::cmd("XGROUP")
            .arg("CREATE")
            .arg(stream)
            .arg(group)
            .arg("0")
            .arg("MKSTREAM")
            .query_async(&mut conn)
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(err) if err.to_string().contains("BUSYGROUP") => Ok(()),
            Err(err) => Err(AlphaError::StorageError(format!(
                "redis XGROUP CREATE failed: {err}"
            ))),
        }
    }

    pub async fn read_group(
        &self,
        stream: &str,
        group: &str,
        consumer: &str,
        count: usize,
        block_ms: usize,
    ) -> AlphaResult<GroupRead> {
        let mut conn = self.get_conn().await?;
        let options = StreamReadOptions::default()
            .group(group, consumer)
            .count(count)
            .block(block_ms);
        let entries: StreamReadReply = conn
            .xread_options(&[stream], &[">"], &options)
            .await
            .map_err(|e| AlphaError::StorageError(format!("redis XREADGROUP failed: {e}")))?;

        let mut result = GroupRead {
            messages: Vec::new(),
            invalid: Vec::new(),
        };
        for key in entries.keys {
            for entry in key.ids {
                let field_pairs = entry
                    .map
                    .into_iter()
                    .filter_map(|(k, v)| {
                        redis::from_redis_value::<String>(&v)
                            .ok()
                            .map(|vv| (k, vv))
                    })
                    .collect::<Vec<_>>();
                let fields: BTreeMap<String, String> =
                    field_pairs.iter().cloned().collect();

                match Self::decode_envelope(&key.key, &entry.id, field_pairs) {
                    Ok(Some(envelope)) => result.messages.push(StreamMessage {
                        id: entry.id,
                        envelope,
                    }),
                    // 单条条目解码失败不再让整批读取报错（此前会导致消息滞留 PEL 永不清理），
                    // 而是作为 invalid 交给消费者按 DLQ 契约处置。
                    Ok(None) => result.invalid.push(InvalidMessage {
                        id: entry.id,
                        stream: key.key.clone(),
                        reason: "missing `payload` field".to_string(),
                        raw_payload: None,
                        fields,
                    }),
                    Err(err) => result.invalid.push(InvalidMessage {
                        id: entry.id,
                        stream: key.key.clone(),
                        reason: err.to_string(),
                        raw_payload: fields.get("payload").cloned(),
                        fields,
                    }),
                }
            }
        }
        Ok(result)
    }

    /// 将一条无法解码的条目投递到独立 DLQ stream（仓库契约：`quotes.dlq` 这类独立 stream）。
    /// 载荷保留失败原因、原始 stream/条目 ID 与原 payload，便于回溯与人工排查。
    pub async fn publish_dlq(
        &self,
        dlq_stream: &str,
        invalid: &InvalidMessage,
    ) -> AlphaResult<String> {
        let payload = serde_json::json!({
            "dlq_reason": invalid.reason,
            "original_stream": invalid.stream,
            "entry_id": invalid.id,
            "original_payload": invalid.raw_payload,
        });
        let envelope = StreamEnvelope::new(
            dlq_stream,
            "invalid_message",
            invalid
                .fields
                .get("source")
                .cloned()
                .unwrap_or_else(|| "unknown".to_string()),
            invalid.fields.get("symbol").cloned(),
            payload,
        );
        self.publish(dlq_stream, &envelope).await
    }

    pub async fn ack(&self, stream: &str, group: &str, id: &str) -> AlphaResult<()> {
        let mut conn = self.get_conn().await?;
        conn.xack::<_, _, _, i64>(stream, group, &[id])
            .await
            .map_err(|e| AlphaError::StorageError(format!("redis XACK failed: {e}")))?;
        Ok(())
    }

    fn decode_envelope(
        stream: &str,
        id: &str,
        fields: Vec<(String, String)>,
    ) -> AlphaResult<Option<StreamEnvelope>> {
        let mut map = BTreeMap::new();
        for (key, value) in fields {
            map.insert(key, value);
        }

        let Some(payload) = map.get("payload") else {
            return Ok(None);
        };

        let mut envelope: StreamEnvelope = serde_json::from_str(payload)
            .map_err(|e| AlphaError::StorageError(format!("decode stream payload failed: {e}")))?;
        envelope.id = Some(id.to_string());
        envelope.stream = map
            .get("stream")
            .cloned()
            .unwrap_or_else(|| stream.to_string());
        if let Some(version) = map.get("version") {
            envelope.version = version.clone();
        }
        if let Some(hash) = map.get("payload_hash") {
            envelope.payload_hash = hash.clone();
        }
        if let Some(ingest_ts) = map.get("ingest_ts") {
            envelope.ingest_ts = ingest_ts
                .parse()
                .map_err(|e| AlphaError::SerializationError(format!("invalid ingest_ts: {e}")))?;
        }
        Ok(Some(envelope))
    }
}

fn payload_hash(payload: &serde_json::Value) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    payload.to_string().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
