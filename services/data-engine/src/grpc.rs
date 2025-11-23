//! gRPC 服务实现

use std::{net::SocketAddr, sync::Arc};

use alpha_protocols::proto::data_engine::{
    data_engine_service_server::{DataEngineService, DataEngineServiceServer},
    HistoryPoint, HistoryRequest, HistoryResponse, IndicatorRequest, IndicatorResponse,
    IndicatorSeries, PerformanceRequest as GrpcPerformanceRequest,
    PerformanceResponse as GrpcPerformanceResponse,
};
use tonic::{async_trait, transport::Server, Request, Response, Status};

use crate::{calculate_performance_metrics, fetch_points, AppState};

pub async fn serve_grpc(addr: SocketAddr, state: Arc<AppState>) -> anyhow::Result<()> {
    let svc = DataEngineGrpc { state };

    tracing::info!("Data Engine gRPC server listening on {}", addr);

    Server::builder()
        .add_service(DataEngineServiceServer::new(svc))
        .serve(addr)
        .await?;

    Ok(())
}

struct DataEngineGrpc {
    state: Arc<AppState>,
}

#[async_trait]
impl DataEngineService for DataEngineGrpc {
    async fn get_indicators(
        &self,
        request: Request<IndicatorRequest>,
    ) -> Result<Response<IndicatorResponse>, Status> {
        let req = request.into_inner();
        let symbol = req.symbol.clone();
        let lookback = if req.lookback_days == 0 {
            self.state.config.data.lookback_days
        } else {
            req.lookback_days
        };

        let points = fetch_points(&self.state, &symbol, lookback)
            .await
            .map_err(map_alpha_error)?;

        if points.is_empty() {
            return Err(Status::not_found("no data available for symbol"));
        }

        let prices: Vec<f64> = points.iter().map(|p| p.value).collect();
        let timestamps: Vec<i64> = points
            .iter()
            .map(|p| p.timestamp.timestamp_millis())
            .collect();

        let rsi_period = non_zero_or(req.rsi_period, 14) as usize;
        let sma_short = non_zero_or(req.sma_short, 20) as usize;
        let sma_long = non_zero_or(req.sma_long, 50) as usize;
        let macd_fast = non_zero_or(req.macd_fast, 12) as usize;
        let macd_slow = non_zero_or(req.macd_slow, 26) as usize;
        let macd_signal = non_zero_or(req.macd_signal, 9) as usize;

        let indicators = &self.state.indicators;
        let rsi = indicators.calculate_rsi(&prices, rsi_period);
        let sma_short_values = indicators.calculate_sma(&prices, sma_short);
        let sma_long_values = indicators.calculate_sma(&prices, sma_long);
        let (macd_line, signal_line, histogram) =
            indicators.calculate_macd(&prices, macd_fast, macd_slow, macd_signal);
        let (upper, middle, lower) = indicators.calculate_bollinger_bands(&prices, 20, 2.0);

        let series = vec![
            IndicatorSeries {
                name: "RSI".into(),
                values: rsi,
                timestamps: timestamps.clone(),
            },
            IndicatorSeries {
                name: "SMA_SHORT".into(),
                values: sma_short_values,
                timestamps: timestamps.clone(),
            },
            IndicatorSeries {
                name: "SMA_LONG".into(),
                values: sma_long_values,
                timestamps: timestamps.clone(),
            },
            IndicatorSeries {
                name: "MACD_LINE".into(),
                values: macd_line,
                timestamps: timestamps.clone(),
            },
            IndicatorSeries {
                name: "MACD_SIGNAL".into(),
                values: signal_line,
                timestamps: timestamps.clone(),
            },
            IndicatorSeries {
                name: "MACD_HISTOGRAM".into(),
                values: histogram,
                timestamps: timestamps.clone(),
            },
            IndicatorSeries {
                name: "BOLLINGER_UPPER".into(),
                values: upper,
                timestamps: timestamps.clone(),
            },
            IndicatorSeries {
                name: "BOLLINGER_MIDDLE".into(),
                values: middle,
                timestamps: timestamps.clone(),
            },
            IndicatorSeries {
                name: "BOLLINGER_LOWER".into(),
                values: lower,
                timestamps,
            },
        ];

        Ok(Response::new(IndicatorResponse { symbol, series }))
    }

    async fn get_history(
        &self,
        request: Request<HistoryRequest>,
    ) -> Result<Response<HistoryResponse>, Status> {
        let req = request.into_inner();
        let symbol = req.symbol.clone();
        let days = if req.days == 0 {
            self.state.config.data.lookback_days
        } else {
            req.days
        };

        let mut points = fetch_points(&self.state, &symbol, days)
            .await
            .map_err(map_alpha_error)?;

        if let Some(limit) = non_zero_opt(req.limit) {
            if points.len() > limit as usize {
                points = points.split_off(points.len() - limit as usize);
            }
        }

        if points.is_empty() {
            return Err(Status::not_found("no data available for symbol"));
        }

        let grpc_points = points
            .into_iter()
            .map(|point| HistoryPoint {
                timestamp: point.timestamp.timestamp_millis(),
                price: point.value,
                volume: point.volume.unwrap_or_default(),
            })
            .collect();

        Ok(Response::new(HistoryResponse {
            symbol,
            period_days: days,
            points: grpc_points,
        }))
    }

    async fn calculate_performance(
        &self,
        request: Request<GrpcPerformanceRequest>,
    ) -> Result<Response<GrpcPerformanceResponse>, Status> {
        let req = request.into_inner();
        let points = fetch_points(&self.state, &req.symbol, req.period_days)
            .await
            .map_err(map_alpha_error)?;

        if points.len() < 2 {
            return Err(Status::invalid_argument(
                "not enough data points to calculate performance",
            ));
        }

        let metrics = calculate_performance_metrics(&points);

        Ok(Response::new(GrpcPerformanceResponse {
            symbol: req.symbol,
            total_return: metrics.total_return,
            annualized_return: metrics.annualized_return,
            volatility: metrics.volatility,
            max_drawdown: metrics.max_drawdown,
            sharpe_ratio: metrics.sharpe_ratio,
            win_rate: metrics.win_rate,
        }))
    }
}

fn non_zero_or(value: u32, default: u32) -> u32 {
    if value == 0 {
        default
    } else {
        value
    }
}

fn non_zero_opt(value: u32) -> Option<u32> {
    if value == 0 {
        None
    } else {
        Some(value)
    }
}

fn map_alpha_error(err: alpha_core::errors::AlphaError) -> Status {
    use alpha_core::errors::AlphaError::*;
    match err {
        DataNotFound(msg) => Status::not_found(msg),
        InvalidInput(msg) => Status::invalid_argument(msg),
        other => Status::internal(other.to_string()),
    }
}
