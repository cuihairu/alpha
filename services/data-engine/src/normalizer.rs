//! normalized 层消费与转发：Redis 消费组拉取 quotes.raw → normalize →
//! 内存 serving 写入 + normalized 转发 + DLQ 隔离；镜像失败补写与湖维护
//! 两个后台 worker；/metrics、/health 运维端点（原 main.rs 811-1310 行）。

use std::{sync::Arc, time::Duration as StdDuration};

use crate::outlier::OutlierObservation;
use crate::sequence_gap::SequenceObservation;
use crate::source_divergence::DivergenceObservation;
use crate::MirrorRetryBuffer;
use crate::{
    AppState, DEFAULT_REDIS_URL, MIRROR_RETRY_BATCH, MIRROR_RETRY_INTERVAL_SECS,
    NORMALIZED_QUOTES_STREAM, NORMALIZER_GROUP, QUOTES_DLQ_STREAM, RAW_QUOTES_STREAM,
};
use alpha_core::models::MarketData;
use alpha_storage::lake::{CompactionOptions, LakeWriter};
use alpha_storage::{
    InvalidMessage, RedisStreamQueue, StreamEnvelope, StreamMessage, TimescaleTimeSeriesStorage,
};
use axum::{extract::State, Json};
use chrono::{DateTime, Utc};
use tokio::time::{interval, MissedTickBehavior};

pub(crate) async fn start_quote_normalizer(state: Arc<AppState>) -> anyhow::Result<()> {
    let redis_url = std::env::var("ALPHA_REDIS_URL")
        .or_else(|_| std::env::var("REDIS_URL"))
        .unwrap_or_else(|_| DEFAULT_REDIS_URL.to_string());
    let queue = RedisStreamQueue::connect(&redis_url)?;
    queue
        .ensure_consumer_group(RAW_QUOTES_STREAM, NORMALIZER_GROUP)
        .await?;
    let consumer = format!("de-{}", uuid::Uuid::new_v4());

    let sweeper_enabled = state.config.sweeper.enabled;
    let min_idle_ms = state.config.sweeper.min_idle_ms;
    let max_delivery_count = state.config.sweeper.max_delivery_count;
    let sweep_interval = StdDuration::from_secs(state.config.sweeper.interval_secs.max(1));
    let sweeper_state = state.clone();
    let sweeper_queue = queue.clone();

    let loop_queue = queue.clone();
    tokio::spawn(async move {
        loop {
            match loop_queue
                .read_group(RAW_QUOTES_STREAM, NORMALIZER_GROUP, &consumer, 50, 1000)
                .await
            {
                Ok(result) => {
                    for invalid in result.invalid {
                        quarantine_invalid(&loop_queue, &invalid).await;
                    }
                    for message in result.messages {
                        process_normalizer_message(&state, &loop_queue, &message).await;
                    }
                }
                Err(err) => tracing::warn!("Quote normalizer read failed: {}", err),
            }
        }
    });

    // 周期兜底：消费端崩溃遗留的「已投递、未 ack」孤儿 pending 由独立的 sweeper 认领后
    // 重放，处理路径与实时消费完全一致；投递次数达到 sweeper.max_delivery_count 仍
    // pending 的「毒消息」不再重投，走 DLQ 契约隔离（注意：这与解码失败 → DLQ 是
    // 两条不同路径，后者发生在读取时，见 quarantine_invalid/process_normalizer_message）。
    if sweeper_enabled {
        let sweeper_consumer = format!("de-sweep-{}", uuid::Uuid::new_v4());

        tokio::spawn(async move {
            let mut ticker = interval(sweep_interval);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
            ticker.tick().await; // interval 首个 tick 立即返回，跳过以保证先等一个周期

            loop {
                ticker.tick().await;
                match sweeper_queue
                    .claim_stale(
                        RAW_QUOTES_STREAM,
                        NORMALIZER_GROUP,
                        &sweeper_consumer,
                        min_idle_ms,
                        100,
                        max_delivery_count,
                    )
                    .await
                {
                    Ok(result) => {
                        for invalid in result.invalid {
                            quarantine_invalid(&sweeper_queue, &invalid).await;
                        }
                        for message in result.messages {
                            tracing::info!("Reprocessing orphaned pending message {}", message.id);
                            process_normalizer_message(&sweeper_state, &sweeper_queue, &message)
                                .await;
                        }
                    }
                    // Redis < 6.2 无 XPENDING IDLE/XCLAIM 认领：兜底不可用属预期降级，debug 级避免刷屏
                    Err(err) => tracing::debug!("claim_stale sweep skipped: {}", err),
                }
            }
        });
    }

    Ok(())
}

