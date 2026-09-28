//! normalizer 写侧去重（payload_hash）集成测试
//!
//! 守护 data-engine normalized 层写侧的去重契约：同一 payload 的重复投递
//! （XAUTOCLAIM 重放、上游重复发布）在窗口内只产生一条 normalized 消息，
//! 且原消息必须 ack（否则滞留 PEL 被 sweeper 无限重放）。
//!
//! 去重决策逻辑与 main.rs 的 `process_normalizer_message` 保持一致
//! （二进制 crate 无法被测试直接引用；窗口淘汰行为由 main.rs 单元测试覆盖，
//! 本测试聚焦真实 Redis 上的 hash 回环稳定性 + 去重/ack/读路径语义）。
//!
//! 仅在设置 `REDIS_TEST_URL` 时运行，未设置时自动跳过。

use alpha_storage::{RedisStreamQueue, StreamEnvelope};
use std::collections::HashSet;

fn test_redis_url() -> Option<String> {
    std::env::var("REDIS_TEST_URL").ok()
}

fn unique_stream(prefix: &str) -> String {
    format!("test:normalizer:{}:{}", prefix, uuid::Uuid::new_v4())
}

async fn cleanup(url: &str, streams: &[&str]) {
    let mut conn = redis::Client::open(url)
        .expect("open test redis client")
        .get_connection_manager()
        .await
        .expect("connect test redis");
    let _: Result<i64, _> = redis::AsyncCommands::del(&mut conn, streams).await;
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

fn quote_envelope(stream: &str, symbol: &str, price: f64) -> StreamEnvelope {
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
            "change_percent": 4.2,
            "timestamp": "2026-09-27T08:00:00Z"
        }),
    )
}

#[tokio::test]
async fn duplicate_delivery_yields_single_normalized_entry_and_full_ack() {
    let Some(url) = test_redis_url() else {
        eprintln!("skipping: REDIS_TEST_URL not set");
        return;
    };
    let queue = RedisStreamQueue::connect(&url).unwrap();

    let raw_stream = unique_stream("dedup-raw");
    let normalized_stream = unique_stream("dedup-normalized");
    let group = "data-engine-normalizer";
    queue
        .ensure_consumer_group(&raw_stream, group)
        .await
        .unwrap();

    // 同一 payload 两次投递（两条不同 entry，内容全同 → payload_hash 相同）
    let envelope = quote_envelope(&raw_stream, "000001", 12.34);
    queue.publish(&raw_stream, &envelope).await.unwrap();
    queue.publish(&raw_stream, &envelope).await.unwrap();

    // 消费组读取：两条都送达，且回环解码后 hash 一致（去重契约的前提）
    let consumer = format!("de-test-{}", uuid::Uuid::new_v4());
    let batch = queue
        .read_group(&raw_stream, group, &consumer, 10, 500)
        .await
        .unwrap();
    assert_eq!(batch.messages.len(), 2, "both deliveries must be read");
    assert!(batch.invalid.is_empty());
    let hashes: HashSet<&str> = batch
        .messages
        .iter()
        .map(|m| m.envelope.payload_hash.as_str())
        .collect();
    assert_eq!(
        hashes.len(),
        1,
        "identical payloads must decode to one payload_hash"
    );

    // 镜像 main.rs 的写侧去重决策：窗口内首次 → 发布 normalized + ack；重复 → 仅 ack
    let mut seen: HashSet<String> = HashSet::new();
    for message in &batch.messages {
        if seen.contains(&message.envelope.payload_hash) {
            queue.ack(&raw_stream, group, &message.id).await.unwrap();
            continue;
        }
        let normalized = StreamEnvelope::new(
            &normalized_stream,
            "normalized_quote",
            "data-engine",
            message.envelope.symbol.clone(),
            message.envelope.payload.clone(),
        );
        queue
            .publish(&normalized_stream, &normalized)
            .await
            .unwrap();
        seen.insert(message.envelope.payload_hash.clone());
        queue.ack(&raw_stream, group, &message.id).await.unwrap();
    }

    // 重复投递只落地一条 normalized，原消息全部 ack（PEL 清零，不会被 sweeper 重放）
    assert_eq!(pending_count(&url, &raw_stream, group).await, 0);

    // 去重后读路径一致：下游消费组恰好读到 1 条，payload 与上游一致
    let downstream_group = "downstream-test";
    queue
        .ensure_consumer_group(&normalized_stream, downstream_group)
        .await
        .unwrap();
    let delivered = queue
        .read_group(&normalized_stream, downstream_group, "down-1", 10, 500)
        .await
        .unwrap();
    assert_eq!(
        delivered.messages.len(),
        1,
        "duplicate delivery must yield a single normalized entry"
    );
    assert_eq!(delivered.messages[0].envelope.payload["symbol"], "000001");
    assert_eq!(delivered.messages[0].envelope.payload["price"], 12.34);

    // 窗口内的第三次同内容投递同样会被跳过（读到 0 条新消息）
    queue.publish(&raw_stream, &envelope).await.unwrap();
    let third = queue
        .read_group(&raw_stream, group, &consumer, 10, 500)
        .await
        .unwrap();
    assert_eq!(third.messages.len(), 1);
    assert!(
        seen.contains(&third.messages[0].envelope.payload_hash),
        "same content must stay in window"
    );
    queue
        .ack(&raw_stream, group, &third.messages[0].id)
        .await
        .unwrap();
    assert_eq!(pending_count(&url, &raw_stream, group).await, 0);

    cleanup(&url, &[&raw_stream, &normalized_stream]).await;
}
