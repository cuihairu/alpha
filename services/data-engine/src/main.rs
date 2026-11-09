//! Alpha Finance Data Engine
//!
//! 基于 Axum + DataFusion 的高性能数据处理服务

use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    sync::{Arc, Mutex, RwLock},
};

use alpha_protocols::instrument::{
    instrument_id, parse_instrument_id, Exchange, Instrument, InstrumentType,
};

use alpha_core::{
    analytics::AnalysisEngine,
    errors::{AlphaError, AlphaResult},
    indicators::TechnicalIndicators,
    models::MarketData,
};
use alpha_storage::{
    clickhouse::ClickHouseStorage, lake::LakeWriter, TimeSeriesStorage, TimescaleTimeSeriesStorage,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
    routing::{get, post},
    Json, Router,
};
use chrono::{Duration, Utc};
use datafusion::{
    arrow::datatypes::DataType,
    datasource::{
        file_format::parquet::ParquetFormat,
        listing::{ListingOptions, ListingTable, ListingTableConfig, ListingTableUrl},
        MemTable,
    },
    prelude::{SessionConfig, SessionContext},
};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use serde::Deserialize;
use tokio::net::TcpListener;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

mod bootstrap;
mod exports;
mod grpc;
mod market_routes;
mod normalizer;
mod outlier;
mod query_udfs;
mod sequence_gap;
mod settings;
mod source_divergence;
use bootstrap::{initialize_clickhouse, initialize_lake, initialize_persistence};
use exports::{
    get_clickhouse_export_parquet, get_clickhouse_market_data_parquet, list_clickhouse_exports,
};
use market_routes::{
    build_market_data_record_batch, calculate_indicators, calculate_performance, execute_query,
    get_stock_history, get_stock_history_csv, get_stock_indicators, init_tracing, ApiErrorResponse,
};
use normalizer::{
    health_check, metrics_endpoint, start_lake_maintenance_worker, start_mirror_retry_worker,
    start_quote_normalizer,
};
use outlier::PriceOutlierMonitor;
use sequence_gap::SequenceGapMonitor;
use settings::AppConfig;
use source_divergence::SourceDivergenceMonitor;

const DEFAULT_REDIS_URL: &str = "redis://localhost:6379";
const RAW_QUOTES_STREAM: &str = "quotes.raw";
const NORMALIZED_QUOTES_STREAM: &str = "quotes.normalized";
const NORMALIZER_GROUP: &str = "data-engine-normalizer";
const QUOTES_DLQ_STREAM: &str = "quotes.dlq";

/// 湖历史层在 /query SQL 里的表名（data-lake-parquet §6；与热层 MemTable
/// `market_data` 并存，SQL 可 UNION 两层。设计稿的 `lake.market_data` 点分名
/// 在 DataFusion 里要建同名 schema，骨架期用平名更直白）。
const LAKE_TABLE: &str = "lake_market_data";

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
    /// 证券主数据注册表（instrument_id → Instrument；见 architecture-review §3.2）。
    /// 当前为启动时种子装载，后续可接权威源的增量刷新。
    instruments: Arc<RwLock<HashMap<String, Instrument>>>,
    /// sequence 断档检测（数据质量 §3.1/§5 P2）：per-(stream, source) 基线
    sequence_gaps: Arc<SequenceGapMonitor>,
    /// 单跳价格异常检测（§5 P2「异常」维度）：per-symbol 价格基线，
    /// 超涨跌停带的跳变 = 行情源污染信号
    price_outliers: Arc<PriceOutlierMonitor>,
    /// 多源背离检测（§5 P2「Source Divergence」维度）：同 symbol 同刻
    /// 各源报价差超容差即背离；单源在报期间休眠
    source_divergence: Arc<SourceDivergenceMonitor>,
    /// Parquet 湖写透（docs/data-lake-parquet.md；lake.enabled 装配；None = 纯即时导出）
    lake: Option<Arc<LakeWriter>>,
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