/// 持久化镜像失败的后台补写：周期从重试缓冲取批次重写 Timescale。未启用持久化
/// 时不启动（预期降级，info 留痕）。
pub(crate) fn start_mirror_retry_worker(state: Arc<AppState>) {
    let Some(persistence) = state.persistence.clone() else {
        tracing::info!("Mirror retry worker not started: persistence disabled");
        return;
    };
    let buffer = state.mirror_retries.clone();
    tokio::spawn(async move {
        let mut ticker = interval(StdDuration::from_secs(MIRROR_RETRY_INTERVAL_SECS));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        ticker.tick().await; // interval 首个 tick 立即返回，跳过以保证先等一个周期

        loop {
            ticker.tick().await;
            let rewrote = run_mirror_retry_round(&persistence, &buffer).await;
            if rewrote > 0 {
                tracing::info!(
                    "Mirror retry round rewrote {} buffered quotes to Timescale (backlog {})",
                    rewrote,
                    buffer.len()
                );
            }
        }
    });
}

/// 湖维护后台任务（data-lake-parquet §5/§7）：周期执行 compaction + manifest
/// 重建。lake 未启用或 `maintenance_interval_secs=0`（默认）时不启动——
/// 湖默认只作写透与读旁路，维护是显式 opt-in。
pub(crate) fn start_lake_maintenance_worker(state: Arc<AppState>) {
    let Some(lake) = state.lake.clone() else {
        return;
    };
    let secs = state.config.lake.maintenance_interval_secs;
    if secs == 0 {
        tracing::info!("Lake maintenance worker not started: interval disabled");
        return;
    }
    let layer = state.config.lake.layer.clone();
    let table = state.config.lake.table.clone();
    tokio::spawn(async move {
        let mut ticker = interval(StdDuration::from_secs(secs.max(1)));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        ticker.tick().await; // interval 首个 tick 立即返回，跳过以保证先等一个周期

        loop {
            ticker.tick().await;
            run_lake_maintenance_round(&lake, &layer, &table).await;
        }
    });
}

/// 单轮湖维护：compaction（小文件装箱合并，失败只告警）后重建 manifest
/// （统计面与 catalog v1 底座）。阻塞 fs IO 放 blocking 线程，不占 worker。
pub(crate) async fn run_lake_maintenance_round(lake: &Arc<LakeWriter>, layer: &str, table: &str) {
    let compact_lake = lake.clone();
    let (l, t) = (layer.to_string(), table.to_string());
    match tokio::task::spawn_blocking(move || {
        compact_lake.compact_table(&l, &t, &CompactionOptions::default())
    })
    .await
    {
        Ok(Ok(report)) if !report.removed.is_empty() => tracing::info!(
            merged = report.written.len(),
            removed = report.removed.len(),
            untouched = report.untouched_files,
            "lake compaction round done"
        ),
        Ok(Ok(_)) => {}
        Ok(Err(err)) => tracing::warn!("lake compaction failed: {}", err),
        Err(err) => tracing::warn!("lake compaction task failed: {}", err),
    }

    let manifest_lake = lake.clone();
    let (l, t) = (layer.to_string(), table.to_string());
    match tokio::task::spawn_blocking(move || manifest_lake.rebuild_manifest(&l, &t)).await {
        Ok(Ok(m)) if m.total_files > 0 => tracing::info!(
            partitions = m.partitions.len(),
            files = m.total_files,
            rows = m.total_rows,
            "lake manifest rebuilt"
        ),
        Ok(Ok(_)) => {}
        Ok(Err(err)) => tracing::warn!("lake manifest rebuild failed: {}", err),
        Err(err) => tracing::warn!("lake manifest task failed: {}", err),
    }
}

