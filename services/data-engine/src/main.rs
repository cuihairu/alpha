//! Alpha Finance Data Engine
//!
//! 基于 Axum + DataFusion 的高性能数据处理服务

use std::{net::SocketAddr, sync::Arc, time::Instant};

use alpha_core::{
    analytics::AnalysisEngine,
    errors::AlphaError,
    indicators::TechnicalIndicators,
    models::{AnalysisResult, MarketData},
};
use alpha_storage::{TimeSeriesPoint, TimeSeriesStorage};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Duration, Utc};
use datafusion::{
    arrow::{
        array::{ArrayRef, Float64Array, StringArray, TimestampMillisecondArray},
        datatypes::{DataType, TimeUnit},
        record_batch::RecordBatch,
    },
    prelude::SessionContext,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

mod grpc;
mod settings;
use settings::{AppConfig, TelemetryConfig};

#[derive(Clone)]
struct AppState {
    session: SessionContext,
    storage: Arc<TimeSeriesStorage>,
    indicators: TechnicalIndicators,
    analysis: AnalysisEngine,
    config: Arc<AppConfig>,
}

impl AppState {
    fn new(config: Arc<AppConfig>) -> Self {
        Self {
            session: SessionContext::new(),
            storage: Arc::new(TimeSeriesStorage::new()),
            indicators: TechnicalIndicators::new(),
            analysis: AnalysisEngine::new(),
            config,
        }
    }

    async fn register_custom_functions(&self) -> anyhow::Result<()> {
        // 实际项目中在此注册自定义 UDF/UDAF。
        // 目前我们只记录日志以确保 DataFusion 会话可正常工作。
        tracing::info!("DataFusion session ready; custom UDF registration placeholder");
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

    let state = Arc::new(AppState::new(config.clone()));

    state.register_custom_functions().await?;
    if let Err(err) = state.seed_demo_data().await {
        tracing::warn!("Failed to seed demo data: {}", err);
    }

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
        .route("/query", post(execute_query))
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

/// 执行 SQL 查询
#[tracing::instrument(skip(state, request))]
async fn execute_query(
    State(state): State<Arc<AppState>>,
    Json(request): Json<QueryRequest>,
) -> Result<Json<QueryResponse>, ApiErrorResponse> {
    let start = Instant::now();
    let session = state.session.clone();

    let dataframe = session
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
}
