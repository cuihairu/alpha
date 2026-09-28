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
#[derive(Debug, Clone, Default)]
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
        let payload = serde_json::to_string(envelope).map_err(|e| {
            AlphaError::StorageError(format!("serialize stream envelope failed: {e}"))
        })?;

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

    pub async fn read_latest(&self, stream: &str, count: usize) -> AlphaResult<Vec<StreamMessage>> {
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

    pub async fn ensure_consumer_group(&self, stream: &str, group: &str) -> AlphaResult<()> {
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

        let mut result = GroupRead::default();
        for key in entries.keys {
            for entry in key.ids {
                let field_pairs = entry
                    .map
                    .into_iter()
                    .filter_map(|(k, v)| {
                        redis::from_redis_value::<String>(&v).ok().map(|vv| (k, vv))
                    })
                    .collect::<Vec<_>>();
                Self::push_decoded(&key.key, entry.id, field_pairs, &mut result);
            }
        }
        Ok(result)
    }

    /// 周期兜底：认领组内闲置超过 `min_idle_ms` 的 pending 条目（典型场景：消费端在
    /// 「已投递、未 ack」之间崩溃，消息滞留 PEL 永不清理），并按投递次数封顶：
    ///
    /// * 投递预算：`max_delivery_count` 为单条消息累计投递次数（含首次投递）上限，
    ///   按 XPENDING 明细行的 delivery_count 判定。达到上限仍未被 ack 的条目判定为
    ///   「毒消息」：不再认领重投，而是作为 `invalid`（reason 标明 delivery_count 与
    ///   cap）交回调用者按既有 DLQ 契约处置（publish_dlq + ack），杜绝反复处理失败的
    ///   消息被无限认领重放。DLQ 发布失败时调用者不 ack，条目留在 PEL，下一轮扫描
    ///   会再次尝试转 DLQ（毒消息不占认领配额，扫描分页越过，单轮扫描总量设上界）。
    /// * 认领配额：`count` 为单轮最多认领重投的条目数（毒消息不计入）。
    ///
    /// 与「解码失败 → DLQ」仍是两条独立路径：本方法把孤儿消息重新交付给调用者，
    /// 认领后发现解码失败的条目同样走 invalid 契约。条目本体已被 XDEL 删除的
    /// pending 项会被直接 ack 清理（无可重处理内容）。
    ///
    /// 实现：XPENDING(IDLE 过滤) 分页扫描 + 按条目 XCLAIM 精确认领（此前为 XAUTOCLAIM，
    /// 但它无法按 delivery_count 过滤，认领即递增计数、无法反悔）。XCLAIM 保留
    /// min-idle 门槛避免与并发认领者竞争；应答中的 Nil 先核实条目确实不存在（XDEL
    /// 空壳）才 ack，竞争失败（条目仍在、已归他人）的条目不动。
    ///
    /// 需要 Redis ≥ 6.2（XPENDING IDLE 过滤 / 排他区间）；服务端不支持时返回
    /// StorageError，调用方按需降级。
    pub async fn claim_stale(
        &self,
        stream: &str,
        group: &str,
        consumer: &str,
        min_idle_ms: u64,
        count: usize,
        max_delivery_count: u32,
    ) -> AlphaResult<GroupRead> {
        let mut conn = self.get_conn().await?;

        // 1) XPENDING 明细分页：收集 idle ≥ min_idle 的可认领条目与毒消息。
        //    排他起点逐条推进，毒消息不阻塞后续条目的认领配额。
        let page_size = count.clamp(16, 1000);
        let max_scan = count.saturating_mul(10).max(128);
        let mut scanned = 0usize;
        let mut cursor = "-".to_string();
        let mut reclaimable: Vec<String> = Vec::new();
        let mut poisoned: Vec<(String, u64)> = Vec::new();

        while reclaimable.len() < count && scanned < max_scan {
            let rows =
                Self::pending_rows(&mut conn, stream, group, min_idle_ms, &cursor, page_size)
                    .await?;
            if rows.is_empty() {
                break;
            }
            let page_len = rows.len();
            for (id, delivery_count) in rows {
                scanned += 1;
                cursor = format!("({id}"); // 排他区间：下一页从该条目之后继续
                if delivery_count >= u64::from(max_delivery_count) {
                    poisoned.push((id, delivery_count));
                } else {
                    reclaimable.push(id);
                    if reclaimable.len() >= count {
                        break;
                    }
                }
            }
            if page_len < page_size {
                break;
            }
        }

        // 2) XCLAIM 精确认领预算内条目（min-idle 再次校验，防止并发认领者竞争）
        let mut result = GroupRead::default();
        if !reclaimable.is_empty() {
            let mut cmd = redis::cmd("XCLAIM");
            cmd.arg(stream).arg(group).arg(consumer).arg(min_idle_ms);
            for id in &reclaimable {
                cmd.arg(id);
            }
            let reply: redis::Value = cmd
                .query_async(&mut conn)
                .await
                .map_err(|e| AlphaError::StorageError(format!("redis XCLAIM failed: {e}")))?;

            let items = match reply {
                redis::Value::Bulk(items) => items,
                _ => return Ok(result),
            };
            for entry in items {
                let pair = match entry {
                    redis::Value::Bulk(pair) if pair.len() == 2 => pair,
                    _ => continue,
                };
                let id = match &pair[0] {
                    redis::Value::Data(bytes) => String::from_utf8_lossy(bytes).to_string(),
                    _ => continue,
                };

                match &pair[1] {
                    // Nil：仅当条目本体确已删除（XDEL 空壳）才 ack 清理；
                    // min-idle 竞争失败（条目仍在、已归其他消费者）时 ack 会
                    // 破坏他人的 pending 状态，必须先核实。
                    redis::Value::Nil => {
                        if self.entry_fields(&mut conn, stream, &id).await?.is_empty() {
                            self.ack(stream, group, &id).await?;
                        }
                    }
                    redis::Value::Bulk(flat) => {
                        let field_pairs = flat
                            .chunks_exact(2)
                            .filter_map(|pair| {
                                let key = redis::from_redis_value::<String>(&pair[0]).ok()?;
                                let value = redis::from_redis_value::<String>(&pair[1]).ok()?;
                                Some((key, value))
                            })
                            .collect::<Vec<_>>();
                        Self::push_decoded(stream, id, field_pairs, &mut result);
                    }
                    _ => continue,
                }
            }
        }

        // 3) 毒消息：不认领、不重投；带原因转 invalid 交调用者按 DLQ 契约处置。
        //    条目本体若已被 XDEL 则只剩 PEL 空壳，直接 ack 清理。
        for (id, delivery_count) in poisoned {
            let fields = self.entry_fields(&mut conn, stream, &id).await?;
            if fields.is_empty() {
                self.ack(stream, group, &id).await?;
                continue;
            }
            let fields: BTreeMap<String, String> = fields.into_iter().collect();
            tracing::warn!(
                stream = %stream,
                group = %group,
                entry_id = %id,
                delivery_count,
                cap = max_delivery_count,
                "poison message: delivery count reached cap, redelivery stopped; routing to DLQ"
            );
            result.invalid.push(InvalidMessage {
                id,
                stream: stream.to_string(),
                reason: format!(
                    "delivery_count {delivery_count} reached cap {max_delivery_count} (poison message; redelivery stopped)"
                ),
                raw_payload: fields.get("payload").cloned(),
                fields,
            });
        }

        Ok(result)
    }

    /// XPENDING 明细单页：idle ≥ `min_idle_ms` 的 pending 行（条目 ID + delivery_count）。
    async fn pending_rows(
        conn: &mut redis::aio::ConnectionManager,
        stream: &str,
        group: &str,
        min_idle_ms: u64,
        start: &str,
        count: usize,
    ) -> AlphaResult<Vec<(String, u64)>> {
        let reply: redis::Value = redis::cmd("XPENDING")
            .arg(stream)
            .arg(group)
            .arg("IDLE")
            .arg(min_idle_ms)
            .arg(start)
            .arg("+")
            // 注意：XPENDING 扩展形态的 count 是位置参数，不带 COUNT 关键字
            // （与 XRANGE/XAUTOCLAIM 的 KEYWORD count 语法不同）
            .arg(count)
            .query_async(conn)
            .await
            .map_err(|e| AlphaError::StorageError(format!("redis XPENDING failed: {e}")))?;

        let rows = match reply {
            redis::Value::Bulk(rows) => rows,
            _ => return Ok(Vec::new()),
        };
        // 明细行：[id, consumer, idle_ms, delivery_count]
        Ok(rows
            .into_iter()
            .filter_map(|row| match row {
                redis::Value::Bulk(cells) if cells.len() >= 4 => {
                    let id = match &cells[0] {
                        redis::Value::Data(bytes) => String::from_utf8_lossy(bytes).to_string(),
                        _ => return None,
                    };
                    let delivery_count = match &cells[3] {
                        redis::Value::Int(n) => *n as u64,
                        redis::Value::Data(bytes) => String::from_utf8_lossy(bytes).parse().ok()?,
                        _ => return None,
                    };
                    Some((id, delivery_count))
                }
                _ => None,
            })
            .collect())
    }

    /// 读取单条条目的全部字段（XRANGE 闭区间查询）；条目不存在时返回空。
    async fn entry_fields(
        &self,
        conn: &mut redis::aio::ConnectionManager,
        stream: &str,
        id: &str,
    ) -> AlphaResult<Vec<(String, String)>> {
        let reply: StreamRangeReply = redis::cmd("XRANGE")
            .arg(stream)
            .arg(id)
            .arg(id)
            .query_async(conn)
            .await
            .map_err(|e| AlphaError::StorageError(format!("redis XRANGE failed: {e}")))?;
        Ok(reply
            .ids
            .into_iter()
            .flat_map(|entry| {
                entry.map.into_iter().filter_map(|(k, v)| {
                    redis::from_redis_value::<String>(&v).ok().map(|vv| (k, vv))
                })
            })
            .collect())
    }

    /// 单条条目解码入库：可解码进 messages，失败进 invalid（不中断批次）。
    fn push_decoded(
        stream: &str,
        id: String,
        field_pairs: Vec<(String, String)>,
        result: &mut GroupRead,
    ) {
        let fields: BTreeMap<String, String> = field_pairs.iter().cloned().collect();

        match Self::decode_envelope(stream, &id, field_pairs) {
            Ok(Some(envelope)) => result.messages.push(StreamMessage { id, envelope }),
            // 单条条目解码失败不再让整批读取报错（此前会导致消息滞留 PEL 永不清理），
            // 而是作为 invalid 交给消费者按 DLQ 契约处置。
            Ok(None) => result.invalid.push(InvalidMessage {
                id,
                stream: stream.to_string(),
                reason: "missing `payload` field".to_string(),
                raw_payload: None,
                fields,
            }),
            Err(err) => result.invalid.push(InvalidMessage {
                id,
                stream: stream.to_string(),
                reason: err.to_string(),
                raw_payload: fields.get("payload").cloned(),
                fields,
            }),
        }
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
