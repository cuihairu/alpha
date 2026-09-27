//! Redis Streams 队列集成测试
//!
//! 与 redis_kv/timescale 的集成测试一致：仅在设置了 `REDIS_TEST_URL` 时运行，
//! 未设置时自动跳过，保证无外部依赖时 `cargo test` 依旧全绿。

use alpha_storage::{RedisStreamQueue, StreamEnvelope};
use redis::AsyncCommands;
use std::time::Duration;
use uuid::Uuid;

fn test_redis_url() -> Option<String> {
    std::env::var("REDIS_TEST_URL").ok()
}

fn test_queue() -> Option<(RedisStreamQueue, String)> {
    let url = test_redis_url()?;
    let queue = RedisStreamQueue::connect(&url).expect("connect to test redis");
    Some((queue, url))
}

async fn raw_conn(url: &str) -> redis::aio::ConnectionManager {
    redis::Client::open(url)
        .expect("open test redis client")
        .get_connection_manager()
        .await
        .expect("connect test redis")
}

fn unique_stream(prefix: &str) -> String {
    format!("test:{}:{}", prefix, Uuid::new_v4())
}

fn sample_envelope(stream: &str, symbol: &str, price: f64) -> StreamEnvelope {
    StreamEnvelope::new(
        stream,
        "quote",
        "eastmoney",
        Some(symbol.to_string()),
        serde_json::json!({
            "symbol": symbol,
            "price": price,
            "volume": 1000,
            "change": 0.5,
            "change_percent": 4.2
        }),
    )
}

async fn cleanup(url: &str, streams: &[&str]) {
    let mut conn = raw_conn(url).await;
    let _: Result<i64, _> = conn.del(streams).await;
}

#[tokio::test]
async fn publish_and_read_latest_roundtrip() {
    let Some((queue, url)) = test_queue() else {
        eprintln!("skipping: REDIS_TEST_URL not set");
        return;
    };

    let stream = unique_stream("roundtrip");
    let envelope = sample_envelope(&stream, "000001", 12.34);
    let published_id = queue.publish(&stream, &envelope).await.unwrap();

    let messages = queue.read_latest(&stream, 10).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, published_id);

    let decoded = &messages[0].envelope;
    assert_eq!(decoded.event_type, "quote");
    assert_eq!(decoded.source, "eastmoney");
    assert_eq!(decoded.symbol.as_deref(), Some("000001"));
    assert_eq!(decoded.payload["price"], 12.34);
    assert_eq!(decoded.payload_hash, envelope.payload_hash);
    assert_eq!(decoded.ingest_ts, envelope.ingest_ts);
    assert_eq!(decoded.stream, stream);

    cleanup(&url, &[&stream]).await;
}

#[tokio::test]
async fn read_latest_skips_entries_without_payload() {
    let Some((queue, url)) = test_queue() else {
        eprintln!("skipping: REDIS_TEST_URL not set");
        return;
    };

    let stream = unique_stream("sparse");

    // 手工写入一条缺 payload 字段的脏数据，应被解码层跳过
    let mut conn = raw_conn(&url).await;
    let _: String = redis::cmd("XADD")
        .arg(&stream)
        .arg("*")
        .arg("event_type")
        .arg("quote")
        .arg("note")
        .arg("no payload here")
        .query_async(&mut conn)
        .await
        .unwrap();

    queue.publish(&stream, &sample_envelope(&stream, "600000", 8.8)).await.unwrap();

    let messages = queue.read_latest(&stream, 10).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].envelope.payload["symbol"], "600000");

    cleanup(&url, &[&stream]).await;
}

#[tokio::test]
async fn ensure_consumer_group_is_idempotent() {
    let Some((queue, url)) = test_queue() else {
        eprintln!("skipping: REDIS_TEST_URL not set");
        return;
    };

    let stream = unique_stream("groups");
    queue.ensure_consumer_group(&stream, "grp").await.unwrap();
    // 第二次创建返回 BUSYGROUP，应被吞掉而不是报错
    queue.ensure_consumer_group(&stream, "grp").await.unwrap();

    cleanup(&url, &[&stream]).await;
}

#[tokio::test]
async fn consumer_group_delivers_and_acks_messages() {
    let Some((queue, url)) = test_queue() else {
        eprintln!("skipping: REDIS_TEST_URL not set");
        return;
    };

    let stream = unique_stream("consumer");
    queue.ensure_consumer_group(&stream, "grp").await.unwrap();

    queue.publish(&stream, &sample_envelope(&stream, "000001", 10.0)).await.unwrap();
    queue.publish(&stream, &sample_envelope(&stream, "000002", 11.0)).await.unwrap();

    let batch1 = queue
        .read_group(&stream, "grp", "consumer-1", 10, 100)
        .await
        .unwrap();
    assert_eq!(batch1.messages.len(), 2);
    assert!(batch1.invalid.is_empty());

    // ack 后同一消费者再次读取不应拿到已确认消息
    for message in &batch1.messages {
        queue.ack(&stream, "grp", &message.id).await.unwrap();
    }
    let batch2 = queue
        .read_group(&stream, "grp", "consumer-1", 10, 100)
        .await
        .unwrap();
    assert!(batch2.messages.is_empty());
    assert!(batch2.invalid.is_empty());

    // 空读应在 block 窗口内快速返回而不是挂死
    let started = std::time::Instant::now();
    let _ = queue.read_group(&stream, "grp", "consumer-1", 10, 300).await;
    assert!(started.elapsed() < Duration::from_secs(5));

    cleanup(&url, &[&stream]).await;
}

