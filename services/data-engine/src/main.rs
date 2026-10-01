//! Alpha Finance Data Engine
//!
//! 基于 Axum + DataFusion 的高性能数据处理服务

use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Instant,
};

use alpha_core::{
    analytics::AnalysisEngine,
    errors::{AlphaError, AlphaResult},
    indicators::TechnicalIndicators,
    models::{AnalysisResult, MarketData},
};
use alpha_storage::{
    clickhouse::{ClickHouseConfig, ClickHouseStorage},
    InvalidMessage, RedisStreamQueue, StreamEnvelope, StreamMessage, TimeSeriesPoint,
    TimeSeriesStorage, TimescaleTimeSeriesStorage,
};
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Duration, Utc};
use datafusion::{
    arrow::{
        array::{
            ArrayRef, Float64Array, Int64Array, StringArray, TimestampMillisecondArray, UInt64Array,
        },
        datatypes::{DataType, TimeUnit},
        record_batch::RecordBatch,
    },
    datasource::MemTable,
    prelude::SessionContext,
};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use serde::{Deserialize, Serialize};
use std::time::Duration as StdDuration;
use tokio::{
    net::TcpListener,
    time::{interval, MissedTickBehavior},
};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

mod grpc;
mod settings;
use settings::{AppConfig, ClickHouseSettings, StorageConfig, TelemetryConfig};

const DEFAULT_REDIS_URL: &str = "redis://localhost:6379";
const RAW_QUOTES_STREAM: &str = "quotes.raw";
const NORMALIZED_QUOTES_STREAM: &str = "quotes.normalized";
const NORMALIZER_GROUP: &str = "data-engine-normalizer";
const QUOTES_DLQ_STREAM: &str = "quotes.dlq";

/// normalized 层写侧去重窗口容量（条）。按内容判重的 FIFO 窗口，
/// 容量取「远大于单进程任何瞬时重放量」的经验值。
const PAYLOAD_DEDUP_WINDOW: usize = 65_536;

/// 持久化镜像失败重试缓冲容量（条），超限丢最旧并计数（内存 serving 不受影响）。
const MIRROR_RETRY_BUFFER_CAP: usize = 100_000;
/// 持久化镜像失败补写：后台轮询间隔（秒）与单轮最大补写条数。
const MIRROR_RETRY_INTERVAL_SECS: u64 = 5;
const MIRROR_RETRY_BATCH: usize = 500;

/// 持久化镜像失败重试缓冲：进程内有界 FIFO。镜像写失败的条目入队，
/// 后台任务周期补写 Timescale；超容量丢最旧并累计计数（可观测）。
/// 边界（非完整 outbox，定位为 best-effort 的有限增强）：重启即失，
/// 跨重启缺口不由本机制弥补；补写与直写并发可能使极少数同 (symbol, ts)
/// 冲突回写旧值（Timescale UPSERT 最后写赢），内存 serving 不受影响。
struct MirrorRetryBuffer {
    queue: Mutex<VecDeque<MarketData>>,
    capacity: usize,
    dropped_total: std::sync::atomic::AtomicU64,
}

impl MirrorRetryBuffer {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            capacity,
            dropped_total: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// 镜像写失败的条目入队；满则丢最旧一条腾位。
    fn push(&self, market_data: MarketData) {
        let mut queue = self.queue.lock().expect("mirror retry mutex poisoned");
        if queue.len() >= self.capacity {
            queue.pop_front();
            self.dropped_total
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        queue.push_back(market_data);
    }

    /// 从队首取至多 `max` 条等待补写。
    fn drain_for_retry(&self, max: usize) -> Vec<MarketData> {
        let mut queue = self.queue.lock().expect("mirror retry mutex poisoned");
        (0..max).map_while(|_| queue.pop_front()).collect()
    }

    /// 补写失败：失败条 + 未尝试条按原序回队首（下一轮从断点继续）。
    fn requeue_front(&self, batch: Vec<MarketData>) {
        let mut queue = self.queue.lock().expect("mirror retry mutex poisoned");
        for market_data in batch.into_iter().rev() {
            queue.push_front(market_data);
        }
    }

    fn len(&self) -> usize {
        self.queue
            .lock()
            .expect("mirror retry mutex poisoned")
            .len()
    }

    fn dropped_total(&self) -> u64 {
        self.dropped_total
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// payload_hash 去重窗口：进程内 FIFO，超容量淘汰最早记录，重启清零。
#[derive(Default)]
struct RecentPayloads {
    seen: HashMap<String, ()>,
    order: VecDeque<String>,
    capacity: usize,
}

impl RecentPayloads {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            ..Default::default()
        }
    }

    fn contains(&self, hash: &str) -> bool {
        self.seen.contains_key(hash)
    }

    /// 记录 hash；窗口满时淘汰最早一条。
    fn insert(&mut self, hash: &str) {
        if self.seen.insert(hash.to_string(), ()).is_none() {
            self.order.push_back(hash.to_string());
        }
        while self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.seen.remove(&oldest);
            }
        }
    }
}

#[derive(Clone)]
struct AppState {
    session: SessionContext,
    storage: Arc<TimeSeriesStorage>,
    /// TimescaleDB 持久化镜像（storage.persistence_enabled 装配；None = 仅内存）
    persistence: Option<Arc<TimescaleTimeSeriesStorage>>,
    clickhouse: Option<Arc<ClickHouseStorage>>,
    /// normalized 层写侧去重窗口（payload_hash → 近期已成功处理的指纹）
    seen_payloads: Arc<Mutex<RecentPayloads>>,
    /// Timescale 镜像写失败的重试缓冲（有界，后台周期补写；未启用持久化时闲置）
    mirror_retries: Arc<MirrorRetryBuffer>,
    indicators: TechnicalIndicators,
    analysis: AnalysisEngine,
    config: Arc<AppConfig>,
    /// Prometheus 指标渲染句柄（/metrics）
    metrics: PrometheusHandle,
}

/// 进程级唯一 Prometheus 句柄（install_recorder 每进程一次——首个调用者
/// install 全局接管 metrics 宏，后续（并行测试）复用同一句柄渲染）
fn global_metrics_handle() -> &'static PrometheusHandle {
    static HANDLE: std::sync::OnceLock<PrometheusHandle> = std::sync::OnceLock::new();
    HANDLE.get_or_init(|| {
        PrometheusBuilder::new()
            .install_recorder()
            .unwrap_or_else(|_| PrometheusBuilder::new().build_recorder().handle())
    })
}

