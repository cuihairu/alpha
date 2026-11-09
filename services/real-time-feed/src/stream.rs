use std::{sync::Arc, time::Duration};

use alpha_storage::{InvalidMessage, RedisStreamQueue, StreamEnvelope, StreamMessage};
use tokio::time::{interval, MissedTickBehavior};

use crate::alerts::evaluate_alerts;
use crate::{
    AppState, RealTimeData, DEFAULT_REDIS_URL, NORMALIZED_QUOTES_STREAM, QUOTES_DLQ_STREAM,
    QUOTES_STREAM, REALTIME_GROUP,
};

pub(crate) async fn start_stream_consumer(app_state: Arc<AppState>) -> anyhow::Result<()> {
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

    // L464 行情摄入连接状态（RealtimeFeedConnectionLoss 数据源）：
    // Redis Stream 连建成功即视为已连接，轮询失败置 0、恢复置 1
    metrics::gauge!("alpha_realtime_feed_connected").set(1.0);

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
                Ok(result) if !result.is_empty() => {
                    metrics::gauge!("alpha_realtime_feed_connected").set(1.0);
                    result
                }
                _ => match loop_queue
                    .read_group(QUOTES_STREAM, REALTIME_GROUP, &consumer, 20, 1000)
                    .await
                {
                    Ok(result) => {
                        metrics::gauge!("alpha_realtime_feed_connected").set(1.0);
                        result
                    }
                    Err(err) => {
                        metrics::gauge!("alpha_realtime_feed_connected").set(0.0);
                        tracing::warn!("Failed to poll Redis stream: {}", err);
                        continue;
                    }
                },
            };

            for invalid in result.invalid {
                quarantine_invalid(&loop_queue, &invalid).await;
            }
            for message in result.messages {
                // L464 消息吞吐计数（RealtimeFeedMessageGap 的 rate 数据源）
                metrics::counter!("alpha_realtime_messages_total").increment(1);
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
pub(crate) fn claim_sweep_config() -> (u64, u64, u32) {
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
pub(crate) async fn process_realtime_message(
    app_state: &Arc<AppState>,
    queue: &RedisStreamQueue,
    message: &StreamMessage,
) {
    let stream_name = message.envelope.stream.clone();
    if let Some(data) = envelope_to_realtime(&message.envelope) {
        if let Err(err) = app_state.data_sender.send(data.clone()) {
            tracing::debug!("Failed to fan out realtime data: {}", err);
        }
        // 告警评估与行情扇出同拍（L503）：不阻塞 ack 语义
        evaluate_alerts(app_state, &data);
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
pub(crate) async fn quarantine_invalid(queue: &RedisStreamQueue, invalid: &InvalidMessage) {
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

pub(crate) fn envelope_to_realtime(envelope: &StreamEnvelope) -> Option<RealTimeData> {
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

pub(crate) async fn send_to_dlq(
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
