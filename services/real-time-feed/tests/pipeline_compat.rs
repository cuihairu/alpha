//! 管线兼容性集成测试
//!
//! 守护 real-time-feed 与上游 stream（collector 的 raw 行情、data-engine 的 normalized 行情）
//! 之间的 payload 契约：真实 Redis 上发布 → 消费组读取 → 行情转换 → ack，
//! 确保合法消息不会被误送入 quotes.dlq。
//!
//! 转换逻辑与 main.rs 的 `envelope_to_realtime`/`send_to_dlq` 保持一致（二进制 crate 无法
//! 被测试直接引用，字段级行为由 main.rs 单元测试覆盖；本测试聚焦真实 stream 语义 + payload 形状）。
//!
//! 仅在设置 `REDIS_TEST_URL` 时运行，未设置时自动跳过。

use alpha_storage::{RedisStreamQueue, StreamEnvelope};
use std::time::Duration;

/// 与 main.rs `envelope_to_realtime` 相同的必填/缺省口径：
/// symbol/price/volume 必填；change/change_percent 缺省 0。
fn realtime_from_payload(payload: &serde_json::Value) -> Option<(String, f64, u64, f64, f64)> {
    let obj = payload.as_object()?;
    Some((
        obj.get("symbol")?.as_str()?.to_string(),
        obj.get("price")?.as_f64()?,
        obj.get("volume")?.as_u64()?,
        obj.get("change").and_then(|v| v.as_f64()).unwrap_or(0.0),
        obj.get("change_percent").and_then(|v| v.as_f64()).unwrap_or(0.0),
    ))
}

fn test_redis_url() -> Option<String> {
    std::env::var("REDIS_TEST_URL").ok()
}

fn unique_stream(prefix: &str) -> String {
    format!("test:pipeline:{}:{}", prefix, uuid::Uuid::new_v4())
}

async fn cleanup(url: &str, streams: &[&str]) {
    let mut conn = redis::Client::open(url)
        .expect("open test redis client")
        .get_connection_manager()
        .await
        .expect("connect test redis");
    let _: Result<i64, _> = redis::AsyncCommands::del(&mut conn, streams).await;
}

/// 模拟 data-engine normalized payload 的形状：
/// 上游原始字段（change/change_percent/pre_close/name/source）+ 规范化 MarketData 字段。
fn normalized_payload_like_data_engine() -> serde_json::Value {
    serde_json::json!({
        "symbol": "000001",
        "name": "平安银行",
        "pre_close": 12.22,
        "change": 0.12,
        "change_percent": 0.98,
        "source": "eastmoney",
        "timestamp": "2026-09-27T08:00:00Z",
        "price": 12.34,
        "volume": 5000,
        "bid": 12.3,
        "ask": 12.4,
        "open": 12.1,
        "high": 12.5,
        "low": 12.0
    })
}

/// collector 写入 quotes.raw 的 RealtimeQuote 形状。
fn raw_payload_like_collector() -> serde_json::Value {
    serde_json::json!({
        "symbol": "sz000001",
        "name": "平安银行",
        "price": 12.34,
        "pre_close": 12.22,
        "open": 12.1,
        "high": 12.5,
        "low": 12.0,
        "volume": 5000,
        "amount": 61700000.0,
        "change": 0.12,
        "change_percent": 0.98,
        "bid1": 12.33,
        "ask1": 12.35,
        "bid1_volume": 1200,
        "ask1_volume": 900,
        "timestamp": "2026-09-27T08:00:00Z",
        "source": "eastmoney"
    })
}

/// 消费组读取唯一消息、转换、ack（与 main.rs 消费循环同构）。
async fn consume_and_convert(
    queue: &RedisStreamQueue,
    stream: &str,
) -> Option<(String, f64, u64, f64, f64)> {
    queue.ensure_consumer_group(stream, "real-time-feed").await.unwrap();
    let consumer = format!("rtf-test-{}", uuid::Uuid::new_v4());
    let result = queue
        .read_group(stream, "real-time-feed", &consumer, 10, 500)
        .await
        .unwrap();
    assert!(result.invalid.is_empty(), "expected no undecodable entries on {}", stream);
    assert_eq!(result.messages.len(), 1, "expected exactly one message on {}", stream);

    let converted = realtime_from_payload(&result.messages[0].envelope.payload);
    queue.ack(stream, "real-time-feed", &result.messages[0].id).await.unwrap();
    converted
}

#[tokio::test]
async fn normalized_stream_message_is_consumable_not_dlqd() {
    // 回归守护：normalized payload 无 change 字段时，整条消息曾被误送 quotes.dlq。
    let Some(url) = test_redis_url() else {
        eprintln!("skipping: REDIS_TEST_URL not set");
        return;
    };
    let queue = RedisStreamQueue::connect(&url).unwrap();

    let stream = unique_stream("normalized");
    let envelope = StreamEnvelope::new(
        &stream,
        "normalized_quote",
        "data-engine",
        Some("000001".to_string()),
        normalized_payload_like_data_engine(),
    );
    queue.publish(&stream, &envelope).await.unwrap();

    let (symbol, price, volume, change, change_percent) =
        consume_and_convert(&queue, &stream).await.expect("normalized quote must be consumable");
    assert_eq!(symbol, "000001");
    assert_eq!(price, 12.34);
    assert_eq!(volume, 5000);
    assert_eq!(change, 0.12);
    assert_eq!(change_percent, 0.98);

    cleanup(&url, &[&stream]).await;
}