/// 单轮补写：按序重写缓冲批次；任一条失败则该条起（含未尝试）按原序回队首，
/// 返回本轮成功条数。
pub(crate) async fn run_mirror_retry_round(
    persistence: &Arc<TimescaleTimeSeriesStorage>,
    buffer: &Arc<MirrorRetryBuffer>,
) -> usize {
    let batch = buffer.drain_for_retry(MIRROR_RETRY_BATCH);
    if batch.is_empty() {
        return 0;
    }
    for (idx, market_data) in batch.iter().enumerate() {
        if let Err(err) = persistence.insert_market_data(market_data).await {
            buffer.requeue_front(batch[idx..].to_vec());
            tracing::warn!(
                "Mirror retry write failed (backlog {}, dropped {} since start): {}",
                buffer.len(),
                buffer.dropped_total(),
                err
            );
            return idx;
        }
    }
    batch.len()
}

/// 处理一条 quotes.raw 消息：规范化 → 去重判定 → 入内存时序 → 转发 normalized → ack。
///
/// 去重（normalized 层写侧）：按原始 envelope 的 payload_hash 判重，窗口内重复的
/// 重放（claim_stale 兜底重投递、上游重复发布）不重复写内存/转发，但仍须 ack（否则会
/// 滞留 PEL 被 sweeper 无限重放）。指纹在「内存写 + 转发」都成功后才记录——提前
/// 记录会让写失败的重试被自己的去重窗口吞掉。窗口边界：进程内 FIFO、容量
/// PAYLOAD_DEDUP_WINDOW 条、重启清零；跨重启的重复由 Timescale 层 (symbol,ts)
/// UPSERT 幂等兜底。判重是内容级的：同窗口内逐字段全同的新 tick（实践中采集端
/// payload 带时间戳，几乎不可能）会被当作重放跳过，属已接受的取舍。
///
/// 写入/转发失败时不 ack，消息留待周期 claim_stale 兜底重放；
/// payload 无法规范化（缺字段等）则直接 ack 跳过——这与 stream 条目解码失败的
/// DLQ 隔离路径（quarantine_invalid）是不同层面的两回事。
///
/// 注意：写路径不再全量重建 DataFusion MemTable（实测 ~25-35µs/点、随存量线性
/// 增长，见 TODO.md P2）——/query 每次执行前自行 refresh（execute_query），
/// /stocks、/indicators 直读内存时序，均不依赖写路径刷新。
pub(crate) async fn process_normalizer_message(
    state: &Arc<AppState>,
    queue: &RedisStreamQueue,
    message: &StreamMessage,
) {
    // 数据质量（architecture-review §5 P2）：sequence 断档检测与去重正交——
    // 重复消息同样推进基线（重放由 envelope.sequence 回退语义处理，不误报）；
    // v1 消息无 sequence 字段时静默跳过（加性演进，不炸老数据）。
    if let Some(sequence) = message.envelope.sequence {
        let source = message.envelope.source.as_str();
        match state
            .sequence_gaps
            .observe(&message.envelope.stream, source, sequence)
        {
            SequenceObservation::Gap(gap) => {
                metrics::counter!(
                    "alpha_dataquality_sequence_gaps_total",
                    "stream" => message.envelope.stream.clone(),
                    "source" => source.to_string(),
                )
                .increment(gap.missing);
                tracing::warn!(
                    stream = %message.envelope.stream,
                    source,
                    expected = gap.expected,
                    got = gap.got,
                    missing = gap.missing,
                    entry_id = %message.id,
                    "sequence gap detected (data quality)"
                );
            }
            SequenceObservation::Regression { last, got } => {
                metrics::counter!(
                    "alpha_dataquality_sequence_regressions_total",
                    "stream" => message.envelope.stream.clone(),
                    "source" => source.to_string(),
                )
                .increment(1);
                tracing::info!(
                    stream = %message.envelope.stream,
                    source,
                    last,
                    got,
                    "sequence regression (source restart or replay); baseline reset"
                );
            }
            SequenceObservation::FirstSeen | SequenceObservation::Continuous => {}
        }
    }

    match normalize_quote(&message.envelope) {
        Some(market_data) => {
            // 数据质量（P2「异常」维度）：单跳涨跌幅超阈判定——超 A 股涨跌停
            // 带（缺省 30%）即行情源污染信号；与去重窗口正交（重放同价只是
            // Within）。符号不进指标标签（symbol 面无界），进日志行。
            match state.price_outliers.observe(
                &market_data.symbol,
                market_data.price,
                Utc::now().timestamp_millis(),
            ) {
                OutlierObservation::Jump(jump) => {
                    metrics::counter!(
                        "alpha_dataquality_price_outliers_total",
                        "stream" => message.envelope.stream.clone(),
                        "source" => message.envelope.source.as_str().to_string(),
                    )
                    .increment(1);
                    tracing::warn!(
                        symbol = %market_data.symbol,
                        last = jump.last,
                        got = jump.got,
                        pct = jump.pct,
                        entry_id = %message.id,
                        "price outlier detected (data quality)"
                    );
                }
                OutlierObservation::FirstSeen | OutlierObservation::Within => {}
            }

            // 数据质量（P2「Source Divergence」维度）：同 symbol 同刻多源比对
            // （事件时间对齐，2 分钟窗口内才比较）；单源在报期间恒
            // FirstSeen/Agree，第二源并发接入即自然生效。
            match state.source_divergence.observe(
                &market_data.symbol,
                message.envelope.source.as_str(),
                market_data.price,
                market_data.timestamp.timestamp_millis(),
                Utc::now().timestamp_millis(),
            ) {
                DivergenceObservation::Diverge(div) => {
                    metrics::counter!(
                        "alpha_dataquality_source_divergences_total",
                        "stream" => message.envelope.stream.clone(),
                        "source" => message.envelope.source.as_str().to_string(),
                    )
                    .increment(1);
                    tracing::warn!(
                        symbol = %market_data.symbol,
                        source = %div.source,
                        other_source = %div.other_source,
                        price = div.price,
                        other_price = div.other_price,
                        pct = div.pct,
                        entry_id = %message.id,
                        "source divergence detected (data quality)"
                    );
                }
                DivergenceObservation::FirstSeen | DivergenceObservation::Agree => {}
            }

            let payload_hash = message.envelope.payload_hash.clone();
            if state.payload_hash_seen(&payload_hash) {
                // 数据质量（P2「重复」维度）：窗口内重放/重复发布计数——
                // 静默丢弃面必须有量纲，否则去重窗口异常只能靠猜
                metrics::counter!(
                    "alpha_dataquality_duplicates_total",
                    "stream" => message.envelope.stream.clone(),
                    "source" => message.envelope.source.as_str().to_string(),
                )
                .increment(1);
                tracing::debug!(
                    "Skipping duplicate payload {} (entry {})",
                    payload_hash,
                    message.id
                );
                if let Err(err) = queue
                    .ack(RAW_QUOTES_STREAM, NORMALIZER_GROUP, &message.id)
                    .await
                {
                    tracing::warn!("Failed to ack duplicate raw quote {}: {}", message.id, err);
                }
                return;
            }

            if let Err(err) = state.write_normalized(&market_data).await {
                tracing::warn!("Failed to write normalized quote to storage: {}", err);
                return; // 不 ack：留待 claim_stale 兜底重放；指纹未记录，重试不会被去重吞掉
            }

            let normalized = build_normalized_envelope(&message.envelope, &market_data, Utc::now());

            if let Err(err) = queue.publish(NORMALIZED_QUOTES_STREAM, &normalized).await {
                tracing::warn!("Failed to publish normalized quote: {}", err);
                return; // 不 ack：留待 claim_stale 兜底重放；指纹未记录，重试不会被去重吞掉
            }

            state.record_payload_hash(&payload_hash);

            if let Err(err) = queue
                .ack(RAW_QUOTES_STREAM, NORMALIZER_GROUP, &message.id)
                .await
            {
                tracing::warn!("Failed to ack raw quote {}: {}", message.id, err);
            }
        }
        None => {
            // 数据质量（P2「完整性」维度）：无法规范化的载荷（缺 symbol/price
            // 等必需字段）计数——直接 ack 跳过的静默面，掉了多少数据要可见
            metrics::counter!(
                "alpha_dataquality_invalid_payloads_total",
                "stream" => message.envelope.stream.clone(),
                "source" => message.envelope.source.as_str().to_string(),
            )
            .increment(1);
            tracing::warn!("Skipping invalid raw quote message {}", message.id);
            let _ = queue
                .ack(RAW_QUOTES_STREAM, NORMALIZER_GROUP, &message.id)
                .await;
        }
    }
}