impl AppState {
    async fn new(config: Arc<AppConfig>) -> Self {
        let clickhouse = initialize_clickhouse(&config.clickhouse).await;
        let persistence = initialize_persistence(&config.storage).await;
        Self {
            session: SessionContext::new(),
            storage: Arc::new(TimeSeriesStorage::new()),
            persistence,
            seen_payloads: Arc::new(Mutex::new(RecentPayloads::with_capacity(
                PAYLOAD_DEDUP_WINDOW,
            ))),
            mirror_retries: Arc::new(MirrorRetryBuffer::with_capacity(MIRROR_RETRY_BUFFER_CAP)),
            clickhouse,
            indicators: TechnicalIndicators::new(),
            analysis: AnalysisEngine::new(),
            config,
            metrics: global_metrics_handle().clone(),
        }
    }

    /// 窗口内是否已成功处理过该 payload_hash（只读判定，不记录）
    fn payload_hash_seen(&self, hash: &str) -> bool {
        self.seen_payloads
            .lock()
            .expect("payload dedup mutex poisoned")
            .contains(hash)
    }

    /// 在「内存写 + normalized 转发」都成功后记录指纹；此前记录会导致
    /// 写失败重试被自己的去重窗口吞掉（消息永不落地）。
    fn record_payload_hash(&self, hash: &str) {
        self.seen_payloads
            .lock()
            .expect("payload dedup mutex poisoned")
            .insert(hash);
    }

    /// 写入规范化行情：内存时序为主（服务查询层），启用持久化时镜像写 Timescale。
    /// Timescale 写失败只告警不阻断管线（内存仍是 serving 层，不因落库失败回灌消费组）。
    async fn write_normalized(&self, market_data: &MarketData) -> AlphaResult<()> {
        self.storage.add_market_data(market_data).await?;
        if let Some(persistence) = self.persistence.as_ref() {
            if let Err(err) = persistence.insert_market_data(market_data).await {
                // 镜像失败不阻断管线（内存仍是 serving 层）；条目入有界缓冲由
                // 后台任务周期补写，短暂不可用期间不再直接丢持久化副本。
                self.mirror_retries.push(market_data.clone());
                tracing::warn!(
                    "Failed to persist normalized quote to Timescale (queued for retry, backlog {}): {}",
                    self.mirror_retries.len(),
                    err
                );
            }
        }
        Ok(())
    }

    async fn register_custom_functions(&self) -> anyhow::Result<()> {
        // 实际项目中在此注册自定义 UDF/UDAF。
        // 目前我们只记录日志以确保 DataFusion 会话可正常工作。
        tracing::info!("DataFusion session ready; custom UDF registration placeholder");
        Ok(())
    }

    async fn refresh_query_tables(&self) -> anyhow::Result<()> {
        let batch = build_market_data_record_batch(&self.storage).await?;
        let schema = batch.schema();
        let table = MemTable::try_new(schema, vec![vec![batch]])?;
        // datafusion 35 的 register_table 遇到同名表会报 "already exists"，
        // 必须先注销旧表（此前第二次 refresh 起就一直失败：写路径 debug 级吞掉、
        // /query 第二次起直接 500 的潜伏 bug）。
        if self.session.table_exist("market_data")? {
            self.session.deregister_table("market_data")?;
        }
        self.session
            .register_table("market_data", Arc::new(table))?;
        Ok(())
    }

