//! 行情 / 指标 / 绩效路由（原 main.rs 1589-2310 行 + 测试块）：/query
//! （只读门 + 热层/湖两层）、历史 JSON/CSV 导出、指标计算、绩效分析、
//! 共享 ApiErrorResponse 与 tracing 初始化。

use std::{sync::Arc, time::Instant};

use crate::settings::TelemetryConfig;
use crate::AppState;
use alpha_core::errors::AlphaError;
use alpha_core::models::{AnalysisResult, MarketData};
use alpha_storage::{TimeSeriesPoint, TimeSeriesStorage};
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Duration, Utc};
use datafusion::arrow::{
    array::{
        ArrayRef, Float64Array, Int64Array, StringArray, StringViewArray,
        TimestampMillisecondArray, UInt64Array,
    },
    datatypes::{DataType, TimeUnit},
    record_batch::RecordBatch,
};
use serde::{Deserialize, Serialize};

/// 进程 RSS 与系统总内存（L464 `alpha_dataengine_memory_bytes` 数据源）。
/// Linux-only：/proc 不可得（macOS/Windows）返回 None——不打点即可，
/// 告警规则与诊断引擎对缺数据口径均为跳过而非误报。
pub(crate) fn process_and_system_memory_bytes() -> Option<(u64, u64)> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let rss_pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    let total_kb: u64 = meminfo
        .lines()
        .find_map(|l| l.strip_prefix("MemTotal:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    // 页大小按 4096 计（x86_64/aarch64 Linux 主流配置；告警阈值是比率口径，
    // 页大小偏差只整体缩放 RSS，不改变趋势判读）
    Some((rss_pages * 4096, total_kb * 1024))
}

/// /query 只读门（安全扫描 S-2）：用 sqlparser 解析并校验全部语句均为查询类
/// （SELECT/WITH/EXPLAIN/SHOW/DESCRIBE）。解析失败一律拒绝（防御性）——客户端
/// 不可经 /query 执行 DDL/DML（CREATE EXTERNAL TABLE ... LOCATION 'file://…'
/// 可读任意文件、INSERT/UPDATE/DELETE/DROP 破坏数据）。
pub(crate) fn is_read_only_query(sql: &str) -> bool {
    use datafusion::sql::sqlparser::ast::Statement;
    use datafusion::sql::sqlparser::dialect::GenericDialect;
    use datafusion::sql::sqlparser::parser::Parser;

    match Parser::parse_sql(&GenericDialect, sql) {
        Ok(stmts) => {
            !stmts.is_empty()
                && stmts.iter().all(|s| {
                    matches!(
                        s,
                        Statement::Query(_)
                            | Statement::Explain { .. }
                            | Statement::ExplainTable { .. }
                            | Statement::ShowVariable { .. }
                            | Statement::ShowCreate { .. }
                            | Statement::ShowColumns { .. }
                            | Statement::ShowTables { .. }
                    )
                })
        }
        Err(_) => false,
    }
}

/// 执行 SQL 查询
#[tracing::instrument(skip(state, request))]
pub(crate) async fn execute_query(
    State(state): State<Arc<AppState>>,
    Json(request): Json<QueryRequest>,
) -> Result<Json<QueryResponse>, ApiErrorResponse> {
    let start = Instant::now();
    if !is_read_only_query(&request.query) {
        return Err(ApiErrorResponse::bad_request(
            "/query 只允许只读查询（SELECT/WITH/EXPLAIN/SHOW/DESCRIBE）",
        ));
    }
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

    // L464 业务指标：查询耗时直方图（DataEngineQueryLatencyHigh p95 数据源）
    metrics::histogram!("alpha_dataengine_query_duration_seconds")
        .record(execution_time_ms as f64 / 1000.0);
    // 进程 RSS / 系统总内存（DataEngineMemoryPressure 数据源；Linux /proc，
    // 非 Linux 平台不打点——规则侧无数据自动跳过，不会误报）
    if let Some((used, total)) = process_and_system_memory_bytes() {
        metrics::gauge!("alpha_dataengine_memory_bytes", "mode" => "used").set(used as f64);
        metrics::gauge!("alpha_dataengine_memory_bytes", "mode" => "total").set(total as f64);
    }

    let data = record_batches_to_json(&results)
        .map_err(|err| ApiErrorResponse::internal(err.to_string()))?;

    Ok(Json(QueryResponse {
        success: true,
        row_count: rows,
        data,
        execution_time_ms,
    }))
}

/// 行情历史点集加载（JSON 与 CSV 导出两条面共用）：days 下限 1，
/// limit 截断保留尾部（最新）窗口。
pub(crate) async fn load_history_points(
    state: &AppState,
    symbol: &str,
    days: u32,
    limit: Option<usize>,
) -> Result<Vec<TimeSeriesPoint>, ApiErrorResponse> {
    let end_time = Utc::now();
    let start_time = end_time - Duration::days(days as i64);

    let mut points = state
        .storage
        .get_data_in_range(symbol, start_time, end_time)
        .await
        .map_err(ApiErrorResponse::from)?;

    if let Some(limit) = limit {
        if points.len() > limit {
            points = points.split_off(points.len() - limit);
        }
    }

    Ok(points)
}

/// 获取股票历史数据
#[tracing::instrument(skip(state))]
pub(crate) async fn get_stock_history(
    State(state): State<Arc<AppState>>,
    Path(symbol): Path<String>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<HistoryResponse>, ApiErrorResponse> {
    let default_days = state.config.data.lookback_days;
    let days = params.days.unwrap_or(default_days).max(1);
    let points = load_history_points(&state, &symbol, days, params.limit).await?;

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

/// 第三方 CSV 导出（L504）：与 /stocks/:symbol/history 同源同参（days/
/// limit），text/csv 输出 `timestamp,price,volume` 表头 + RFC3339 行。
/// 三列均为无逗号类型（时间戳/浮点/整数），无需引号转义；metadata 等
/// 富字段不进导出面（机器消费以 JSON /query 与 parquet 导出为准）。
#[tracing::instrument(skip(state))]
pub(crate) async fn get_stock_history_csv(
    State(state): State<Arc<AppState>>,
    Path(symbol): Path<String>,
    Query(params): Query<HistoryParams>,
) -> Result<impl IntoResponse, ApiErrorResponse> {
    let default_days = state.config.data.lookback_days;
    let days = params.days.unwrap_or(default_days).max(1);
    let points = load_history_points(&state, &symbol, days, params.limit).await?;

    let mut body = String::from("timestamp,price,volume\n");
    for point in &points {
        body.push_str(&point.timestamp.to_rfc3339());
        body.push(',');
        body.push_str(&point.value.to_string());
        body.push(',');
        body.push_str(&point.volume.unwrap_or(0).to_string());
        body.push('\n');
    }

    Ok(([(header::CONTENT_TYPE, "text/csv; charset=utf-8")], body))
}

/// 获取股票指标快照
#[tracing::instrument(skip(state))]
pub(crate) async fn get_stock_indicators(
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
pub(crate) async fn calculate_indicators(
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
pub(crate) async fn calculate_performance(
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

pub(crate) async fn load_points(
    state: &Arc<AppState>,
    symbol: &str,
    lookback_days: u32,
) -> Result<Vec<TimeSeriesPoint>, ApiErrorResponse> {
    fetch_points(state, symbol, lookback_days)
        .await
        .map_err(ApiErrorResponse::from)
}

pub(crate) fn points_to_market_data(symbol: &str, points: &[TimeSeriesPoint]) -> Vec<MarketData> {
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
pub(crate) struct QueryRequest {
    query: String,
}

/// 查询响应
#[derive(Debug, Serialize)]
pub(crate) struct QueryResponse {
    success: bool,
    row_count: usize,
    data: serde_json::Value,
    execution_time_ms: u64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct HistoryParams {
    days: Option<u32>,
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct HistoryResponse {
    success: bool,
    symbol: String,
    period_days: u32,
    data_points: usize,
    data: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct IndicatorParams {
    rsi_period: Option<u32>,
    sma_short: Option<u32>,
    sma_long: Option<u32>,
    macd_fast: Option<u32>,
    macd_slow: Option<u32>,
    macd_signal: Option<u32>,
    lookback_days: Option<u32>,
}

#[derive(Debug, Serialize)]
pub(crate) struct IndicatorsResponse {
    success: bool,
    symbol: String,
    data: serde_json::Value,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub(crate) struct IndicatorCalculationRequest {
    symbol: String,
    lookback_days: Option<u32>,
    indicators: Option<Vec<String>>,
    rsi_period: Option<u32>,
    macd_fast: Option<u32>,
    macd_slow: Option<u32>,
    macd_signal: Option<u32>,
}

#[derive(Debug, Serialize)]
pub(crate) struct IndicatorCalculationResponse {
    success: bool,
    symbol: String,
    indicators: Vec<String>,
    analysis: AnalysisResult,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PerformanceRequest {
    symbol: String,
    period_days: u32,
}

#[derive(Debug, Serialize)]
pub(crate) struct PerformanceResponse {
    success: bool,
    symbol: String,
    period_days: u32,
    performance: PerformanceMetrics,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PerformanceMetrics {
    pub(crate) total_return: f64,
    pub(crate) annualized_return: f64,
    pub(crate) volatility: f64,
    pub(crate) max_drawdown: f64,
    pub(crate) sharpe_ratio: f64,
    pub(crate) win_rate: f64,
}

pub(crate) fn calculate_performance_metrics(points: &[TimeSeriesPoint]) -> PerformanceMetrics {
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

pub(crate) async fn fetch_points(
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

pub(crate) async fn build_market_data_record_batch(
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

pub(crate) fn record_batches_to_json(batches: &[RecordBatch]) -> anyhow::Result<serde_json::Value> {
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

pub(crate) fn array_value_to_json(
    array: &ArrayRef,
    row_idx: usize,
) -> anyhow::Result<serde_json::Value> {
    if array.is_null(row_idx) {
        return Ok(serde_json::Value::Null);
    }

    match array.data_type() {
        DataType::Utf8 => {
            let array = array.as_any().downcast_ref::<StringArray>().unwrap();
            Ok(serde_json::Value::String(array.value(row_idx).to_string()))
        }
        DataType::Utf8View => {
            let array = array.as_any().downcast_ref::<StringViewArray>().unwrap();
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

pub(crate) fn init_tracing(config: &TelemetryConfig) -> anyhow::Result<()> {
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
pub(crate) struct ApiErrorResponse {
    status: StatusCode,
    message: String,
}

impl ApiErrorResponse {
    pub(crate) fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
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
    use std::sync::{Mutex, RwLock};

    use crate::bootstrap::initialize_persistence;
    use crate::exports::{export_market_data_parquet, MarketDataParquetParams};
    use crate::normalizer::run_lake_maintenance_round;
    use crate::outlier::PriceOutlierMonitor;
    use crate::sequence_gap::SequenceGapMonitor;
    use crate::settings::{AppConfig, StorageConfig};
    use crate::source_divergence::SourceDivergenceMonitor;
    use crate::{
        api_key_matches, build_router, build_session_context, global_metrics_handle,
        seed_instruments, AppState, MirrorRetryBuffer, RecentPayloads, LAKE_TABLE,
        MIRROR_RETRY_BUFFER_CAP, PAYLOAD_DEDUP_WINDOW,
    };
    use alpha_core::{
        analytics::AnalysisEngine, errors::AlphaResult, indicators::TechnicalIndicators,
    };
    use alpha_storage::lake::{LakeWriter, MarketBar};
    use alpha_storage::partition::PartitionScheme;
    use alpha_storage::TimescaleTimeSeriesStorage;
    use axum::http::HeaderValue;
    use axum::Router;
    use datafusion::prelude::SessionContext;

    fn test_config() -> Arc<AppConfig> {
        Arc::new(AppConfig::default())
    }

    /// 手工装配AppState（clickhouse/lake 按 test 需要给定，其余走默认组件）。
    fn lake_state(lake: Option<Arc<LakeWriter>>) -> Arc<AppState> {
        let mut config = (*test_config()).clone();
        if lake.is_some() {
            // 与生产装配同口径：湖启用时 /query 才会注册 ListingTable，
            // 会话也才需要关掉子目录忽略（见 build_session_context）。
            config.lake.enabled = true;
        }
        let config = Arc::new(config);
        Arc::new(AppState {
            session: build_session_context(&config),
            storage: Arc::new(TimeSeriesStorage::new()),
            persistence: None,
            clickhouse: None,
            seen_payloads: Arc::new(Mutex::new(RecentPayloads::with_capacity(
                PAYLOAD_DEDUP_WINDOW,
            ))),
            mirror_retries: Arc::new(MirrorRetryBuffer::with_capacity(MIRROR_RETRY_BUFFER_CAP)),
            indicators: TechnicalIndicators::new(),
            analysis: AnalysisEngine::new(),
            config,
            metrics: global_metrics_handle().clone(),
            instruments: Arc::new(RwLock::new(seed_instruments())),
            sequence_gaps: Arc::new(SequenceGapMonitor::new()),
            price_outliers: Arc::new(PriceOutlierMonitor::from_env()),
            source_divergence: Arc::new(SourceDivergenceMonitor::from_env()),
            lake,
        })
    }

    /// 解码导出响应体（湖 schema：col1=symbol、col5=close）。
    fn decode_parquet_bars(bytes: &[u8]) -> Vec<(String, f64)> {
        use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dump.parquet");
        std::fs::write(&path, bytes).unwrap();
        let reader = ParquetRecordBatchReaderBuilder::try_new(std::fs::File::open(&path).unwrap())
            .unwrap()
            .build()
            .unwrap();
        let mut rows = Vec::new();
        for batch in reader {
            let batch = batch.unwrap();
            let symbol = batch
                .column(1)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let close = batch
                .column(5)
                .as_any()
                .downcast_ref::<Float64Array>()
                .unwrap();
            for i in 0..batch.num_rows() {
                rows.push((symbol.value(i).to_string(), close.value(i)));
            }
        }
        rows
    }

    #[tokio::test]
    async fn from_lake_export_serves_written_bars_without_clickhouse() {
        let dir = tempfile::tempdir().unwrap();
        let lake = Arc::new(LakeWriter::new(dir.path(), PartitionScheme::DateOnly));
        let state = lake_state(Some(lake.clone()));

        // 两个 symbol、两个交易日：窗口过滤只该命中后一日的那条。
        let t1 = Utc::now() - Duration::days(3);
        let t2 = Utc::now() - Duration::days(1);
        let bars = vec![
            MarketBar {
                timestamp_ms: t1.timestamp_millis(),
                symbol: "600519".to_string(),
                open: 1.0,
                high: 2.0,
                low: 0.5,
                close: 1.5,
                volume: 10.0,
            },
            MarketBar {
                timestamp_ms: t2.timestamp_millis(),
                symbol: "600519".to_string(),
                open: 2.0,
                high: 3.0,
                low: 1.5,
                close: 2.5,
                volume: 20.0,
            },
            MarketBar {
                timestamp_ms: t1.timestamp_millis(),
                symbol: "000001".to_string(),
                open: 9.0,
                high: 9.5,
                low: 8.5,
                close: 9.2,
                volume: 5.0,
            },
        ];
        lake.write_bars("silver", "market_data", &bars, None)
            .unwrap();

        let response = export_market_data_parquet(
            state.clone(),
            MarketDataParquetParams {
                symbol: "600519".to_string(),
                start: Some((t2 - Duration::hours(1)).to_rfc3339()),
                end: None,
                days: None,
                limit: Some(10),
                from_lake: Some(true),
            },
        )
        .await
        .unwrap()
        .into_response();
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("application/x-parquet"))
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let rows = decode_parquet_bars(&body);
        assert_eq!(rows.len(), 1, "窗口+symbol 过滤后只剩一条");
        assert_eq!(rows[0], ("600519".to_string(), 2.5));

        // limit 截断：默认 90 天回看覆盖全部两条，limit=1 只剩一条。
        let response = export_market_data_parquet(
            state,
            MarketDataParquetParams {
                symbol: "600519".to_string(),
                start: None,
                end: None,
                days: None,
                limit: Some(1),
                from_lake: Some(true),
            },
        )
        .await
        .unwrap()
        .into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(decode_parquet_bars(&body).len(), 1, "limit 截断生效");
    }

    #[tokio::test]
    async fn lake_and_clickhouse_gate_export_modes_independently() {
        let state = lake_state(None);

        // from_lake=true 但湖未启用 → 400。
        let err = export_market_data_parquet(
            state.clone(),
            MarketDataParquetParams {
                symbol: "600519".to_string(),
                start: None,
                end: None,
                days: None,
                limit: None,
                from_lake: Some(true),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);

        // 即时导出（from_lake 缺省）但 ClickHouse 未启用 → 400（行为不变）。
        let err = export_market_data_parquet(
            state,
            MarketDataParquetParams {
                symbol: "600519".to_string(),
                start: None,
                end: None,
                days: None,
                limit: None,
                from_lake: None,
            },
        )
        .await
        .unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
    }

    /// 湖维护单轮（compaction + manifest 重建）：默认阈值下小湖不触发合并，
    /// manifest 必须落盘且统计口径正确——后台 worker 只是周期调用本函数。
    #[tokio::test]
    async fn lake_maintenance_round_rebuilds_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let lake = Arc::new(LakeWriter::new(dir.path(), PartitionScheme::DateOnly));
        let bars = vec![MarketBar {
            timestamp_ms: Utc::now().timestamp_millis(),
            symbol: "600519".to_string(),
            open: 1.0,
            high: 2.0,
            low: 0.5,
            close: 1.5,
            volume: 10.0,
        }];
        lake.write_bars("silver", "market_data", &bars, None)
            .unwrap();

        run_lake_maintenance_round(&lake, "silver", "market_data").await;

        let manifest = lake
            .read_manifest("silver", "market_data")
            .unwrap()
            .unwrap();
        assert_eq!(manifest.total_files, 1);
        assert_eq!(manifest.total_rows, 1);
        assert!(dir
            .path()
            .join("silver/market_data/_manifest.json")
            .exists());
    }

    #[test]
    fn api_key_comparison_is_exact_and_full_scan() {
        let keys = |keys: &[&str]| keys.iter().map(|k| k.to_string()).collect::<Vec<_>>();

        // 精确相等才命中（逐字节异或归零）
        assert!(api_key_matches("secret-1", &keys(&["secret-1"])));
        assert!(!api_key_matches("secret-2", &keys(&["secret-1"])));
        // 长度不同直接不等（长度不属秘密面），且不 panic
        assert!(!api_key_matches("short", &keys(&["a-much-longer-key"])));
        // 多 key：任一命中即可；候选为空串/空表永不命中
        assert!(api_key_matches("b", &keys(&["a", "b"])));
        assert!(!api_key_matches("", &keys(&["a"])));
        assert!(!api_key_matches("anything", &keys(&[])));
    }

    /// API key 门：默认关闭无感通行；配置后数据面 401/命中通行，
    /// /health、/metrics 运维面豁免（不带业务凭据的存活探测不受影响）。
    #[tokio::test]
    async fn api_key_gate_off_by_default_and_enforced_when_configured() {
        use axum::body::Body;
        use tower::ServiceExt;

        let request = |app: Router, path: &'static str, key: Option<&str>| {
            let mut builder = axum::http::Request::builder().uri(path);
            if let Some(key) = key {
                builder = builder.header("X-Api-Key", key);
            }
            app.oneshot(builder.body(Body::empty()).unwrap())
        };

        // 关闭态（默认配置）：无凭据照常通行
        let off = build_router(Arc::new(AppState::new(test_config()).await));
        let res = request(off, "/stocks/GATE/history", None).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // 开启态：数据面无凭据 401、错凭据 401、任一命中凭据 200
        let mut config = (*test_config()).clone();
        config.security.api_keys = vec!["k1".to_string(), "k2".to_string()];
        let on = build_router(Arc::new(AppState::new(Arc::new(config)).await));
        let res = request(on.clone(), "/stocks/GATE/history", None)
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let res = request(on.clone(), "/stocks/GATE/history", Some("wrong-key"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let res = request(on.clone(), "/stocks/GATE/history", Some("k2"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        // CSV 导出面同受门管控（任一命中 key 皆可）
        let res = request(on.clone(), "/stocks/GATE/history.csv", Some("k1"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // 运维面豁免：无凭据的存活探测与抓取器照常
        let res = request(on.clone(), "/health", None).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let res = request(on, "/metrics", None).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    /// CSV 导出面：表头 + RFC3339 行 + 缺 volume 补 0；空数据只回表头。
    #[tokio::test]
    async fn history_csv_exports_header_rows_and_placeholder_volume() {
        let state = Arc::new(AppState::new(test_config()).await);
        let now = Utc::now();
        state
            .storage
            .add_market_data_batch(&[
                MarketData {
                    symbol: "CSVT".to_string(),
                    timestamp: now - Duration::minutes(2),
                    price: 100.5,
                    volume: 1000,
                    bid: None,
                    ask: None,
                    open: None,
                    high: None,
                    low: None,
                },
                MarketData {
                    symbol: "CSVT".to_string(),
                    timestamp: now - Duration::minutes(1),
                    price: 101.0,
                    volume: 0,
                    bid: None,
                    ask: None,
                    open: None,
                    high: None,
                    low: None,
                },
            ])
            .await
            .unwrap();

        let response = get_stock_history_csv(
            State(state.clone()),
            Path("CSVT".to_string()),
            Query(HistoryParams {
                days: Some(1),
                limit: None,
            }),
        )
        .await
        .unwrap()
        .into_response();
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("text/csv; charset=utf-8"))
        );
        let body = String::from_utf8(
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines[0], "timestamp,price,volume");
        assert_eq!(lines.len(), 3, "header + two seeded rows");
        assert!(lines[1].ends_with(",100.5,1000"));
        assert!(lines[2].ends_with(",101,0"), "zero volume exports as 0");

        // 未播种 symbol：只回表头（空数据不报错，机器消费方按行数判空）
        let response = get_stock_history_csv(
            State(state),
            Path("EMPTY".to_string()),
            Query(HistoryParams {
                days: Some(1),
                limit: None,
            }),
        )
        .await
        .unwrap()
        .into_response();
        let body = String::from_utf8(
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert_eq!(body, "timestamp,price,volume\n");
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

    /// S-2 回归：/query 只读门拒绝 DDL/DML（CREATE EXTERNAL TABLE 读任意文件、
    /// DROP/INSERT/UPDATE/DELETE 破坏数据），只读 SELECT 照常放行。
    #[tokio::test]
    async fn execute_query_rejects_ddl_and_dml() {
        let state = Arc::new(AppState::new(test_config()).await);
        for ddl in [
            "CREATE EXTERNAL TABLE t STORED AS CSV LOCATION 'file:///etc/passwd'",
            "DROP TABLE market_data",
            "INSERT INTO market_data VALUES (1, 2)",
            "DELETE FROM market_data WHERE symbol = 'AAPL'",
        ] {
            let res = execute_query(
                State(state.clone()),
                Json(QueryRequest {
                    query: ddl.to_string(),
                }),
            )
            .await;
            assert!(res.is_err(), "应拒绝非只读语句: {ddl}");
        }
        let Json(response) = execute_query(
            State(state),
            Json(QueryRequest {
                query: "SELECT 1 AS x".to_string(),
            }),
        )
        .await
        .unwrap();
        assert!(response.success);
    }

    /// /query 湖历史层（data-lake-parquet §6 接缝）：落湖分区注册为
    /// ListingTable 后 SQL 直扫（trade_date hive 目录即分区列，路径裁剪零配置），
    /// 且可与热层 MemTable UNION 两层合扫。
    #[tokio::test]
    async fn execute_query_scans_lake_listing_table_and_unions_hot_layer() {
        let dir = tempfile::tempdir().unwrap();
        let lake = Arc::new(LakeWriter::new(dir.path(), PartitionScheme::DateOnly));
        let state = lake_state(Some(lake.clone()));

        let t = Utc::now() - Duration::days(1);
        let bars = vec![
            MarketBar {
                timestamp_ms: t.timestamp_millis(),
                symbol: "600519".to_string(),
                open: 41.0,
                high: 43.0,
                low: 40.5,
                close: 42.0,
                volume: 12.0,
            },
            MarketBar {
                timestamp_ms: t.timestamp_millis(),
                symbol: "000001".to_string(),
                open: 9.0,
                high: 9.5,
                low: 8.5,
                close: 9.2,
                volume: 5.0,
            },
        ];
        lake.write_bars("silver", "market_data", &bars, None)
            .unwrap();
        let trade_date = alpha_storage::trade_date_of(t.timestamp_millis());

        let Json(response) = execute_query(
            State(state.clone()),
            Json(QueryRequest {
                query: format!(
                    "SELECT symbol, MAX(close_price) AS max_close FROM {LAKE_TABLE} \
                     WHERE trade_date = '{trade_date}' GROUP BY symbol ORDER BY symbol"
                ),
            }),
        )
        .await
        .unwrap();

        assert!(response.success, "lake query failed: {:?}", response.data);
        assert_eq!(response.row_count, 2);
        assert_eq!(response.data[0]["symbol"], "000001");
        assert_eq!(response.data[0]["max_close"], 9.2);
        assert_eq!(response.data[1]["symbol"], "600519");
        assert_eq!(response.data[1]["max_close"], 42.0);

        // 热层（内存 MemTable）与湖层 UNION 两层合扫。
        state
            .storage
            .add_market_data_batch(&[MarketData {
                symbol: "AAPL".to_string(),
                timestamp: t,
                price: 100.0,
                volume: 10,
                bid: None,
                ask: None,
                open: None,
                high: None,
                low: None,
            }])
            .await
            .unwrap();
        let Json(response) = execute_query(
            State(state),
            Json(QueryRequest {
                query: format!(
                    "SELECT COUNT(*) AS total FROM ( \
                         SELECT symbol FROM market_data \
                         UNION ALL \
                         SELECT symbol FROM {LAKE_TABLE} WHERE trade_date = '{trade_date}' \
                     )"
                ),
            }),
        )
        .await
        .unwrap();

        assert!(response.success, "union query failed: {:?}", response.data);
        assert_eq!(response.row_count, 1);
        assert_eq!(response.data[0]["total"], 3, "热层 1 条 + 湖层 2 条");
    }

    /// /query 自定义聚合函数面（query_udfs.rs）：注册后经 execute_query
    /// 对真实 market_data 列型算出 vwap/range_pct/avg_spread 三值。
    #[tokio::test]
    async fn execute_query_supports_custom_aggregate_udfs() {
        let state = Arc::new(AppState::new(test_config()).await);
        state.register_custom_functions().await.unwrap();
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
                    open: None,
                    high: None,
                    low: None,
                },
                MarketData {
                    symbol: "AAPL".to_string(),
                    timestamp: now + Duration::minutes(1),
                    price: 102.0,
                    volume: 20,
                    bid: Some(101.5),
                    ask: Some(102.5),
                    open: None,
                    high: None,
                    low: None,
                },
                MarketData {
                    symbol: "AAPL".to_string(),
                    timestamp: now + Duration::minutes(2),
                    price: 95.0,
                    volume: 0, // 零量行不改 vwap 权重
                    bid: Some(100.0),
                    ask: Some(101.2),
                    open: None,
                    high: None,
                    low: None,
                },
            ])
            .await
            .unwrap();

        let Json(response) = execute_query(
            State(state),
            Json(QueryRequest {
                query:
                    "SELECT symbol, vwap(price, volume) AS vwap, range_pct(price) AS range_pct, \
                        avg_spread(bid, ask) AS spread FROM market_data GROUP BY symbol"
                        .to_string(),
            }),
        )
        .await
        .unwrap();

        assert!(response.success);
        assert_eq!(response.row_count, 1);
        let row = &response.data[0];
        assert_eq!(row["symbol"], "AAPL");
        // vwap = (100×10 + 102×20 + 95×0) / 30 = 3040/30
        let vwap = row["vwap"].as_f64().unwrap();
        assert!((vwap - 3040.0 / 30.0).abs() < 1e-9);
        // 振幅 = (102−95)/95×100
        let range_pct = row["range_pct"].as_f64().unwrap();
        assert!((range_pct - 7.0 / 95.0 * 100.0).abs() < 1e-9);
        // 价差均值 = (1.0 + 1.0 + 1.2) / 3
        let spread = row["spread"].as_f64().unwrap();
        assert!((spread - 3.2 / 3.0).abs() < 1e-9);
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
            instruments: Arc::new(RwLock::new(seed_instruments())),
            sequence_gaps: Arc::new(SequenceGapMonitor::new()),
            price_outliers: Arc::new(PriceOutlierMonitor::from_env()),
            source_divergence: Arc::new(SourceDivergenceMonitor::from_env()),
            lake: None,
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

    /// /instruments（architecture-review §3.2 数据契约面）：
    /// 列表可组合过滤（exchange/type/symbol/q）；单条按规范键精确命中；
    /// 000001 双市场消歧键共存；未知 id 404、非法参数 400。
    #[tokio::test]
    async fn instruments_endpoints_list_filter_and_get() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;

        let app = build_router(Arc::new(AppState::new(test_config()).await));
        let get_json = |app: Router, uri: String| {
            let app = app.clone();
            async move {
                let res = app
                    .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                let status = res.status();
                let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
                    .await
                    .unwrap();
                (
                    status,
                    serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
                )
            }
        };

        // 列表：种子全覆盖（9 条 + 双交易所 + 三种类型）
        let (status, body) = get_json(app.clone(), "/instruments".to_string()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["total"], 9, "种子注册表数量");
        let ids: Vec<&str> = body["instruments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["instrument_id"].as_str().unwrap())
            .collect();
        // 000001 双市场消歧键必须共存
        assert!(ids.contains(&"cn.sse.000001"), "上证指数");
        assert!(ids.contains(&"cn.szse.000001"), "平安银行");
        assert!(ids.contains(&"cn.sse.600519"), "贵州茅台");

        // 组合过滤
        let (status, body) = get_json(app.clone(), "/instruments?exchange=sse".to_string()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body["instruments"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["exchange"] == "SSE"));

        let (status, body) = get_json(app.clone(), "/instruments?type=etf".to_string()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["total"], 2, "两只 ETF 种子");

        let (status, body) = get_json(app.clone(), "/instruments?q=茅台".to_string()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["total"], 1);
        assert_eq!(body["instruments"][0]["symbol"], "600519");

        // 非法参数 → 400（exchange 未知、type 未知）
        let (status, _) = get_json(app.clone(), "/instruments?exchange=NASD".to_string()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = get_json(app.clone(), "/instruments?type=option".to_string()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // 单条：规范键命中（含大小写宽容）；未知 404；格式非法 400
        let (status, body) = get_json(app.clone(), "/instruments/cn.sse.600519".to_string()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["instrument"]["name"], "贵州茅台");
        assert_eq!(body["instrument"]["currency"], "CNY");

        let (status, body) = get_json(app.clone(), "/instruments/CN.szse.000001".to_string()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["instrument"]["symbol"], "000001");
        assert_eq!(body["instrument"]["name"], "平安银行");

        let (status, _) = get_json(app.clone(), "/instruments/cn.sse.999999".to_string()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = get_json(app.clone(), "/instruments/not-an-id".to_string()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
