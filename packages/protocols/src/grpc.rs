//! gRPC 协议定义

use alpha_core::models::*;
use serde::{Deserialize, Serialize};

// 股票查询请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockQueryRequest {
    pub symbols: Vec<String>,
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    pub limit: Option<u32>,
}

// 股票查询响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockQueryResponse {
    pub success: bool,
    pub data: Vec<MarketData>,
    pub total_count: u32,
    pub has_more: bool,
}

// 实时行情订阅请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteSubscribeRequest {
    pub symbols: Vec<String>,
    pub data_types: Vec<QuoteDataType>,
}

// 行情数据类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuoteDataType {
    Trade,
    Bid,
    Ask,
    Ohlcv,
}

// 技术指标计算请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndicatorRequest {
    pub symbol: String,
    pub indicators: Vec<IndicatorType>,
    pub period: Option<String>,
}

// 技术指标类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IndicatorType {
    RSI { period: u32 },
    SMA { period: u32 },
    EMA { period: u32 },
    MACD { fast: u32, slow: u32, signal: u32 },
    BollingerBands { period: u32, std_dev: f64 },
}

// 技术指标响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndicatorResponse {
    pub symbol: String,
    pub indicator: String,
    pub timestamps: Vec<i64>,
    pub values: Vec<f64>,
}
