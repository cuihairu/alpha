//! 跨平台数据模型定义

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 市场数据基础结构
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarketData {
    /// 股票代码
    pub symbol: String,
    /// 时间戳
    pub timestamp: DateTime<Utc>,
    /// 价格
    pub price: f64,
    /// 成交量
    pub volume: u64,
    /// 买价
    pub bid: Option<f64>,
    /// 卖价
    pub ask: Option<f64>,
    /// 开盘价
    pub open: Option<f64>,
    /// 最高价
    pub high: Option<f64>,
    /// 最低价
    pub low: Option<f64>,
}

impl MarketData {
    pub fn new(symbol: String, price: f64, volume: u64) -> Self {
        Self {
            symbol,
            timestamp: Utc::now(),
            price,
            volume,
            bid: None,
            ask: None,
            open: None,
            high: None,
            low: None,
        }
    }

    pub fn with_ohlcv(
        symbol: String,
        timestamp: DateTime<Utc>,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
        volume: u64,
    ) -> Self {
        Self {
            symbol,
            timestamp,
            price: close,
            volume,
            bid: None,
            ask: None,
            open: Some(open),
            high: Some(high),
            low: Some(low),
        }
    }
}

/// 技术指标结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndicatorResult {
    /// 指标名称
    pub name: String,
    /// 时间序列
    pub timestamps: Vec<DateTime<Utc>>,
    /// 指标值
    pub values: Vec<f64>,
    /// 信号序列
    pub signals: Vec<SignalType>,
}

/// 交易信号类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SignalType {
    /// 买入信号
    Buy,
    /// 卖出信号
    Sell,
    /// 持有信号
    Hold,
    /// 无信号
    None,
}

/// 分析结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    /// 股票代码
    pub symbol: String,
    /// 分析时间
    pub analyzed_at: DateTime<Utc>,
    /// 技术指标结果
    pub indicators: Vec<IndicatorResult>,
    /// 风险评估
    pub risk_metrics: RiskMetrics,
    /// 推荐信号
    pub recommendation: SignalType,
    /// 置信度
    pub confidence: f64,
}

/// 风险指标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskMetrics {
    /// 波动率
    pub volatility: f64,
    /// 夏普比率
    pub sharpe_ratio: Option<f64>,
    /// 最大回撤
    pub max_drawdown: f64,
    /// Beta 系数
    pub beta: Option<f64>,
}

/// 交易策略定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingStrategy {
    /// 策略 ID
    pub id: Uuid,
    /// 策略名称
    pub name: String,
    /// 策略描述
    pub description: String,
    /// 策略参数
    pub parameters: StrategyParameters,
    /// 使用的指标列表
    pub indicators: Vec<String>,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 更新时间
    pub updated_at: DateTime<Utc>,
}

/// 策略参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyParameters {
    /// 参数映射
    pub params: std::collections::HashMap<String, f64>,
    /// 参数描述
    pub descriptions: std::collections::HashMap<String, String>,
}

impl StrategyParameters {
    pub fn new() -> Self {
        Self {
            params: std::collections::HashMap::new(),
            descriptions: std::collections::HashMap::new(),
        }
    }

    pub fn set_param(&mut self, key: String, value: f64, description: String) {
        self.params.insert(key.clone(), value);
        self.descriptions.insert(key, description);
    }

    pub fn get_param(&self, key: &str) -> Option<f64> {
        self.params.get(key).copied()
    }
}

/// 时间区间
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    /// 开始时间
    pub start: DateTime<Utc>,
    /// 结束时间
    pub end: DateTime<Utc>,
}

impl TimeRange {
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self { start, end }
    }

    pub fn duration(&self) -> chrono::Duration {
        self.end - self.start
    }
}

/// 数据源类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DataSource {
    /// 实时数据源
    Realtime,
    /// 历史数据源
    Historical,
    /// 缓存数据源
    Cache,
    /// 文件数据源
    File,
    /// API 数据源
    API(String),
}