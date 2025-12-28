//! A 股数据源模块
//!
//! 提供统一的爬虫接口，支持多种数据源

pub mod mod_sina;
pub mod mod_eastmoney;
pub mod mod_tencent;
pub mod mod_163;

pub use mod_sina::SinaSource;
pub use mod_eastmoney::EastmoneySource;
pub use mod_tencent::TencentSource;
pub use mod_163::Netease163Source;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A 股市场类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Market {
    /// 上海证券交易所
    SH,
    /// 深圳证券交易所
    SZ,
    /// 北京证券交易所
    BJ,
}

impl Market {
    pub fn from_symbol(symbol: &str) -> Option<Self> {
        if symbol.starts_with('6') || symbol.starts_with("900") || symbol.starts_with("688") {
            Some(Market::SH)
        } else if symbol.starts_with('0') || symbol.starts_with('3') || symbol.starts_with("300") {
            Some(Market::SZ)
        } else if symbol.starts_with('8') || symbol.starts_with('4') {
            Some(Market::BJ)
        } else {
            None
        }
    }

    pub fn prefix(&self) -> &'static str {
        match self {
            Market::SH => "sh",
            Market::SZ => "sz",
            Market::BJ => "bj",
        }
    }

    pub fn full_code(&self, code: &str) -> String {
        format!("{}{}", self.prefix(), code)
    }
}

/// 股票实时行情数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealtimeQuote {
    /// 股票代码（如：sh600000）
    pub symbol: String,
    /// 股票名称
    pub name: String,
    /// 当前价格
    pub price: f64,
    /// 昨收价
    pub pre_close: f64,
    /// 开盘价
    pub open: f64,
    /// 最高价
    pub high: f64,
    /// 最低价
    pub low: f64,
    /// 成交量（手）
    pub volume: u64,
    /// 成交额（元）
    pub amount: f64,
    /// 涨跌额
    pub change: f64,
    /// 涨跌幅（%）
    pub change_percent: f64,
    /// 买一价
    pub bid1: Option<f64>,
    /// 卖一价
    pub ask1: Option<f64>,
    /// 买一量（手）
    pub bid1_volume: Option<u64>,
    /// 卖一量（手）
    pub ask1_volume: Option<u64>,
    /// 时间戳
    pub timestamp: DateTime<Utc>,
    /// 数据源
    pub source: String,
}

impl RealtimeQuote {
    /// 计算涨跌额和涨跌幅
    pub fn calculate_change(&mut self) {
        self.change = self.price - self.pre_close;
        if self.pre_close > 0.0 {
            self.change_percent = (self.change / self.pre_close) * 100.0;
        } else {
            self.change_percent = 0.0;
        }
    }

    /// 是否涨停
    pub fn is_limit_up(&self) -> bool {
        const LIMIT_UP_THRESHOLD: f64 = 9.9; // 考虑浮点误差
        self.change_percent >= LIMIT_UP_THRESHOLD
    }

    /// 是否跌停
    pub fn is_limit_down(&self) -> bool {
        const LIMIT_DOWN_THRESHOLD: f64 = -9.9;
        self.change_percent <= LIMIT_DOWN_THRESHOLD
    }
}

/// K线数据类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KlineType {
    /// 1分钟
    Min1,
    /// 5分钟
    Min5,
    /// 15分钟
    Min15,
    /// 30分钟
    Min30,
    /// 60分钟
    Min60,
    /// 日K
    Day,
    /// 周K
    Week,
    /// 月K
    Month,
}

impl KlineType {
    pub fn as_str(&self) -> &'static str {
        match self {
            KlineType::Min1 => "1min",
            KlineType::Min5 => "5min",
            KlineType::Min15 => "15min",
            KlineType::Min30 => "30min",
            KlineType::Min60 => "60min",
            KlineType::Day => "day",
            KlineType::Week => "week",
            KlineType::Month => "month",
        }
    }

    pub fn minutes(&self) -> Option<u32> {
        match self {
            KlineType::Min1 => Some(1),
            KlineType::Min5 => Some(5),
            KlineType::Min15 => Some(15),
            KlineType::Min30 => Some(30),
            KlineType::Min60 => Some(60),
            KlineType::Day => Some(1440),
            KlineType::Week => Some(10080),
            KlineType::Month => None,
        }
    }
}