/// 会话构造：lake 启用时关掉 `listing_table_ignore_subdirectory`——湖文件在
/// `trade_date=` 子目录下，DF 默认忽略子目录会让 ListingTable 列不到文件
/// （表只剩分区列）。仅影响 ListingTable 列目录面，热层 MemTable 不经此路径。
fn build_session_context(config: &AppConfig) -> SessionContext {
    if config.lake.enabled {
        let cfg = SessionConfig::new().set_bool(
            "datafusion.execution.listing_table_ignore_subdirectory",
            false,
        );
        return SessionContext::new_with_config(cfg);
    }
    SessionContext::new()
}

impl AppState {
    async fn new(config: Arc<AppConfig>) -> Self {
        let clickhouse = initialize_clickhouse(&config.clickhouse).await;
        let persistence = initialize_persistence(&config.storage).await;
        let lake = initialize_lake(&config);
        let session = build_session_context(&config);
        Self {
            session,
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
            instruments: Arc::new(RwLock::new(seed_instruments())),
            sequence_gaps: Arc::new(SequenceGapMonitor::new()),
            price_outliers: Arc::new(PriceOutlierMonitor::from_env()),
            source_divergence: Arc::new(SourceDivergenceMonitor::from_env()),
            lake,
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
        // /query 自定义聚合面：vwap/range_pct/avg_spread（query_udfs.rs），
        // 注册后 SQL 面（含网关反代）即可直接调用。
        query_udfs::register(&self.session);
        tracing::info!(
            functions = "vwap,range_pct,avg_spread",
            "DataFusion custom aggregate functions registered"
        );
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

        // 湖历史层（data-lake-parquet §6 接缝）：lake 启用时注册 ListingTable，
        // trade_date hive 目录即分区列（裁剪靠路径，零配置）。目录不存在（尚无
        // 落湖）静默跳过——/query 每次都重走本函数，首次写透落湖后下一查即可用；
        // 新分区文件由 ListingTable 查询期动态列目录，注册一次即可。
        if let Some(lake) = self.lake.as_ref() {
            self.register_lake_listing_table(lake).await;
        }
        Ok(())
    }

    /// 把 `{lake_root}/{layer}/{table}` 注册为 DataFusion ListingTable
    /// （lake_market_data）。失败只告警不阻断 /query（热层 MemTable 不受影响）。
    async fn register_lake_listing_table(&self, lake: &LakeWriter) {
        if self.session.table_exist(LAKE_TABLE).unwrap_or(false) {
            return;
        }
        let dir = lake
            .lake_root()
            .join(&self.config.lake.layer)
            .join(&self.config.lake.table);
        // 尚无任何落湖分区（表目录未建）不算错误，留给后续 refresh 再试。
        let Ok(abs) = std::fs::canonicalize(&dir) else {
            return;
        };
        let Ok(table_url) = ListingTableUrl::parse(abs.to_string_lossy()) else {
            tracing::warn!("lake listing table path invalid: {}", abs.display());
            return;
        };
        let options = ListingOptions::new(Arc::new(ParquetFormat::default()))
            .with_file_extension(".parquet")
            .with_table_partition_cols(vec![("trade_date".to_string(), DataType::Utf8)]);
        let config = match ListingTableConfig::new(table_url)
            .with_listing_options(options)
            .infer_schema(&self.session.state())
            .await
        {
            Ok(config) => config,
            Err(err) => {
                tracing::warn!("lake listing table schema infer failed: {}", err);
                return;
            }
        };
        match ListingTable::try_new(config) {
            Ok(table) => match self.session.register_table(LAKE_TABLE, Arc::new(table)) {
                Ok(_) => tracing::info!(
                    table = LAKE_TABLE,
                    path = %abs.display(),
                    "lake history layer registered for /query"
                ),
                Err(err) => tracing::warn!("lake listing table register failed: {}", err),
            },
            Err(err) => tracing::warn!("lake listing table build failed: {}", err),
        }
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
    start_lake_maintenance_worker(state.clone());

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

/// 证券主数据种子注册表（architecture-review §3.2）。
///
/// 内置 A 股代表标的：双交易所、覆盖 equity/index/etf 三类，并刻意收录
/// 000001 消歧示范对（cn.sse.000001 = 上证指数 / cn.szse.000001 = 平安银行）。
/// listed_at 为真实上市日期，作为契约样例。
fn seed_instruments() -> HashMap<String, Instrument> {
    use chrono::NaiveDate;

    let seeds = [
        (
            Exchange::Sse,
            "600519",
            "贵州茅台",
            InstrumentType::Equity,
            Some((2001, 8, 27)),
        ),
        (
            Exchange::Sse,
            "600036",
            "招商银行",
            InstrumentType::Equity,
            Some((2002, 4, 9)),
        ),
        (
            Exchange::Szse,
            "000001",
            "平安银行",
            InstrumentType::Equity,
            Some((1991, 4, 3)),
        ),
        (
            Exchange::Szse,
            "300750",
            "宁德时代",
            InstrumentType::Equity,
            Some((2018, 6, 11)),
        ),
        (
            Exchange::Sse,
            "000001",
            "上证指数",
            InstrumentType::Index,
            Some((1990, 12, 19)),
        ),
        (
            Exchange::Sse,
            "000300",
            "沪深300",
            InstrumentType::Index,
            Some((2005, 4, 8)),
        ),
        (
            Exchange::Szse,
            "399001",
            "深证成指",
            InstrumentType::Index,
            Some((1995, 1, 23)),
        ),
        (
            Exchange::Sse,
            "510300",
            "沪深300ETF",
            InstrumentType::Etf,
            Some((2012, 5, 28)),
        ),
        (
            Exchange::Szse,
            "159915",
            "创业板ETF",
            InstrumentType::Etf,
            Some((2011, 9, 20)),
        ),
    ];

    seeds
        .into_iter()
        .map(|(exchange, symbol, name, instrument_type, listed)| {
            let mut instrument = Instrument::new(exchange, symbol, name, instrument_type);
            instrument.listed_at = listed.map(|(y, m, d)| {
                NaiveDate::from_ymd_opt(y, m, d).expect("seed listed date must be valid")
            });
            (instrument.instrument_id.clone(), instrument)
        })
        .collect()
}

fn parse_instrument_type(raw: &str) -> Result<InstrumentType, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "equity" => Ok(InstrumentType::Equity),
        "index" => Ok(InstrumentType::Index),
        "fund" => Ok(InstrumentType::Fund),
        "bond" => Ok(InstrumentType::Bond),
        "etf" => Ok(InstrumentType::Etf),
        "other" => Ok(InstrumentType::Other),
        other => Err(format!(
            "未知证券类型「{other}」（可选 equity/index/fund/bond/etf/other）"
        )),
    }
}

/// GET /instruments 查询参数（均可选；exchange/type 传了就必须合法）
#[derive(Debug, Deserialize)]
struct InstrumentQueryParams {
    /// 按交易所过滤（大小写不敏感；SSE/SH 或 SZSE/SZ）
    exchange: Option<String>,
    /// 按证券类型过滤（equity/index/fund/bond/etf/other）
    #[serde(rename = "type")]
    instrument_type: Option<String>,
    /// 按符号子串过滤（大小写不敏感）
    symbol: Option<String>,
    /// 按名称子串过滤（大小写不敏感）
    q: Option<String>,
}

/// GET /instruments：列出注册表（可选组合过滤）
#[tracing::instrument(skip(state))]
async fn list_instruments(
    State(state): State<Arc<AppState>>,
    Query(params): Query<InstrumentQueryParams>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    let exchange = match params.exchange {
        None => None,
        Some(raw) => Some(Exchange::parse_token(&raw).map_err(ApiErrorResponse::bad_request)?),
    };
    let instrument_type = match params.instrument_type {
        None => None,
        Some(raw) => Some(parse_instrument_type(&raw).map_err(ApiErrorResponse::bad_request)?),
    };
    let symbol_lc = params.symbol.as_deref().map(str::to_lowercase);
    let name_lc = params.q.as_deref().map(str::to_lowercase);

    let registry = state
        .instruments
        .read()
        .expect("instrument registry poisoned");
    let instruments: Vec<&Instrument> = registry
        .values()
        .filter(|inst| exchange.map_or(true, |e| inst.exchange == e))
        .filter(|inst| instrument_type.map_or(true, |t| inst.instrument_type == t))
        .filter(|inst| {
            symbol_lc
                .as_deref()
                .map_or(true, |q| inst.symbol.to_lowercase().contains(q))
        })
        .filter(|inst| {
            name_lc.as_deref().map_or(true, |q| {
                inst.name.to_lowercase().contains(q) || inst.symbol.to_lowercase().contains(q)
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "success": true,
        "total": instruments.len(),
        "instruments": instruments,
    })))
}

/// GET /instruments/:id：按 instrument_id 精确查询（容忍大小写/空白，
/// 经 parse_instrument_id 规范后再查键）
#[tracing::instrument(skip(state))]
async fn get_instrument(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiErrorResponse> {
    let (market, exchange, symbol) =
        parse_instrument_id(&id).map_err(ApiErrorResponse::bad_request)?;
    let canonical = instrument_id(market, exchange, &symbol);
    let registry = state
        .instruments
        .read()
        .expect("instrument registry poisoned");
    match registry.get(&canonical) {
        Some(instrument) => Ok(Json(serde_json::json!({
            "success": true,
            "instrument": instrument,
        }))),
        None => Err(ApiErrorResponse::not_found(format!(
            "instrument {id} 未登记"
        ))),
    }
}

fn build_router(state: Arc<AppState>) -> Router {
    let enable_cors = state.config.server.enable_cors;

    // 行情数据面（L504 第三方集成面）：security.api_keys 非空时统一过
    // API key 门；/health 与 /metrics 运维面豁免（存活探测与抓取器不带
    // 业务凭据）。空表 = 关闭，行为与历史版本一致。
    let protected = Router::new()
        .route("/query", post(execute_query))
        .route("/clickhouse/exports", get(list_clickhouse_exports))
        .route("/clickhouse/export.parquet", get(get_clickhouse_export_parquet))
        // Back-compat (kept for existing links)
        .route("/clickhouse/market-data.parquet", get(get_clickhouse_market_data_parquet))
        .route("/stocks/:symbol/history", get(get_stock_history))
        .route("/stocks/:symbol/history.csv", get(get_stock_history_csv))
        .route("/stocks/:symbol/indicators", get(get_stock_indicators))
        .route("/instruments", get(list_instruments))
        .route("/instruments/:id", get(get_instrument))
        .route("/indicators/calculate", post(calculate_indicators))
        .route("/analytics/performance", post(calculate_performance))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_api_key,
        ));