/// 无法解码的条目：按 DLQ 契约隔离（quotes.dlq）并 ack，
/// 避免滞留消费组 PEL 永不清理；DLQ 发布失败则不 ack，留待下轮重试。
pub(crate) async fn quarantine_invalid(queue: &RedisStreamQueue, invalid: &InvalidMessage) {
    // 数据质量（P2「完整性」维度，stream 层）：解码失败的隔离计数
    metrics::counter!(
        "alpha_dataquality_quarantined_total",
        "stream" => invalid.stream.clone(),
    )
    .increment(1);
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
        .ack(RAW_QUOTES_STREAM, NORMALIZER_GROUP, &invalid.id)
        .await
    {
        tracing::warn!("Failed to ack quarantined message {}: {}", invalid.id, err);
    }
}

pub(crate) fn normalize_quote(envelope: &StreamEnvelope) -> Option<MarketData> {
    let payload = envelope.payload.as_object()?;
    let symbol = payload.get("symbol")?.as_str()?.to_string();
    let price = payload.get("price")?.as_f64()?;
    let volume = payload.get("volume")?.as_u64()?;

    if price <= 0.0 {
        return None;
    }

    Some(MarketData {
        symbol,
        // 时间模型（architecture-review §3.3）：行情时刻用事件时间（源侧），
        // v1 消息无 event_time 时退回 ingest_ts（历史行为）。
        timestamp: envelope.event_time.unwrap_or(envelope.ingest_ts),
        price,
        volume,
        bid: payload.get("bid1").and_then(|v| v.as_f64()),
        ask: payload.get("ask1").and_then(|v| v.as_f64()),
        open: payload.get("open").and_then(|v| v.as_f64()),
        high: payload.get("high").and_then(|v| v.as_f64()),
        low: payload.get("low").and_then(|v| v.as_f64()),
    })
}