/// K线数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KlineData {
    /// 股票代码
    pub symbol: String,
    /// K线类型
    pub kline_type: KlineType,
    /// 时间戳
    pub timestamp: i64,
    /// 开盘价
    pub open: f64,
    /// 最高价
    pub high: f64,
    /// 最低价
    pub low: f64,
    /// 收盘价
    pub close: f64,
    /// 成交量（手）
    pub volume: u64,
    /// 成交额（元）
    pub amount: f64,
    /// 涨跌幅
    pub change_percent: f64,
    /// 涨跌额
    pub change: f64,
    /// 换手率
    pub turnover_rate: Option<f64>,
}

/// 股票列表信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockInfo {
    /// 股票代码
    pub symbol: String,
    /// 股票名称
    pub name: String,
    /// 所属市场
    pub market: Market,
    /// 行业
    pub industry: Option<String>,
    /// 股票类型（股票/指数）
    pub stock_type: StockType,
    /// 上市日期
    pub list_date: Option<String>,
    /// 状态
    pub status: StockStatus,
}

/// 股票类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StockType {
    /// 普通股票
    Stock,
    /// 指数
    Index,
    /// ETF
    Etf,
    /// LOF
    Lof,
}

/// 股票状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StockStatus {
    /// 正常交易
    Normal,
    /// 停牌
    Suspended,
    /// 退市
    Delisted,
    /// ST
    ST,
    /// *ST
    StarST,
}

/// 爬虫错误类型
#[derive(Debug, thiserror::Error)]
pub enum CrawlerError {
    #[error("HTTP request failed: {0}")]
    RequestError(#[from] reqwest::Error),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Data source error: {0}")]
    SourceError(String),

    #[error("Rate limited")]
    RateLimited,

    #[error("Timeout")]
    Timeout,

    #[error("Invalid data: {0}")]
    InvalidData(String),
}

/// 爬虫结果
pub type CrawlerResult<T> = Result<T, CrawlerError>;

/// 数据源 trait
#[async_trait]
pub trait DataSource: Send + Sync {
    /// 获取数据源名称
    fn name(&self) -> &'static str;

    /// 获取单个股票实时行情
    async fn get_realtime_quote(&self, symbol: &str) -> CrawlerResult<RealtimeQuote>;

    /// 批量获取股票实时行情
    async fn get_realtime_quotes(&self, symbols: &[String]) -> CrawlerResult<Vec<RealtimeQuote>>;

    /// 获取 K线数据
    async fn get_kline(
        &self,
        symbol: &str,
        kline_type: KlineType,
        limit: usize,
    ) -> CrawlerResult<Vec<KlineData>>;

    /// 获取股票列表
    async fn get_stock_list(&self, market: Option<Market>) -> CrawlerResult<Vec<StockInfo>>;

    /// 健康检查
    async fn health_check(&self) -> CrawlerResult<bool>;

    /// 获取数据源优先级（数字越小优先级越高）
    fn priority(&self) -> u8 {
        100
    }

    /// 是否支持并发请求
    fn supports_batch(&self) -> bool {
        false
    }
}

/// 代理配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// 代理地址
    pub url: String,
    /// 用户名
    pub username: Option<String>,
    /// 密码
    pub password: Option<String>,
}

/// 爬虫配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlerConfig {
    /// 请求超时时间（秒）
    pub timeout: u64,
    /// 并发请求数
    pub max_concurrent: usize,
    /// 请求间隔（毫秒）
    pub request_interval: u64,
    /// 重试次数
    pub retry_times: usize,
    /// 重试间隔（毫秒）
    pub retry_interval: u64,
    /// User-Agent
    pub user_agent: Option<String>,
    /// 代理配置
    pub proxy: Option<ProxyConfig>,
}

impl Default for CrawlerConfig {
    fn default() -> Self {
        Self {
            timeout: 30,
            max_concurrent: 10,
            request_interval: 100,
            retry_times: 3,
            retry_interval: 1000,
            user_agent: Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36".to_string()),
            proxy: None,
        }
    }
}