    let mut router = Router::new()
        .route("/health", get(health_check))
        .route("/metrics", get(metrics_endpoint))
        .merge(protected)
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

/// 第三方集成鉴权门（L504）：`security.api_keys` 非空时要求请求携带
/// `X-Api-Key` 且命中其一，否则 401。空表 = 关闭（内网默认形态，不带
/// 凭据照常通行——这也是既有调用方/e2e 不受影响的原因）。
async fn require_api_key(
    State(state): State<Arc<AppState>>,
    req: axum::extract::Request,
    next: Next,
) -> Result<Response, ApiErrorResponse> {
    let keys = state.config.security.api_keys.as_slice();
    if keys.is_empty() {
        return Ok(next.run(req).await);
    }
    let provided = req
        .headers()
        .get("X-Api-Key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if api_key_matches(provided, keys) {
        Ok(next.run(req).await)
    } else {
        Err(ApiErrorResponse::new(
            StatusCode::UNAUTHORIZED,
            "missing or invalid API key (X-Api-Key)",
        ))
    }
}

/// 常时形状比较：逐字节累积异或且循环走满；多 key 全量判定不因命中
/// 提前返回（避免「第几个 key 正确」的时序侧信道）。长度不同直接不等
/// ——长度不属秘密面（HTTP 头长度可观测），无需填充。
fn api_key_matches(candidate: &str, keys: &[String]) -> bool {
    let candidate = candidate.as_bytes();
    let mut hit = false;
    for key in keys {
        let expected = key.as_bytes();
        let mut diff = 1u8;
        if candidate.len() == expected.len() {
            diff = 0;
            for i in 0..expected.len() {
                diff |= candidate[i] ^ expected[i];
            }
        }
        hit |= diff == 0;
    }
    hit
}