/// 构建 normalized payload：保留上游原始字段（change/change_percent/pre_close/name/source 等，
/// 供 real-time-feed 与审计回溯使用），再以规范化后的 MarketData 字段覆盖同名字段。
pub(crate) fn normalized_payload(
    envelope: &StreamEnvelope,
    market_data: &MarketData,
) -> serde_json::Value {
    let mut payload = match envelope.payload.as_object() {
        Some(obj) => obj.clone(),
        None => serde_json::Map::new(),
    };

    if let Ok(normalized) = serde_json::to_value(market_data) {
        if let Some(fields) = normalized.as_object() {
            for (key, value) in fields {
                payload.insert(key.clone(), value.clone());
            }
        }
    }

    serde_json::Value::Object(payload)
}

/// normalized 转发信封（Envelope v2，architecture-review §3.1/§3.3）：
/// - `process_time` 打本跳完成时刻（时间三跳模型的处理时刻）
/// - 上游契约字段**透传**：event_time（源侧事件时刻）、source_event_id
///   （对账锚点）、market、sequence（断档检测）、trace_id（链路）——
///   换跳不丢溯源信息；ingest_ts/payload_hash 由 new() 对本跳重新赋值。
pub(crate) fn build_normalized_envelope(
    upstream: &StreamEnvelope,
    market_data: &MarketData,
    process_time: DateTime<Utc>,
) -> StreamEnvelope {
    let mut normalized = StreamEnvelope::new(
        NORMALIZED_QUOTES_STREAM,
        "normalized_quote",
        "data-engine",
        Some(market_data.symbol.clone()),
        normalized_payload(upstream, market_data),
    )
    .with_process_time(process_time);
    // 上游契约字段透传（None 透传为 None，v1 上游消息缺失时本跳保持缺省）
    normalized.event_time = upstream.event_time;
    normalized.source_event_id = upstream.source_event_id.clone();
    normalized.market = upstream.market.clone();
    normalized.sequence = upstream.sequence;
    normalized.trace_id = upstream.trace_id.clone();
    normalized
}

/// Prometheus 抓取端点：渲染 metrics recorder 文本快照
pub(crate) async fn metrics_endpoint(State(state): State<Arc<AppState>>) -> String {
    state.metrics.render()
}