#[tokio::test]
async fn group_created_at_zero_reads_history() {
    // 组在消息之后创建（XGROUP CREATE ... 0）也应能读到历史消息，
    // 与 data-engine/real-time-feed 的重启语义一致。
    let Some((queue, url)) = test_queue() else {
        eprintln!("skipping: REDIS_TEST_URL not set");
        return;
    };

    let stream = unique_stream("history");
    queue.publish(&stream, &sample_envelope(&stream, "600519", 1700.0)).await.unwrap();
    queue.ensure_consumer_group(&stream, "late-grp").await.unwrap();

    let messages = queue
        .read_group(&stream, "late-grp", "consumer-1", 10, 100)
        .await
        .unwrap();
    assert_eq!(messages.messages.len(), 1);
    assert_eq!(messages.messages[0].envelope.payload["symbol"], "600519");

    cleanup(&url, &[&stream]).await;
}

/// 读取消费组 PEL 中未确认消息数（XPENDING 摘要的第一个元素）。
async fn pending_count(url: &str, stream: &str, group: &str) -> i64 {
    let mut conn = raw_conn(url).await;
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
async fn undecodable_entries_are_surfaced_not_poisoning_the_group() {
    let Some((queue, url)) = test_queue() else {
        eprintln!("skipping: REDIS_TEST_URL not set");
        return;
    };

    let stream = unique_stream("quarantine");
    let dlq_stream = unique_stream("quarantine-dlq");
    queue.ensure_consumer_group(&stream, "grp").await.unwrap();

    // 1 条合法消息 + 1 条 payload 为垃圾 JSON + 1 条完全没有 payload 字段
    queue.publish(&stream, &sample_envelope(&stream, "000001", 10.0)).await.unwrap();
    let mut conn = raw_conn(&url).await;
    let _: String = redis::cmd("XADD")
        .arg(&stream)
        .arg("*")
        .arg("event_type")
        .arg("quote")
        .arg("payload")
        .arg("this is not json")
        .query_async(&mut conn)
        .await
        .unwrap();
    let _: String = redis::cmd("XADD")
        .arg(&stream)
        .arg("*")
        .arg("event_type")
        .arg("quote")
        .arg("note")
        .arg("no payload field")
        .query_async(&mut conn)
        .await
        .unwrap();

    let result = queue.read_group(&stream, "grp", "consumer-1", 10, 100).await.unwrap();
    assert_eq!(result.messages.len(), 1, "valid message must still be delivered");
    assert_eq!(result.invalid.len(), 2, "both undecodable entries must be surfaced");

    // 消费者按 DLQ 契约隔离（publish_dlq + ack）
    for invalid in &result.invalid {
        if invalid.raw_payload.is_some() {
            assert!(invalid.reason.contains("decode"), "reason: {}", invalid.reason);
            assert_eq!(invalid.raw_payload.as_deref(), Some("this is not json"));
        } else {
            assert!(invalid.reason.contains("payload"), "reason: {}", invalid.reason);
        }
        queue.publish_dlq(&dlq_stream, invalid).await.unwrap();
        queue.ack(&stream, "grp", &invalid.id).await.unwrap();
    }
    for message in &result.messages {
        queue.ack(&stream, "grp", &message.id).await.unwrap();
    }

    // PEL 清空：解码失败的消息不再永久滞留
    assert_eq!(pending_count(&url, &stream, "grp").await, 0);

    // 再次读取：合法与非法条目都已被消费，组内无剩余
    let again = queue.read_group(&stream, "grp", "consumer-1", 10, 100).await.unwrap();
    assert!(again.is_empty());

    // DLQ 上能看到两条带原因的隔离条目
    let dlq_messages = queue.read_latest(&dlq_stream, 10).await.unwrap();
    assert_eq!(dlq_messages.len(), 2);
    for message in &dlq_messages {
        assert_eq!(message.envelope.event_type, "invalid_message");
        let reason = message.envelope.payload["dlq_reason"].as_str().unwrap();
        assert!(
            reason.contains("decode") || reason.contains("payload"),
            "unexpected dlq_reason: {reason}"
        );
        assert!(!message.envelope.payload["entry_id"].is_null());
    }

    cleanup(&url, &[&stream, &dlq_stream]).await;
}