#[tokio::test]
async fn raw_stream_message_is_consumable_not_dlqd() {
    let Some(url) = test_redis_url() else {
        eprintln!("skipping: REDIS_TEST_URL not set");
        return;
    };
    let queue = RedisStreamQueue::connect(&url).unwrap();

    let stream = unique_stream("raw");
    let envelope = StreamEnvelope::new(
        &stream,
        "quote",
        "eastmoney",
        Some("sz000001".to_string()),
        raw_payload_like_collector(),
    );
    queue.publish(&stream, &envelope).await.unwrap();

    let (symbol, price, volume, change, change_percent) =
        consume_and_convert(&queue, &stream).await.expect("raw quote must be consumable");
    assert_eq!(symbol, "sz000001");
    assert_eq!(price, 12.34);
    assert_eq!(volume, 5000);
    assert_eq!(change, 0.12);
    assert_eq!(change_percent, 0.98);

    cleanup(&url, &[&stream]).await;
}

#[tokio::test]
async fn malformed_message_follows_dlq_policy() {
    // 缺 price 的消息转换失败 → 走 DLQ：转发 quotes.dlq（带原因）后 ack 原消息，
    // 与 main.rs 的 `send_to_dlq` + ack 语义一致。
    let Some(url) = test_redis_url() else {
        eprintln!("skipping: REDIS_TEST_URL not set");
        return;
    };
    let queue = RedisStreamQueue::connect(&url).unwrap();

    let stream = unique_stream("malformed");
    let dlq_stream = unique_stream("malformed-dlq");
    let envelope = StreamEnvelope::new(
        &stream,
        "quote",
        "eastmoney",
        None,
        serde_json::json!({ "symbol": "000001", "volume": 10 }),
    );
    queue.publish(&stream, &envelope).await.unwrap();

    queue.ensure_consumer_group(&stream, "real-time-feed").await.unwrap();
    let result = queue
        .read_group(&stream, "real-time-feed", "rtf-test", 10, 500)
        .await
        .unwrap();
    assert_eq!(result.messages.len(), 1);

    let converted = realtime_from_payload(&result.messages[0].envelope.payload);
    assert!(converted.is_none(), "malformed message must fail conversion and hit DLQ path");

    let mut payload = result.messages[0].envelope.payload.clone();
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("dlq_reason".to_string(), serde_json::json!("invalid realtime quote payload"));
    }
    let dlq_envelope = StreamEnvelope::new(
        &dlq_stream,
        result.messages[0].envelope.event_type.clone(),
        result.messages[0].envelope.source.clone(),
        result.messages[0].envelope.symbol.clone(),
        payload,
    );
    queue.publish(&dlq_stream, &dlq_envelope).await.unwrap();
    queue.ack(&stream, "real-time-feed", &result.messages[0].id).await.unwrap();

    let dlq_messages = queue.read_latest(&dlq_stream, 10).await.unwrap();
    assert_eq!(dlq_messages.len(), 1);
    assert_eq!(dlq_messages[0].envelope.payload["dlq_reason"], "invalid realtime quote payload");
    assert_eq!(dlq_messages[0].envelope.payload["symbol"], "000001");

    cleanup(&url, &[&stream, &dlq_stream]).await;
}

