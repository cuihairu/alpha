//! A 股数据源模块
//!
//! 提供统一的爬虫接口，支持多种数据源

pub mod mod_163;
pub mod mod_eastmoney;
pub mod mod_sina;
pub mod mod_tencent;
pub mod ua_rotation;

pub use mod_163::Netease163Source;
pub use mod_eastmoney::EastmoneySource;
pub use mod_sina::SinaSource;
pub use mod_tencent::TencentSource;
pub use ua_rotation::HeaderRotator;

use crate::types::{BackoffStrategy, MAX_BACKOFF_DELAY_MS};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{RequestBuilder, Response};
use serde::{Deserialize, Serialize};
use std::time::Duration;

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
    /// 重试间隔基准值（毫秒）；实际等待按 [`BackoffStrategy`] 退避
    pub retry_interval: u64,
    /// 请求级重试退避策略（默认带抖动指数退避）
    #[serde(default)]
    pub backoff_strategy: BackoffStrategy,
    /// User-Agent：None（默认）= 每请求从轮换池取（ua_rotation.rs，
    /// 抗封）；Some(x) = 定向伪装，恒用 x 不轮换
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
            backoff_strategy: BackoffStrategy::ExponentialWithJitter,
            user_agent: None,
            proxy: None,
        }
    }
}

/// 请求级重试（`CrawlerConfig.retry_times`/`retry_interval` 转正）：
/// 网络错误、5xx、429 按 `retry_times` 次数上限重试（含首次共
/// retry_times+1 次尝试），等待间隔走 `backoff_strategy` 退避（基准
/// `retry_interval`，封顶 [`MAX_BACKOFF_DELAY_MS`]）。其余 4xx 属请求
/// 本身问题，不重试直接失败；返回的成功响应必为 2xx（调用方无需再
/// 判状态）。health_check 探测面刻意不走此路径（快速单发语义）。
pub(crate) async fn send_with_retry(
    request: RequestBuilder,
    config: &CrawlerConfig,
) -> CrawlerResult<Response> {
    let total_attempts = config.retry_times.saturating_add(1);
    let mut last_err: Option<CrawlerError> = None;

    for attempt in 0..total_attempts {
        if attempt > 0 {
            let delay = config.backoff_strategy.delay_ms(
                config.retry_interval,
                attempt as u32,
                MAX_BACKOFF_DELAY_MS,
            );
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }

        // 每次尝试都经克隆发出（保留原件供下轮重试）；不可克隆的
        // 请求只能力争一次直发，失败即返（GET 请求恒可克隆，此为
        // 防御分支）
        let sendable = match request.try_clone() {
            Some(cloned) => cloned,
            None => return request.send().await.map_err(CrawlerError::from),
        };

        let result = sendable.send().await;

        match result {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    return Ok(resp);
                }
                if status.is_server_error() || status.as_u16() == 429 {
                    // 暂时性错误：记为最后一次错误后重试
                    last_err = Some(if status.as_u16() == 429 {
                        CrawlerError::RateLimited
                    } else {
                        CrawlerError::SourceError(format!("HTTP error: {status}"))
                    });
                    continue;
                }
                // 其余 4xx/3xx：请求本身有问题，不重试
                return Err(CrawlerError::SourceError(format!("HTTP error: {status}")));
            }
            Err(e) => {
                last_err = Some(CrawlerError::from(e));
            }
        }
    }

    Err(last_err.unwrap_or_else(|| CrawlerError::SourceError("请求未发出".to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// 极简假 HTTP 服务器：按脚本逐次返回状态码，记录收到的请求数
    fn spawn_scripted_server(script: Vec<u16>) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_clone = hits.clone();
        std::thread::spawn(move || {
            for status in script {
                if let Ok((mut stream, _)) = listener.accept() {
                    hits_clone.fetch_add(1, Ordering::SeqCst);
                    let mut buf = [0u8; 4096];
                    let _ = stream.read(&mut buf);
                    let resp = format!(
                        "HTTP/1.1 {status} T\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                    );
                    let _ = stream.write_all(resp.as_bytes());
                }
            }
        });
        (format!("http://{addr}/"), hits)
    }

    fn retry_config(retry_times: usize) -> CrawlerConfig {
        CrawlerConfig {
            retry_times,
            retry_interval: 1,
            ..CrawlerConfig::default()
        }
    }

    #[tokio::test]
    async fn retries_transient_5xx_then_succeeds() {
        let (url, hits) = spawn_scripted_server(vec![500, 200]);
        let config = retry_config(3);
        let request = reqwest::Client::new().get(&url);
        let resp = send_with_retry(request, &config).await.unwrap();
        assert!(resp.status().is_success());
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn client_error_returns_without_retry() {
        let (url, hits) = spawn_scripted_server(vec![404]);
        let config = retry_config(3);
        let request = reqwest::Client::new().get(&url);
        let err = send_with_retry(request, &config).await.unwrap_err();
        assert!(matches!(err, CrawlerError::SourceError(_)));
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn exhausts_retries_and_reports_last_error() {
        let (url, hits) = spawn_scripted_server(vec![503, 503, 503]);
        let config = retry_config(2);
        let request = reqwest::Client::new().get(&url);
        let err = send_with_retry(request, &config).await.unwrap_err();
        assert!(matches!(err, CrawlerError::SourceError(_)));
        assert_eq!(hits.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn rate_limited_maps_to_rate_limited_error() {
        let (url, _) = spawn_scripted_server(vec![429]);
        let config = retry_config(0);
        let request = reqwest::Client::new().get(&url);
        let err = send_with_retry(request, &config).await.unwrap_err();
        assert!(matches!(err, CrawlerError::RateLimited));
    }

    #[tokio::test]
    async fn connection_refused_retries_then_errors() {
        let config = retry_config(1);
        let request = reqwest::Client::new().get("http://127.0.0.1:1/");
        let err = send_with_retry(request, &config).await.unwrap_err();
        assert!(matches!(err, CrawlerError::RequestError(_)));
    }
}