    async fn seed_demo_data(&self) -> Result<(), AlphaError> {
        if !self.config.data.seed_demo_data {
            return Ok(());
        }

        if !self.storage.list_symbols().await?.is_empty() {
            return Ok(());
        }

        let symbols = &self.config.data.seed_symbols;
        let now = Utc::now() - Duration::days(60);

        for symbol in symbols {
            let mut series = Vec::new();
            let mut price = 120.0;

            for i in 0..120 {
                let offset = i as i64;
                let ts = now + Duration::hours(offset * 12);
                let drift = (i as f64).sin() * 2.5;
                price = (price + drift).max(1.0);

                let volume = 10_000 + (i as u64 * 50);
                let ohlc = (price - 0.8, price + 1.2, price - 1.5, price + 0.4);

                series.push(MarketData {
                    symbol: symbol.to_string(),
                    timestamp: ts,
                    price,
                    volume,
                    bid: Some(price - 0.1),
                    ask: Some(price + 0.1),
                    open: Some(ohlc.0),
                    high: Some(ohlc.1),
                    low: Some(ohlc.2),
                });
            }

            self.storage.add_market_data_batch(&series).await?;
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Arc::new(AppConfig::load()?);
    init_tracing(&config.telemetry)?;

    let state = Arc::new(AppState::new(config.clone()).await);

    state.register_custom_functions().await?;
    if let Err(err) = state.seed_demo_data().await {
        tracing::warn!("Failed to seed demo data: {}", err);
    }
    start_quote_normalizer(state.clone()).await?;
    start_mirror_retry_worker(state.clone());

    let router = build_router(state.clone());
    let addr = config.server.addr.clone();
    let grpc_addr: SocketAddr = config.server.grpc_addr.parse()?;

    let grpc_state = state.clone();
    tokio::spawn(async move {
        if let Err(err) = grpc::serve_grpc(grpc_addr, grpc_state).await {
            tracing::error!("gRPC server exited with error: {}", err);
        }
    });

    tracing::info!("Data Engine HTTP server listening on {}", addr);
    let listener = TcpListener::bind(&addr).await?;

    axum::serve(listener, router).await?;
    Ok(())
}

fn build_router(state: Arc<AppState>) -> Router {
    let enable_cors = state.config.server.enable_cors;

    let mut router = Router::new()
        .route("/health", get(health_check))
        .route("/metrics", get(metrics_endpoint))
        .route("/query", post(execute_query))
        .route("/clickhouse/exports", get(list_clickhouse_exports))
        .route("/clickhouse/export.parquet", get(get_clickhouse_export_parquet))
        // Back-compat (kept for existing links)
        .route("/clickhouse/market-data.parquet", get(get_clickhouse_market_data_parquet))
        .route("/stocks/:symbol/history", get(get_stock_history))
        .route("/stocks/:symbol/indicators", get(get_stock_indicators))
        .route("/indicators/calculate", post(calculate_indicators))
        .route("/analytics/performance", post(calculate_performance))
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    if enable_cors {
        router = router.layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );
    }

    router
}

async fn initialize_clickhouse(settings: &ClickHouseSettings) -> Option<Arc<ClickHouseStorage>> {
    if !settings.enabled {
        return None;
    }

    let cfg = ClickHouseConfig {
        url: settings.url.clone(),
        native_url: "tcp://localhost:9000".to_string(),
        database: settings.database.clone(),
        user: settings.user.clone(),
        password: settings.password.clone(),
    };

    match ClickHouseStorage::new(cfg).await {
        Ok(storage) => {
            tracing::info!("ClickHouse backend enabled");
            Some(Arc::new(storage))
        }
        Err(err) => {
            tracing::warn!("ClickHouse enabled but connection failed: {}", err);
            None
        }
    }
}

/// 按配置装配 Timescale 持久化（与 ClickHouse 装配同口径的三态降级）：
/// - 未启用（persistence_enabled=false）→ None，纯内存；
/// - 启用但 URL 缺失/无效、连接失败 → 告警后降级为 None（服务不因落库不可用而拒绝启动）；
/// - 正常 → Some，规范化写入经 write_normalized 镜像落库。
async fn initialize_persistence(
    settings: &StorageConfig,
) -> Option<Arc<TimescaleTimeSeriesStorage>> {
    if !settings.persistence_enabled {
        return None;
    }

    let Some(url) = settings.timescale_url.as_deref() else {
        tracing::warn!(
            "Persistence enabled but storage.timescale_url is not set; falling back to memory-only"
        );
        return None;
    };

    match TimescaleTimeSeriesStorage::connect(url).await {
        Ok(storage) => {
            tracing::info!("Timescale persistence enabled");
            Some(Arc::new(storage))
        }
        Err(err) => {
            tracing::warn!(
                "Persistence enabled but Timescale connect failed: {}; falling back to memory-only",
                err
            );
            None
        }
    }
}

async fn start_quote_normalizer(state: Arc<AppState>) -> anyhow::Result<()> {
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
fn start_mirror_retry_worker(state: Arc<AppState>) {
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

/// 单轮补写：按序重写缓冲批次；任一条失败则该条起（含未尝试）按原序回队首，
/// 返回本轮成功条数。
async fn run_mirror_retry_round(
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
async fn process_normalizer_message(
    state: &Arc<AppState>,
    queue: &RedisStreamQueue,
    message: &StreamMessage,
) {
    match normalize_quote(&message.envelope) {
        Some(market_data) => {
            let payload_hash = message.envelope.payload_hash.clone();
            if state.payload_hash_seen(&payload_hash) {
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

            let normalized = StreamEnvelope::new(
                NORMALIZED_QUOTES_STREAM,
                "normalized_quote",
                "data-engine",
                Some(market_data.symbol.clone()),
                normalized_payload(&message.envelope, &market_data),
            );

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
            tracing::warn!("Skipping invalid raw quote message {}", message.id);
            let _ = queue
                .ack(RAW_QUOTES_STREAM, NORMALIZER_GROUP, &message.id)
                .await;
        }
    }
}

/// 无法解码的条目：按 DLQ 契约隔离（quotes.dlq）并 ack，
/// 避免滞留消费组 PEL 永不清理；DLQ 发布失败则不 ack，留待下轮重试。
async fn quarantine_invalid(queue: &RedisStreamQueue, invalid: &InvalidMessage) {
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

fn normalize_quote(envelope: &StreamEnvelope) -> Option<MarketData> {
    let payload = envelope.payload.as_object()?;
    let symbol = payload.get("symbol")?.as_str()?.to_string();
    let price = payload.get("price")?.as_f64()?;
    let volume = payload.get("volume")?.as_u64()?;

    if price <= 0.0 {
        return None;
    }

    Some(MarketData {
        symbol,
        timestamp: envelope.ingest_ts,
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
fn normalized_payload(envelope: &StreamEnvelope, market_data: &MarketData) -> serde_json::Value {
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

/// Prometheus 抓取端点：渲染 metrics recorder 文本快照
async fn metrics_endpoint(State(state): State<Arc<AppState>>) -> String {
    state.metrics.render()
}

/// 健康检查
async fn health_check(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let stats = state.storage.get_statistics().await.ok();

    Json(serde_json::json!({
        "status": "healthy",
        "service": "data-engine",
        "timestamp": Utc::now().to_rfc3339(),
        "version": env!("CARGO_PKG_VERSION"),
        "time_series": stats,
    }))
}

#[derive(Debug, Deserialize)]
struct MarketDataParquetParams {
    symbol: String,
    start: Option<String>,
    end: Option<String>,
    days: Option<u32>,
    limit: Option<u64>,
}

#[derive(Debug, Serialize)]
struct ClickhouseExportDescriptor {
    id: &'static str,
    description: &'static str,
    params: &'static [&'static str],
}

const CLICKHOUSE_EXPORTS: &[ClickhouseExportDescriptor] = &[
    ClickhouseExportDescriptor {
        id: "market_data",
        description: "OHLCV market data (timestamp, symbol, open/high/low/close, volume)",
        params: &["symbol", "start|days", "end", "limit"],
    },
    ClickhouseExportDescriptor {
        id: "realtime_quotes",
        description: "Realtime quotes snapshot (symbol, last/bid/ask, volume, timestamp, change)",
        params: &["symbols(optional)", "limit"],
    },
];

async fn list_clickhouse_exports() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "success": true,
        "exports": CLICKHOUSE_EXPORTS,
    }))
}

#[derive(Debug, Deserialize)]
struct ClickhouseExportParams {
    query_id: String,
    symbol: Option<String>,
    symbols: Option<String>,
    start: Option<String>,
    end: Option<String>,
    days: Option<u32>,
    limit: Option<u64>,
}

/// 从 ClickHouse 导出预设数据集（Parquet）
///
/// 注意：此接口不接受任意 SQL，仅允许 query_id 白名单。
async fn get_clickhouse_export_parquet(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ClickhouseExportParams>,
) -> Result<axum::response::Response, ApiErrorResponse> {
    match params.query_id.as_str() {
        "market_data" => {
            let symbol = params
                .symbol
                .ok_or_else(|| ApiErrorResponse::bad_request("missing symbol".to_string()))?;
            let req = MarketDataParquetParams {
                symbol,
                start: params.start,
                end: params.end,
                days: params.days,
                limit: params.limit,
            };
            export_market_data_parquet(state, req).await
        }
        "realtime_quotes" => export_realtime_quotes_parquet(state, params).await,
        other => Err(ApiErrorResponse::bad_request(format!(
            "unknown query_id: {}",
            other
        ))),
    }
}

async fn export_market_data_parquet(
    state: Arc<AppState>,
    params: MarketDataParquetParams,
) -> Result<axum::response::Response, ApiErrorResponse> {
    let Some(clickhouse) = state.clickhouse.as_ref() else {
        return Err(ApiErrorResponse::bad_request(
            "ClickHouse backend is not enabled".to_string(),
        ));
    };

    // Simple guardrails to avoid accidental huge responses
    let limit = params.limit.unwrap_or(10_000).clamp(1, 200_000);

    let end = params
        .end
        .as_deref()
        .and_then(parse_datetime)
        .unwrap_or_else(Utc::now);
    let start = params
        .start
        .as_deref()
        .and_then(parse_datetime)
        .unwrap_or_else(|| {
            let days = params
                .days
                .unwrap_or(state.config.data.lookback_days)
                .clamp(1, 3650);
            end - Duration::days(days as i64)
        });

    let data = clickhouse
        .query_market_data_parquet(&params.symbol, start, end, Some(limit))
        .await
        .map_err(ApiErrorResponse::internal)?;

    let mut response = axum::response::Response::new(axum::body::Body::from(data));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-parquet"),
    );
    Ok(response)
}

/// 从 ClickHouse 导出市场数据（Parquet）
///
/// 供 DuckDB-WASM / DuckDB Desktop 直接 `read_parquet()` 使用。
async fn get_clickhouse_market_data_parquet(
    State(state): State<Arc<AppState>>,
    Query(params): Query<MarketDataParquetParams>,
) -> Result<axum::response::Response, ApiErrorResponse> {
    export_market_data_parquet(state, params).await
}

fn parse_datetime(input: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(input)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

async fn export_realtime_quotes_parquet(
    state: Arc<AppState>,
    params: ClickhouseExportParams,
) -> Result<axum::response::Response, ApiErrorResponse> {
    let Some(clickhouse) = state.clickhouse.as_ref() else {
        return Err(ApiErrorResponse::bad_request(
            "ClickHouse backend is not enabled".to_string(),
        ));
    };

    let limit = params.limit.unwrap_or(5000).clamp(1, 200_000);
    let symbols = params
        .symbols
        .as_deref()
        .map(|s| {
            s.split(',')
                .map(|part| part.trim().to_string())
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty());

    let data = clickhouse
        .query_realtime_quotes_parquet(symbols.as_deref(), Some(limit))
        .await
        .map_err(ApiErrorResponse::internal)?;

    let mut response = axum::response::Response::new(axum::body::Body::from(data));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-parquet"),
    );
    Ok(response)
}

/// 执行 SQL 查询
#[tracing::instrument(skip(state, request))]
async fn execute_query(
    State(state): State<Arc<AppState>>,
    Json(request): Json<QueryRequest>,
) -> Result<Json<QueryResponse>, ApiErrorResponse> {
    let start = Instant::now();
    state
        .refresh_query_tables()
        .await
        .map_err(|err| ApiErrorResponse::internal(err.to_string()))?;

    let dataframe = state
        .session
        .sql(&request.query)
        .await
        .map_err(|err| ApiErrorResponse::internal(err.to_string()))?;

    let results = dataframe
        .collect()
        .await
        .map_err(|err| ApiErrorResponse::internal(err.to_string()))?;

    let execution_time_ms = start.elapsed().as_millis() as u64;
    let rows: usize = results.iter().map(|batch| batch.num_rows()).sum();

    let data = record_batches_to_json(&results)
        .map_err(|err| ApiErrorResponse::internal(err.to_string()))?;

    Ok(Json(QueryResponse {
        success: true,
        row_count: rows,
        data,
        execution_time_ms,
    }))
}

/// 获取股票历史数据
#[tracing::instrument(skip(state))]
async fn get_stock_history(
    State(state): State<Arc<AppState>>,
    Path(symbol): Path<String>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<HistoryResponse>, ApiErrorResponse> {
    let default_days = state.config.data.lookback_days;
    let days = params.days.unwrap_or(default_days).max(1);
    let end_time = Utc::now();
    let start_time = end_time - Duration::days(days as i64);

    let mut points = state
        .storage
        .get_data_in_range(&symbol, start_time, end_time)
        .await
        .map_err(ApiErrorResponse::from)?;

    if let Some(limit) = params.limit {
        if points.len() > limit {
            points = points.split_off(points.len() - limit);
        }
    }

    let data = points
        .iter()
        .map(|point| {
            serde_json::json!({
                "timestamp": point.timestamp.to_rfc3339(),
                "price": point.value,
                "volume": point.volume,
                "metadata": point.metadata,
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(HistoryResponse {
        success: true,
        symbol,
        period_days: days,
        data_points: data.len(),
        data,
    }))
}

/// 获取股票指标快照
#[tracing::instrument(skip(state))]
async fn get_stock_indicators(
    State(state): State<Arc<AppState>>,
    Path(symbol): Path<String>,
    Query(params): Query<IndicatorParams>,
) -> Result<Json<IndicatorsResponse>, ApiErrorResponse> {
    let lookback = params
        .lookback_days
        .unwrap_or(state.config.data.lookback_days);
    let points = load_points(&state, &symbol, lookback).await?;

    if points.is_empty() {
        return Err(ApiErrorResponse::not_found(format!(
            "No data available for symbol {}",
            symbol
        )));
    }

    let prices: Vec<f64> = points.iter().map(|p| p.value).collect();
    let timestamps: Vec<_> = points.iter().map(|p| p.timestamp).collect();

    let rsi_period = params.rsi_period.unwrap_or(14) as usize;
    let sma_short = params.sma_short.unwrap_or(20) as usize;
    let sma_long = params.sma_long.unwrap_or(50) as usize;
    let macd_fast = params.macd_fast.unwrap_or(12) as usize;
    let macd_slow = params.macd_slow.unwrap_or(26) as usize;
    let macd_signal = params.macd_signal.unwrap_or(9) as usize;

    let rsi = state.indicators.calculate_rsi(&prices, rsi_period);
    let sma_short_values = state.indicators.calculate_sma(&prices, sma_short);
    let sma_long_values = state.indicators.calculate_sma(&prices, sma_long);
    let (macd_line, signal_line, histogram) =
        state
            .indicators
            .calculate_macd(&prices, macd_fast, macd_slow, macd_signal);
    let (upper, middle, lower) = state.indicators.calculate_bollinger_bands(&prices, 20, 2.0);

    Ok(Json(IndicatorsResponse {
        success: true,
        symbol,
        data: serde_json::json!({
            "timestamps": timestamps,
            "rsi": rsi,
            "sma_short": sma_short_values,
            "sma_long": sma_long_values,
            "macd": {
                "line": macd_line,
                "signal": signal_line,
                "histogram": histogram
            },
            "bollinger": {
                "upper": upper,
                "middle": middle,
                "lower": lower
            }
        }),
    }))
}

/// 深度计算指标 (基于核心分析引擎)
#[tracing::instrument(skip(state, request))]
async fn calculate_indicators(
    State(state): State<Arc<AppState>>,
    Json(request): Json<IndicatorCalculationRequest>,
) -> Result<Json<IndicatorCalculationResponse>, ApiErrorResponse> {
    let lookback = request
        .lookback_days
        .unwrap_or(state.config.data.lookback_days);
    let points = load_points(&state, &request.symbol, lookback).await?;

    if points.len() < 10 {
        return Err(ApiErrorResponse::bad_request(
            "Not enough points to calculate indicators",
        ));
    }

    let market_data = points_to_market_data(&request.symbol, &points);
    let indicators = state
        .analysis
        .analyze_symbol(&market_data, None)
        .await
        .map_err(|err| ApiErrorResponse::internal(err.to_string()))?;

    Ok(Json(IndicatorCalculationResponse {
        success: true,
        symbol: request.symbol,
        indicators: request.indicators.unwrap_or_default(),
        analysis: indicators,
    }))
}

/// 计算性能指标
#[tracing::instrument(skip(state, request))]
async fn calculate_performance(
    State(state): State<Arc<AppState>>,
    Json(request): Json<PerformanceRequest>,
) -> Result<Json<PerformanceResponse>, ApiErrorResponse> {
    let points = load_points(&state, &request.symbol, request.period_days).await?;

    if points.len() < 2 {
        return Err(ApiErrorResponse::bad_request(
            "Not enough data points to evaluate performance",
        ));
    }

    let performance = calculate_performance_metrics(&points);

    Ok(Json(PerformanceResponse {
        success: true,
        symbol: request.symbol,
        period_days: request.period_days,
        performance,
    }))
}

async fn load_points(
    state: &Arc<AppState>,
    symbol: &str,
    lookback_days: u32,
) -> Result<Vec<TimeSeriesPoint>, ApiErrorResponse> {
    fetch_points(state, symbol, lookback_days)
        .await
        .map_err(ApiErrorResponse::from)
}

fn points_to_market_data(symbol: &str, points: &[TimeSeriesPoint]) -> Vec<MarketData> {
    points
        .iter()
        .map(|point| {
            let metadata = point.metadata.as_ref().and_then(|meta| meta.as_object());
            let open = metadata.and_then(|m| m.get("open").and_then(|v| v.as_f64()));
            let high = metadata.and_then(|m| m.get("high").and_then(|v| v.as_f64()));
            let low = metadata.and_then(|m| m.get("low").and_then(|v| v.as_f64()));
            let bid = metadata.and_then(|m| m.get("bid").and_then(|v| v.as_f64()));
            let ask = metadata.and_then(|m| m.get("ask").and_then(|v| v.as_f64()));

            MarketData {
                symbol: symbol.to_string(),
                timestamp: point.timestamp,
                price: point.value,
                volume: point.volume.unwrap_or_default(),
                bid,
                ask,
                open,
                high,
                low,
            }
        })
        .collect()
}

/// 查询请求
#[derive(Debug, Deserialize)]
struct QueryRequest {
    query: String,
}

/// 查询响应
#[derive(Debug, Serialize)]
struct QueryResponse {
    success: bool,
    row_count: usize,
    data: serde_json::Value,
    execution_time_ms: u64,
}

#[derive(Debug, Deserialize)]
struct HistoryParams {
    days: Option<u32>,
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
struct HistoryResponse {
    success: bool,
    symbol: String,
    period_days: u32,
    data_points: usize,
    data: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct IndicatorParams {
    rsi_period: Option<u32>,
    sma_short: Option<u32>,
    sma_long: Option<u32>,
    macd_fast: Option<u32>,
    macd_slow: Option<u32>,
    macd_signal: Option<u32>,
    lookback_days: Option<u32>,
}

#[derive(Debug, Serialize)]
struct IndicatorsResponse {
    success: bool,
    symbol: String,
    data: serde_json::Value,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct IndicatorCalculationRequest {
    symbol: String,
    lookback_days: Option<u32>,
    indicators: Option<Vec<String>>,
    rsi_period: Option<u32>,
    macd_fast: Option<u32>,
    macd_slow: Option<u32>,
    macd_signal: Option<u32>,
}

#[derive(Debug, Serialize)]
struct IndicatorCalculationResponse {
    success: bool,
    symbol: String,
    indicators: Vec<String>,
    analysis: AnalysisResult,
}

#[derive(Debug, Deserialize)]
struct PerformanceRequest {
    symbol: String,
    period_days: u32,
}

#[derive(Debug, Serialize)]
struct PerformanceResponse {
    success: bool,
    symbol: String,
    period_days: u32,
    performance: PerformanceMetrics,
}

#[derive(Debug, Clone, Serialize)]
struct PerformanceMetrics {
    total_return: f64,
    annualized_return: f64,
    volatility: f64,
    max_drawdown: f64,
    sharpe_ratio: f64,
    win_rate: f64,
}

fn calculate_performance_metrics(points: &[TimeSeriesPoint]) -> PerformanceMetrics {
    if points.len() < 2 {
        return PerformanceMetrics {
            total_return: 0.0,
            annualized_return: 0.0,
            volatility: 0.0,
            max_drawdown: 0.0,
            sharpe_ratio: 0.0,
            win_rate: 0.0,
        };
    }

    let prices: Vec<f64> = points.iter().map(|p| p.value).collect();
    let start_price = prices.first().copied().unwrap_or(0.0);
    let end_price = prices.last().copied().unwrap_or(0.0);
    let total_return = if start_price > 0.0 {
        ((end_price - start_price) / start_price) * 100.0
    } else {
        0.0
    };

    let elapsed_days = (points.last().unwrap().timestamp - points.first().unwrap().timestamp)
        .num_seconds() as f64
        / 86_400.0;
    let elapsed_days = elapsed_days.max(1.0);

    let annualized_return = if start_price > 0.0 && end_price > 0.0 {
        ((end_price / start_price).powf(365.0 / elapsed_days) - 1.0) * 100.0
    } else {
        0.0
    };

    let returns: Vec<f64> = prices
        .windows(2)
        .map(|window| (window[1] - window[0]) / window[0])
        .collect();

    let volatility = if returns.len() > 1 {
        let mean = returns.iter().sum::<f64>() / returns.len() as f64;
        let variance =
            returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (returns.len() - 1) as f64;
        (variance.sqrt() * (252.0_f64).sqrt()) * 100.0
    } else {
        0.0
    };

    let mut peak_price = start_price;
    let mut max_drawdown = 0.0;

    for price in prices.iter().copied() {
        if price > peak_price {
            peak_price = price;
            continue;
        }

        if peak_price > 0.0 {
            let drawdown = (peak_price - price) / peak_price;
            if drawdown > max_drawdown {
                max_drawdown = drawdown;
            }
        }
    }

    let risk_free_rate = 0.02;
    let sharpe_ratio = if volatility > 0.0 {
        ((annualized_return / 100.0) - risk_free_rate) / (volatility / 100.0)
    } else {
        0.0
    };

    let win_rate = if returns.is_empty() {
        0.0
    } else {
        (returns.iter().filter(|r| **r > 0.0).count() as f64 / returns.len() as f64) * 100.0
    };

    PerformanceMetrics {
        total_return,
        annualized_return,
        volatility,
        max_drawdown,
        sharpe_ratio,
        win_rate,
    }
}

async fn fetch_points(
    state: &Arc<AppState>,
    symbol: &str,
    lookback_days: u32,
) -> Result<Vec<TimeSeriesPoint>, AlphaError> {
    let end_time = Utc::now();
    let start_time = end_time - Duration::days(lookback_days as i64);

    state
        .storage
        .get_data_in_range(symbol, start_time, end_time)
        .await
}

async fn build_market_data_record_batch(
    storage: &TimeSeriesStorage,
) -> anyhow::Result<RecordBatch> {
    let symbols = storage.list_symbols().await?;

    let mut timestamps = Vec::new();
    let mut out_symbols = Vec::new();
    let mut prices = Vec::new();
    let mut volumes = Vec::new();
    let mut bids = Vec::new();
    let mut asks = Vec::new();
    let mut opens = Vec::new();
    let mut highs = Vec::new();
    let mut lows = Vec::new();

    for symbol in symbols {
        if let Some(series) = storage.get_series(&symbol).await? {
            for point in series.data {
                let metadata = point.metadata.as_ref().and_then(|meta| meta.as_object());

                timestamps.push(point.timestamp.timestamp_millis());
                out_symbols.push(symbol.clone());
                prices.push(point.value);
                volumes.push(point.volume);
                bids.push(metadata.and_then(|m| m.get("bid").and_then(|v| v.as_f64())));
                asks.push(metadata.and_then(|m| m.get("ask").and_then(|v| v.as_f64())));
                opens.push(metadata.and_then(|m| m.get("open").and_then(|v| v.as_f64())));
                highs.push(metadata.and_then(|m| m.get("high").and_then(|v| v.as_f64())));
                lows.push(metadata.and_then(|m| m.get("low").and_then(|v| v.as_f64())));
            }
        }
    }

    let schema = Arc::new(datafusion::arrow::datatypes::Schema::new(vec![
        datafusion::arrow::datatypes::Field::new(
            "timestamp",
            DataType::Timestamp(TimeUnit::Millisecond, None),
            false,
        ),
        datafusion::arrow::datatypes::Field::new("symbol", DataType::Utf8, false),
        datafusion::arrow::datatypes::Field::new("price", DataType::Float64, false),
        datafusion::arrow::datatypes::Field::new("volume", DataType::UInt64, true),
        datafusion::arrow::datatypes::Field::new("bid", DataType::Float64, true),
        datafusion::arrow::datatypes::Field::new("ask", DataType::Float64, true),
        datafusion::arrow::datatypes::Field::new("open", DataType::Float64, true),
        datafusion::arrow::datatypes::Field::new("high", DataType::Float64, true),
        datafusion::arrow::datatypes::Field::new("low", DataType::Float64, true),
    ]));

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(TimestampMillisecondArray::from(timestamps)) as ArrayRef,
            Arc::new(StringArray::from(out_symbols)) as ArrayRef,
            Arc::new(Float64Array::from(prices)) as ArrayRef,
            Arc::new(UInt64Array::from(volumes)) as ArrayRef,
            Arc::new(Float64Array::from(bids)) as ArrayRef,
            Arc::new(Float64Array::from(asks)) as ArrayRef,
            Arc::new(Float64Array::from(opens)) as ArrayRef,
            Arc::new(Float64Array::from(highs)) as ArrayRef,
            Arc::new(Float64Array::from(lows)) as ArrayRef,
        ],
    )
    .map_err(Into::into)
}

fn record_batches_to_json(batches: &[RecordBatch]) -> anyhow::Result<serde_json::Value> {
    let mut rows = Vec::new();

    for batch in batches {
        for row_idx in 0..batch.num_rows() {
            let mut row = serde_json::Map::new();

            for (col_idx, field) in batch.schema().fields().iter().enumerate() {
                let column = batch.column(col_idx);
                let value = array_value_to_json(column, row_idx)?;
                row.insert(field.name().clone(), value);
            }

            rows.push(serde_json::Value::Object(row));
        }
    }

    Ok(serde_json::Value::Array(rows))
}

fn array_value_to_json(array: &ArrayRef, row_idx: usize) -> anyhow::Result<serde_json::Value> {
    if array.is_null(row_idx) {
        return Ok(serde_json::Value::Null);
    }

    match array.data_type() {
        DataType::Utf8 => {
            let array = array.as_any().downcast_ref::<StringArray>().unwrap();
            Ok(serde_json::Value::String(array.value(row_idx).to_string()))
        }
        DataType::Int64 => {
            let array = array.as_any().downcast_ref::<Int64Array>().unwrap();
            Ok(serde_json::Value::Number(array.value(row_idx).into()))
        }
        DataType::UInt64 => {
            let array = array.as_any().downcast_ref::<UInt64Array>().unwrap();
            Ok(serde_json::Value::Number(array.value(row_idx).into()))
        }
        DataType::Float64 => {
            let array = array.as_any().downcast_ref::<Float64Array>().unwrap();
            let value = array.value(row_idx);
            Ok(serde_json::Value::Number(
                serde_json::Number::from_f64(value).unwrap_or(serde_json::Number::from(0)),
            ))
        }
        DataType::Timestamp(TimeUnit::Millisecond, _) => {
            let array = array
                .as_any()
                .downcast_ref::<TimestampMillisecondArray>()
                .unwrap();
            let timestamp = array.value(row_idx);
            let datetime = DateTime::<Utc>::from_timestamp_millis(timestamp)
                .ok_or_else(|| anyhow::anyhow!("Invalid timestamp {}", timestamp))?;
            Ok(serde_json::Value::String(datetime.to_rfc3339()))
        }
        other => Ok(serde_json::Value::String(format!("{:?}", other))),
    }
}

fn init_tracing(config: &TelemetryConfig) -> anyhow::Result<()> {
    if config.json {
        tracing_subscriber::fmt()
            .with_max_level(config.level_filter())
            .with_target(false)
            .json()
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_max_level(config.level_filter())
            .with_target(false)
            .init();
    }
    Ok(())
}

/// 统一的 API 错误响应
#[derive(Debug)]
struct ApiErrorResponse {
    status: StatusCode,
    message: String,
}

impl ApiErrorResponse {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }
}

impl From<AlphaError> for ApiErrorResponse {
    fn from(error: AlphaError) -> Self {
        match error {
            AlphaError::InvalidInput(_) => ApiErrorResponse::bad_request(error.to_string()),
            AlphaError::DataNotFound(_) => ApiErrorResponse::not_found(error.to_string()),
            _ => ApiErrorResponse::internal(error.to_string()),
        }
    }
}

impl IntoResponse for ApiErrorResponse {
    fn into_response(self) -> axum::response::Response {
        let body = Json(serde_json::json!({
            "success": false,
            "error": self.message,
        }));
        (self.status, body).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::State;
    use std::sync::Arc;

    fn test_config() -> Arc<AppConfig> {
        Arc::new(AppConfig::default())
    }

    #[test]
    fn test_performance_metrics_basic() {
        let now = Utc::now();
        let mut points = Vec::new();

        for i in 0..10 {
            points.push(TimeSeriesPoint {
                timestamp: now + Duration::days(i as i64),
                value: 100.0 + i as f64,
                volume: Some(1000 + i as u64),
                metadata: None,
            });
        }

        let metrics = calculate_performance_metrics(&points);
        assert!(metrics.total_return > 0.0);
        assert!(metrics.annualized_return > 0.0);
        assert!(metrics.volatility >= 0.0);
    }

    #[tokio::test]
    async fn execute_query_reads_registered_market_data_table() {
        let state = Arc::new(AppState::new(test_config()).await);
        let now = Utc::now();

        state
            .storage
            .add_market_data_batch(&[
                MarketData {
                    symbol: "AAPL".to_string(),
                    timestamp: now,
                    price: 100.0,
                    volume: 10,
                    bid: Some(99.5),
                    ask: Some(100.5),
                    open: Some(98.0),
                    high: Some(101.0),
                    low: Some(97.5),
                },
                MarketData {
                    symbol: "AAPL".to_string(),
                    timestamp: now + Duration::minutes(1),
                    price: 102.0,
                    volume: 20,
                    bid: Some(101.5),
                    ask: Some(102.5),
                    open: Some(100.0),
                    high: Some(103.0),
                    low: Some(99.0),
                },
            ])
            .await
            .unwrap();

        let Json(response) = execute_query(
            State(state),
            Json(QueryRequest {
                query: "SELECT symbol, MAX(price) AS max_price, SUM(volume) AS total_volume FROM market_data GROUP BY symbol".to_string(),
            }),
        )
        .await
        .unwrap();

        assert!(response.success);
        assert_eq!(response.row_count, 1);
        assert_eq!(response.data[0]["symbol"], "AAPL");
        assert_eq!(response.data[0]["max_price"], 102.0);
        assert_eq!(response.data[0]["total_volume"], 30);
    }

    fn quote_envelope(payload: serde_json::Value) -> StreamEnvelope {
        StreamEnvelope::new("quotes.raw", "quote", "eastmoney", None, payload)
    }

    #[test]
    fn normalize_quote_maps_bid_ask_fields() {
        let envelope = quote_envelope(serde_json::json!({
            "symbol": "000001",
            "price": 12.34,
            "volume": 5000,
            "bid1": 12.30,
            "ask1": 12.40,
            "open": 12.10,
            "high": 12.50,
            "low": 12.00
        }));

        let market_data = normalize_quote(&envelope).unwrap();
        assert_eq!(market_data.symbol, "000001");
        assert_eq!(market_data.price, 12.34);
        assert_eq!(market_data.volume, 5000);
        assert_eq!(market_data.bid, Some(12.30));
        assert_eq!(market_data.ask, Some(12.40));
        assert_eq!(market_data.open, Some(12.10));
        assert_eq!(market_data.high, Some(12.50));
        assert_eq!(market_data.low, Some(12.00));
    }

    #[test]
    fn normalize_quote_rejects_invalid_payloads() {
        let missing_price = quote_envelope(serde_json::json!({ "symbol": "000001", "volume": 1 }));
        assert!(normalize_quote(&missing_price).is_none());

        let negative_price =
            quote_envelope(serde_json::json!({ "symbol": "000001", "price": -1.0, "volume": 1 }));
        assert!(normalize_quote(&negative_price).is_none());

        let not_an_object = StreamEnvelope::new(
            "quotes.raw",
            "quote",
            "eastmoney",
            None,
            serde_json::json!("scalar"),
        );
        assert!(normalize_quote(&not_an_object).is_none());
    }

    #[test]
    fn normalized_payload_preserves_upstream_and_overrides_normalized_fields() {
        let envelope = quote_envelope(serde_json::json!({
            "symbol": "000001",
            "name": "平安银行",
            "price": 12.0,
            "volume": 1000,
            "change": 0.12,
            "change_percent": 0.98,
            "pre_close": 12.22,
            "source": "eastmoney"
        }));

        // 规范化结果与上游原始值不同（例如标准化/修正后），应覆盖上游同名字段
        let market_data = MarketData {
            symbol: "000001".to_string(),
            timestamp: Utc::now(),
            price: 12.34,
            volume: 5000,
            bid: Some(12.30),
            ask: Some(12.40),
            open: Some(12.10),
            high: Some(12.50),
            low: Some(12.00),
        };

        let payload = normalized_payload(&envelope, &market_data);
        let obj = payload.as_object().unwrap();

        // 上游独有字段保留（real-time-feed 依赖 change/change_percent，审计依赖 name/pre_close/source）
        assert_eq!(obj["change"], 0.12);
        assert_eq!(obj["change_percent"], 0.98);
        assert_eq!(obj["pre_close"], 12.22);
        assert_eq!(obj["name"], "平安银行");
        assert_eq!(obj["source"], "eastmoney");
        // 规范化字段覆盖上游同名字段
        assert_eq!(obj["price"], 12.34);
        assert_eq!(obj["volume"], 5000);
        assert_eq!(obj["bid"], 12.30);
        assert_eq!(obj["ask"], 12.40);
        assert_eq!(
            obj["timestamp"],
            serde_json::to_value(market_data.timestamp).unwrap()
        );
    }

    #[test]
    fn normalized_payload_handles_non_object_upstream() {
        let envelope = StreamEnvelope::new(
            "quotes.raw",
            "quote",
            "eastmoney",
            None,
            serde_json::json!("garbage"),
        );
        let market_data = MarketData {
            symbol: "000001".to_string(),
            timestamp: Utc::now(),
            price: 12.34,
            volume: 1,
            bid: None,
            ask: None,
            open: None,
            high: None,
            low: None,
        };

        let payload = normalized_payload(&envelope, &market_data);
        assert_eq!(payload["symbol"], "000001");
        assert_eq!(payload["price"], 12.34);
    }

    fn storage_settings(enabled: bool, url: Option<&str>) -> StorageConfig {
        StorageConfig {
            persistence_enabled: enabled,
            timescale_url: url.map(|s| s.to_string()),
        }
    }

    /// 三态之一：未启用 → 不装配持久化，写入仅走内存 serving 层。
    fn mirror_md(price: f64) -> MarketData {
        MarketData {
            symbol: "MIRROR".to_string(),
            timestamp: Utc::now(),
            price,
            volume: 100,
            bid: None,
            ask: None,
            open: None,
            high: None,
            low: None,
        }
    }

    /// 镜像重试缓冲：补写失败回队首后保持原序（断点续写语义）。
    #[test]
    fn mirror_retry_buffer_requeues_in_order_to_front() {
        let buffer = MirrorRetryBuffer::with_capacity(8);
        for price in [10.0, 11.0, 12.0] {
            buffer.push(mirror_md(price));
        }
        let first_batch = buffer.drain_for_retry(2);
        assert_eq!(2, first_batch.len());
        assert_eq!(10.0, first_batch[0].price);
        assert_eq!(11.0, first_batch[1].price);
        assert_eq!(1, buffer.len(), "undrained entries stay in queue");

        // 模拟补写失败：整批按原序回队首，且排在未消费条目之前
        buffer.requeue_front(first_batch);
        let all = buffer.drain_for_retry(10);
        assert_eq!(3, all.len());
        assert_eq!(
            vec![10.0, 11.0, 12.0],
            all.iter().map(|d| d.price).collect::<Vec<_>>()
        );
    }

    /// 镜像重试缓冲：超容量丢最旧并累计 dropped 计数（内存 serving 不受影响）。
    #[test]
    fn mirror_retry_buffer_drops_oldest_and_counts_on_overflow() {
        let buffer = MirrorRetryBuffer::with_capacity(2);
        for price in [10.0, 11.0, 12.0] {
            buffer.push(mirror_md(price));
        }
        assert_eq!(2, buffer.len());
        assert_eq!(1, buffer.dropped_total());
        let remaining = buffer.drain_for_retry(10);
        assert_eq!(
            vec![11.0, 12.0],
            remaining.iter().map(|d| d.price).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn persistence_disabled_writes_memory_only() {
        let disabled = storage_settings(false, Some("postgres://would-be-ignored"));
        assert!(initialize_persistence(&disabled).await.is_none());

        let state = Arc::new(AppState::new(test_config()).await);
        assert!(
            state.persistence.is_none(),
            "default config must not assemble persistence"
        );

        let market_data = MarketData {
            symbol: "E2E-DISABLED".to_string(),
            timestamp: Utc::now(),
            price: 10.0,
            volume: 1,
            bid: None,
            ask: None,
            open: None,
            high: None,
            low: None,
        };
        state.write_normalized(&market_data).await.unwrap();
        assert!(state
            .storage
            .list_symbols()
            .await
            .unwrap()
            .contains(&"E2E-DISABLED".to_string()));
    }

    /// 三态之二：启用但 URL 缺失/无效 → 降级为内存模式，服务不拒绝启动。
    #[tokio::test]
    async fn persistence_enabled_with_bad_url_degrades_to_memory() {
        let missing = storage_settings(true, None);
        assert!(initialize_persistence(&missing).await.is_none());

        let invalid = storage_settings(true, Some("postgres://invalid"));
        // 直接连接必须报错（与 timescale 模块的 connect_returns_error_without_db 同口径）
        assert!(TimescaleTimeSeriesStorage::connect("postgres://invalid")
            .await
            .is_err());
        // 装配层吞掉错误降级为 None，而不是让服务崩溃
        assert!(initialize_persistence(&invalid).await.is_none());

        // 降级后写入路径照常可用
        let config = AppConfig {
            storage: invalid,
            ..AppConfig::default()
        };
        let state = Arc::new(AppState::new(Arc::new(config)).await);
        assert!(state.persistence.is_none());
    }

    #[test]
    fn dedup_window_tracks_and_evicts_oldest() {
        let mut window = RecentPayloads::with_capacity(2);

        assert!(!window.contains("a"));
        window.insert("a");
        window.insert("b");
        assert!(window.contains("a"));
        assert!(window.contains("b"));

        // 未淘汰前：重复判定生效
        assert!(window.contains("a"));

        // 窗口满：插入 c 淘汰最早的 a
        window.insert("c");
        assert!(
            !window.contains("a"),
            "oldest entry must be evicted at capacity"
        );
        assert!(window.contains("b"));
        assert!(window.contains("c"));
    }

    /// 写路径不再全量重建 MemTable 后，查询仍必须看到最新数据，
    /// 且连续两次 /query 可用（此前 register_table 撞名导致第二次 refresh 失败）。
    #[tokio::test]
    async fn consecutive_queries_after_write_return_fresh_data() {
        let state = Arc::new(AppState::new(test_config()).await);
        let now = Utc::now();

        // 模拟 normalize 写路径：只写内存，不调用 refresh_query_tables
        state
            .write_normalized(&MarketData {
                symbol: "DEDUP1".to_string(),
                timestamp: now,
                price: 55.5,
                volume: 10,
                bid: None,
                ask: None,
                open: None,
                high: None,
                low: None,
            })
            .await
            .unwrap();

        for expected_price in [55.5, 56.5] {
            if expected_price == 56.5 {
                state
                    .write_normalized(&MarketData {
                        symbol: "DEDUP1".to_string(),
                        timestamp: now + Duration::minutes(1),
                        price: 56.5,
                        volume: 10,
                        bid: None,
                        ask: None,
                        open: None,
                        high: None,
                        low: None,
                    })
                    .await
                    .unwrap();
            }

            let Json(response) = execute_query(
                State(state.clone()),
                Json(QueryRequest {
                    query:
                        "SELECT MAX(price) AS max_price FROM market_data WHERE symbol = 'DEDUP1'"
                            .to_string(),
                }),
            )
            .await
            .unwrap();
            assert!(
                response.success,
                "query must succeed (register_table collision fixed)"
            );
            assert_eq!(
                response.data[0]["max_price"],
                serde_json::json!(expected_price)
            );
        }
    }

    /// 三态之三：正常连接 → write_normalized 镜像落库（需真实 TimescaleDB，无库自动跳过）。
    #[tokio::test]
    async fn write_through_persists_to_timescale() -> AlphaResult<()> {
        let Some(url) = std::env::var("TIMESCALE_TEST_URL").ok() else {
            eprintln!("skipping: TIMESCALE_TEST_URL not set");
            return Ok(());
        };

        let persistence = Arc::new(TimescaleTimeSeriesStorage::connect(&url).await?);
        let state = Arc::new(AppState {
            session: SessionContext::new(),
            storage: Arc::new(TimeSeriesStorage::new()),
            persistence: Some(persistence.clone()),
            clickhouse: None,
            seen_payloads: Arc::new(Mutex::new(RecentPayloads::with_capacity(
                PAYLOAD_DEDUP_WINDOW,
            ))),
            mirror_retries: Arc::new(MirrorRetryBuffer::with_capacity(MIRROR_RETRY_BUFFER_CAP)),
            indicators: TechnicalIndicators::new(),
            analysis: AnalysisEngine::new(),
            config: test_config(),
            metrics: global_metrics_handle().clone(),
        });

        let symbol = format!("E2E-{}", uuid::Uuid::new_v4());
        let now = Utc::now();
        for (offset, price) in [(0_i64, 42.5_f64), (1, 43.0)] {
            state
                .write_normalized(&MarketData {
                    symbol: symbol.clone(),
                    timestamp: now + Duration::minutes(offset),
                    price,
                    volume: 100,
                    bid: Some(price - 0.2),
                    ask: Some(price + 0.2),
                    open: None,
                    high: None,
                    low: None,
                })
                .await?;
        }

        // 内存 serving 层可见
        assert!(state.storage.list_symbols().await?.contains(&symbol));
        // Timescale 镜像同样落库
        assert_eq!(persistence.count(&symbol).await?, 2);
        let latest = persistence.latest(&symbol).await?.expect("persisted row");
        assert_eq!(latest.price, 43.0);

        Ok(())
    }
}