#[tokio::test]
async fn undecodable_entry_is_quarantined_and_acked() {
    // 回归守护：损坏生产者写出的垃圾 payload 曾导致整批读取失败、
    // 消息滞留消费组 PEL 永不清理；现在应被隔离到 DLQ 并 ack（与 main.rs 消费循环同构）。
    let Some(url) = test_redis_url() else {
        eprintln!("skipping: REDIS_TEST_URL not set");
        return;
    };
    let queue = RedisStreamQueue::connect(&url).unwrap();

    let stream = unique_stream("undecodable");
    let dlq_stream = unique_stream("undecodable-dlq");

    let mut conn = redis::Client::open(url.as_str())
        .unwrap()
        .get_connection_manager()
        .await
        .unwrap();
    let _: String = redis::cmd("XADD")
        .arg(&stream)
        .arg("*")
        .arg("event_type")
        .arg("quote")
        .arg("payload")
        .arg("{not json")
        .query_async(&mut conn)
        .await
        .unwrap();

    queue.ensure_consumer_group(&stream, "real-time-feed").await.unwrap();
    let result = queue
        .read_group(&stream, "real-time-feed", "rtf-test", 10, 500)
        .await
        .unwrap();
    assert!(result.messages.is_empty());
    assert_eq!(result.invalid.len(), 1, "undecodable entry must be surfaced, not fail the batch");

    // 消费者隔离路径：publish_dlq + ack
    let invalid = &result.invalid[0];
    assert!(invalid.reason.contains("decode"), "reason: {}", invalid.reason);
    assert_eq!(invalid.raw_payload.as_deref(), Some("{not json"));
    queue.publish_dlq(&dlq_stream, invalid).await.unwrap();
    queue.ack(&stream, "real-time-feed", &invalid.id).await.unwrap();

    // PEL 清空，组内无剩余条目
    let pending: redis::Value = redis::cmd("XPENDING")
        .arg(&stream)
        .arg("real-time-feed")
        .query_async(&mut conn)
        .await
        .unwrap();
    let count = match &pending {
        redis::Value::Bulk(items) => match items.first() {
            Some(redis::Value::Int(n)) => *n,
            other => panic!("unexpected XPENDING first element: {other:?}"),
        },
        other => panic!("unexpected XPENDING reply: {other:?}"),
    };
    assert_eq!(count, 0, "quarantined message must not remain pending");
    let again = queue
        .read_group(&stream, "real-time-feed", "rtf-test-2", 10, 100)
        .await
        .unwrap();
    assert!(again.is_empty());

    // DLQ 条目带原因与原载荷
    let dlq_messages = queue.read_latest(&dlq_stream, 10).await.unwrap();
    assert_eq!(dlq_messages.len(), 1);
    assert_eq!(dlq_messages[0].envelope.event_type, "invalid_message");
    assert!(
        dlq_messages[0].envelope.payload["dlq_reason"]
            .as_str()
            .unwrap()
            .contains("decode")
    );
    assert_eq!(dlq_messages[0].envelope.payload["original_payload"], "{not json");
    assert_eq!(dlq_messages[0].envelope.payload["original_stream"], stream);

    cleanup(&url, &[&stream, &dlq_stream]).await;
}

/// 读取消费组 PEL 中未确认消息数（XPENDING 摘要的第一个元素）。
async fn pending_count(url: &str, stream: &str, group: &str) -> i64 {
    let mut conn = redis::Client::open(url)
        .expect("open test redis client")
        .get_connection_manager()
        .await
        .expect("connect test redis");
    let reply: redis::Value = redis::cmd("XPENDING")
        .arg(stream)
        .arg(group)
        .query_async(&mut conn)
        .await
        .expect("XPENDING");
    match reply {
        redis::Value::Bulk(items) => match items.first() {
            Some(redis::Value::Int(n)) => *n,
            other => panic!("unexpected XPENDING first element: {other:?}"),
        },
        other => panic!("unexpected XPENDING reply: {other:?}"),
    }
}

#[tokio::test]
async fn orphaned_pending_message_is_claimed_and_reprocessed() {
    // 崩溃遗留场景镜像（与 main.rs 的 sweeper 任务同构）：
    // 消费端「已投递、未 ack」→ PEL 滞留 → 周期 XAUTOCLAIM 认领 →
    // 转换/广播路径可正常处理 → ack 后 XPENDING 清零。
    let Some(url) = test_redis_url() else {
        eprintln!("skipping: REDIS_TEST_URL not set");
        return;
    };
    let queue = RedisStreamQueue::connect(&url).unwrap();

    let stream = unique_stream("orphan");

    let envelope = StreamEnvelope::new(
        &stream,
        "quote",
        "eastmoney",
        Some("sz000001".to_string()),
        raw_payload_like_collector(),
    );
    queue.publish(&stream, &envelope).await.unwrap();

    // 模拟崩溃：读走但不 ack
    queue.ensure_consumer_group(&stream, "real-time-feed").await.unwrap();
    let crashed = queue
        .read_group(&stream, "real-time-feed", "rtf-crashed", 10, 500)
        .await
        .unwrap();
    assert_eq!(crashed.messages.len(), 1);
    assert_eq!(pending_count(&url, &stream, "real-time-feed").await, 1);

    // 闲置未过阈值不认领
    let not_yet = queue
        .claim_stale(&stream, "real-time-feed", "rtf-sweep", 60_000, 10, 5)
        .await
        .unwrap();
    assert!(not_yet.is_empty());
    assert_eq!(pending_count(&url, &stream, "real-time-feed").await, 1);

    // 闲置超过阈值：认领后按既有转换路径重放（合法行情不被误入 DLQ），随后 ack 清零
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let claimed = queue
        .claim_stale(&stream, "real-time-feed", "rtf-sweep", 1000, 10, 5)
        .await
        .unwrap();
    assert_eq!(claimed.messages.len(), 1, "orphaned message must be reclaimed");
    assert!(claimed.invalid.is_empty());

    let (symbol, price, volume, change, change_percent) =
        realtime_from_payload(&claimed.messages[0].envelope.payload)
            .expect("reclaimed message must be consumable via normal conversion path");
    assert_eq!(symbol, "sz000001");
    assert_eq!(price, 12.34);
    assert_eq!(volume, 5000);
    assert_eq!(change, 0.12);
    assert_eq!(change_percent, 0.98);

    queue.ack(&stream, "real-time-feed", &claimed.messages[0].id).await.unwrap();
    assert_eq!(pending_count(&url, &stream, "real-time-feed").await, 0);

    cleanup(&url, &[&stream]).await;
}
