//! ClickHouse 导出面（原 main.rs 1312-1587 行）：导出目录清单、
//! market_data / realtime_quotes 两类 Parquet 即时导出、湖读旁路与
//! 写透落湖，行为不变。

use std::sync::Arc;

use crate::market_routes::ApiErrorResponse;
use crate::AppState;
use alpha_storage::lake::{bars_to_parquet_bytes, MarketBar};
use axum::{
    extract::{Query, State},
    http::{header, HeaderValue},
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub(crate) struct MarketDataParquetParams {
    pub(crate) symbol: String,
    pub(crate) start: Option<String>,
    pub(crate) end: Option<String>,
    pub(crate) days: Option<u32>,
    pub(crate) limit: Option<u64>,
    /// true = 读湖旁路（不查 ClickHouse，需 lake.enabled）
    pub(crate) from_lake: Option<bool>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ClickhouseExportDescriptor {
    id: &'static str,
    description: &'static str,
    params: &'static [&'static str],
}

pub(crate) const CLICKHOUSE_EXPORTS: &[ClickhouseExportDescriptor] = &[
    ClickhouseExportDescriptor {
        id: "market_data",
        description: "OHLCV market data (timestamp, symbol, open/high/low/close, volume)",
        params: &[
            "symbol",
            "start|days",
            "end",
            "limit",
            "from_lake(optional)",
        ],
    },
    ClickhouseExportDescriptor {
        id: "realtime_quotes",
        description: "Realtime quotes snapshot (symbol, last/bid/ask, volume, timestamp, change)",
        params: &["symbols(optional)", "limit"],
    },
];

pub(crate) async fn list_clickhouse_exports() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "success": true,
        "exports": CLICKHOUSE_EXPORTS,
    }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct ClickhouseExportParams {
    query_id: String,
    symbol: Option<String>,
    symbols: Option<String>,
    start: Option<String>,
    end: Option<String>,
    days: Option<u32>,
    limit: Option<u64>,
    from_lake: Option<bool>,
}

/// 从 ClickHouse 导出预设数据集（Parquet）
///
/// 注意：此接口不接受任意 SQL，仅允许 query_id 白名单。
pub(crate) async fn get_clickhouse_export_parquet(
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
                from_lake: params.from_lake,
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

pub(crate) async fn export_market_data_parquet(
    state: Arc<AppState>,
    params: MarketDataParquetParams,
) -> Result<axum::response::Response, ApiErrorResponse> {
    let from_lake = params.from_lake.unwrap_or(false);
    if from_lake && state.lake.is_none() {
        return Err(ApiErrorResponse::bad_request(
            "Parquet lake is not enabled".to_string(),
        ));
    }
    if !from_lake && state.clickhouse.is_none() {
        return Err(ApiErrorResponse::bad_request(
            "ClickHouse backend is not enabled".to_string(),
        ));
    }

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

    let data = if from_lake {
        read_market_data_from_lake(&state, &params.symbol, start, end, limit)?
    } else {
        let clickhouse = state
            .clickhouse
            .as_ref()
            .expect("clickhouse presence checked above");
        let data = clickhouse
            .query_market_data_parquet(&params.symbol, start, end, Some(limit))
            .await
            .map_err(ApiErrorResponse::internal)?;
        if state.lake.is_some() {
            write_through_to_lake(&state, &params.symbol, start, end, limit).await;
        }
        data
    };

    let mut response = axum::response::Response::new(axum::body::Body::from(data));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-parquet"),
    );
    Ok(response)
}

/// 读湖旁路：区间 + symbol 过滤后按 limit 截断，湖 schema 直出 Parquet。
pub(crate) fn read_market_data_from_lake(
    state: &AppState,
    symbol: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    limit: u64,
) -> Result<Vec<u8>, ApiErrorResponse> {
    let lake = state
        .lake
        .as_ref()
        .expect("lake presence checked by caller");
    let mut bars = lake
        .read_range(
            &state.config.lake.layer,
            &state.config.lake.table,
            symbol,
            start.timestamp_millis(),
            end.timestamp_millis(),
        )
        .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
    bars.truncate(limit as usize);
    bars_to_parquet_bytes(&bars).map_err(|e| ApiErrorResponse::internal(e.to_string()))
}

/// 导出成功后把同一窗口的行级数据按交易日分区落湖。失败只告警，不影响响应
/// （与 collector 归档同口径：旁路落盘失败不拒绝主请求）。
pub(crate) async fn write_through_to_lake(
    state: &AppState,
    symbol: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    limit: u64,
) {
    let (Some(lake), Some(clickhouse)) = (state.lake.as_ref(), state.clickhouse.as_ref()) else {
        return;
    };
    let rows = match clickhouse
        .query_market_data(symbol, start, end, Some(limit))
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            tracing::warn!(%symbol, "lake write-through query failed: {}", err);
            return;
        }
    };
    if rows.is_empty() {
        return;
    }
    let bars: Vec<MarketBar> = rows
        .into_iter()
        .map(|r| MarketBar {
            timestamp_ms: r.timestamp.timestamp_millis(),
            symbol: r.symbol,
            open: r.open_price,
            high: r.high_price,
            low: r.low_price,
            close: r.close_price,
            volume: r.volume as f64,
        })
        .collect();
    let (layer, table) = (
        state.config.lake.layer.clone(),
        state.config.lake.table.clone(),
    );
    let lake = lake.clone();
    // 落盘是阻塞 IO（fsync），放到 blocking 线程避免占 runtime worker。
    let result =
        tokio::task::spawn_blocking(move || lake.write_bars(&layer, &table, &bars, None)).await;
    match result {
        Ok(Ok(reports)) => {
            tracing::info!(%symbol, partitions = reports.len(), "lake write-through done");
        }
        Ok(Err(err)) => tracing::warn!(%symbol, "lake write-through failed: {}", err),
        Err(err) => tracing::warn!(%symbol, "lake write-through task failed: {}", err),
    }
}

/// 从 ClickHouse 导出市场数据（Parquet）
///
/// 供 DuckDB-WASM / DuckDB Desktop 直接 `read_parquet()` 使用。
pub(crate) async fn get_clickhouse_market_data_parquet(
    State(state): State<Arc<AppState>>,
    Query(params): Query<MarketDataParquetParams>,
) -> Result<axum::response::Response, ApiErrorResponse> {
    export_market_data_parquet(state, params).await
}

pub(crate) fn parse_datetime(input: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(input)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

pub(crate) async fn export_realtime_quotes_parquet(
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