/// 健康检查
pub(crate) async fn health_check(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let stats = state.storage.get_statistics().await.ok();

    Json(serde_json::json!({
        "status": "healthy",
        "service": "data-engine",
        "timestamp": Utc::now().to_rfc3339(),
        "version": env!("CARGO_PKG_VERSION"),
        "time_series": stats,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn quote_envelope(payload: serde_json::Value) -> StreamEnvelope {
        StreamEnvelope::new(RAW_QUOTES_STREAM, "quote", "eastmoney", None, payload)
    }

    fn full_payload() -> serde_json::Value {
        serde_json::json!({
            "symbol": "sh600000",
            "price": 12.5,
            "volume": 12345u64,
            "bid1": 12.49,
            "ask1": 12.51,
            "open": 12.1,
            "high": 12.8,
            "low": 12.0,
        })
    }

    #[test]
    fn normalize_quote_maps_required_and_optional_fields() {
        let event_time = Utc.with_ymd_and_hms(2026, 11, 11, 1, 2, 3).unwrap();
        let envelope = quote_envelope(full_payload()).with_event_time(event_time);

        let md = normalize_quote(&envelope).expect("完整载荷应可规范化");
        assert_eq!(md.symbol, "sh600000");
        assert_eq!(md.price, 12.5);
        assert_eq!(md.volume, 12345);
        assert_eq!(md.bid, Some(12.49));
        assert_eq!(md.ask, Some(12.51));
        assert_eq!(md.open, Some(12.1));
        assert_eq!(md.high, Some(12.8));
        assert_eq!(md.low, Some(12.0));
        assert_eq!(md.timestamp, event_time);
    }

    #[test]
    fn normalize_quote_falls_back_to_ingest_ts_without_event_time() {
        let envelope = quote_envelope(full_payload());
        let md = normalize_quote(&envelope).expect("无 event_time 应回退 ingest_ts");
        assert_eq!(md.timestamp, envelope.ingest_ts);
    }

    #[test]
    fn normalize_quote_rejects_non_object_payload() {
        assert!(normalize_quote(&quote_envelope(serde_json::json!("str"))).is_none());
        assert!(normalize_quote(&quote_envelope(serde_json::json!([1, 2]))).is_none());
    }

    #[test]
    fn normalize_quote_rejects_missing_or_mistyped_required_fields() {
        // 缺 symbol
        let missing_symbol = serde_json::json!({"price": 1.0, "volume": 1u64});
        assert!(normalize_quote(&quote_envelope(missing_symbol)).is_none());
        // symbol 非字符串
        let symbol_not_str = serde_json::json!({"symbol": 42, "price": 1.0, "volume": 1u64});
        assert!(normalize_quote(&quote_envelope(symbol_not_str)).is_none());
        // 缺 price
        let missing_price = serde_json::json!({"symbol": "sh600000", "volume": 1u64});
        assert!(normalize_quote(&quote_envelope(missing_price)).is_none());
        // price 非数值
        let price_not_f64 = serde_json::json!({"symbol": "sh600000", "price": "12.5", "volume": 1u64});
        assert!(normalize_quote(&quote_envelope(price_not_f64)).is_none());
        // 缺 volume
        let missing_volume = serde_json::json!({"symbol": "sh600000", "price": 1.0});
        assert!(normalize_quote(&quote_envelope(missing_volume)).is_none());
        // volume 非非负整数（负数/小数 as_u64 均失败）
        let volume_negative = serde_json::json!({"symbol": "sh600000", "price": 1.0, "volume": -5});
        assert!(normalize_quote(&quote_envelope(volume_negative)).is_none());
        let volume_fractional = serde_json::json!({"symbol": "sh600000", "price": 1.0, "volume": 1.5});
        assert!(normalize_quote(&quote_envelope(volume_fractional)).is_none());
    }

    #[test]
    fn normalize_quote_rejects_non_positive_price() {
        let zero = serde_json::json!({"symbol": "sh600000", "price": 0.0, "volume": 1u64});
        assert!(normalize_quote(&quote_envelope(zero)).is_none());
        let negative = serde_json::json!({"symbol": "sh600000", "price": -1.0, "volume": 1u64});
        assert!(normalize_quote(&quote_envelope(negative)).is_none());
    }

    #[test]
    fn normalized_payload_merges_normalized_fields_over_upstream() {
        // 上游附加字段 name/change_percent 应保留；同名污染字段 bid:99.0 应被
        // 规范化值（bid1 → bid）覆盖
        let envelope = quote_envelope(serde_json::json!({
            "symbol": "sh600000",
            "price": 12.5,
            "volume": 12345u64,
            "bid1": 12.49,
            "bid": 99.0,
            "name": "浦发银行",
            "change_percent": 1.2,
        }));
        let market_data = normalize_quote(&envelope).expect("上游载荷可规范化");
        let payload = normalized_payload(&envelope, &market_data);
        let obj = payload.as_object().expect("合并结果为对象");

        assert_eq!(obj.get("symbol").and_then(|v| v.as_str()), Some("sh600000"));
        assert_eq!(obj.get("name").and_then(|v| v.as_str()), Some("浦发银行"));
        assert_eq!(obj.get("change_percent").and_then(|v| v.as_f64()), Some(1.2));
        assert_eq!(obj.get("price").and_then(|v| v.as_f64()), Some(12.5));
        assert_eq!(obj.get("volume").and_then(|v| v.as_u64()), Some(12345));
        // 覆盖生效：bid 取规范化后的 bid1 值，不再是被污染的 99.0
        assert_eq!(obj.get("bid").and_then(|v| v.as_f64()), Some(12.49));
    }

    #[test]
    fn normalized_payload_handles_non_object_upstream() {
        let envelope = quote_envelope(serde_json::json!("not-an-object"));
        let market_data = MarketData {
            symbol: "sh600000".to_string(),
            timestamp: envelope.ingest_ts,
            price: 1.0,
            volume: 1,
            bid: None,
            ask: None,
            open: None,
            high: None,
            low: None,
        };
        let payload = normalized_payload(&envelope, &market_data);
        let obj = payload.as_object().expect("非对象上游退化为仅规范化字段");
        assert_eq!(obj.get("symbol").and_then(|v| v.as_str()), Some("sh600000"));
        assert_eq!(obj.get("price").and_then(|v| v.as_f64()), Some(1.0));
    }

    #[test]
    fn build_normalized_envelope_passes_upstream_contract_through() {
        let event_time = Utc.with_ymd_and_hms(2026, 11, 11, 0, 0, 0).unwrap();
        let process_time = Utc.with_ymd_and_hms(2026, 11, 11, 0, 0, 1).unwrap();
        let upstream = quote_envelope(full_payload())
            .with_event_time(event_time)
            .with_source_event_id("src-evt-1")
            .with_market("cn")
            .with_sequence(7)
            .with_trace_id("trace-abc");

        let market_data = normalize_quote(&upstream).expect("上游载荷可规范化");
        let normalized = build_normalized_envelope(&upstream, &market_data, process_time);

        assert_eq!(normalized.stream, NORMALIZED_QUOTES_STREAM);
        assert_eq!(normalized.event_type, "normalized_quote");
        assert_eq!(normalized.source, "data-engine");
        assert_eq!(normalized.event_time, Some(event_time));
        assert_eq!(normalized.source_event_id.as_deref(), Some("src-evt-1"));
        assert_eq!(normalized.market.as_deref(), Some("cn"));
        assert_eq!(normalized.sequence, Some(7));
        assert_eq!(normalized.trace_id.as_deref(), Some("trace-abc"));
        assert_eq!(normalized.process_time, Some(process_time));
        // 换跳重新打指纹与 ingest_ts
        assert_ne!(normalized.payload_hash, upstream.payload_hash);
        assert!(normalized.ingest_ts >= upstream.ingest_ts);
    }

    #[test]
    fn build_normalized_envelope_keeps_absent_contract_fields_none() {
        let upstream = quote_envelope(full_payload());
        let market_data = normalize_quote(&upstream).expect("上游载荷可规范化");
        let process_time = Utc.with_ymd_and_hms(2026, 11, 11, 0, 0, 2).unwrap();
        let normalized = build_normalized_envelope(&upstream, &market_data, process_time);

        assert_eq!(normalized.event_time, None);
        assert_eq!(normalized.source_event_id, None);
        assert_eq!(normalized.market, None);
        assert_eq!(normalized.sequence, None);
        assert_eq!(normalized.trace_id, None);
    }
}
