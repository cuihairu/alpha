//! 新浪财经数据源
//!
//! 提供新浪财经的股票行情数据获取

use super::{
    CrawlerConfig, CrawlerError, CrawlerResult, DataSource, KlineData, KlineType, Market,
    RealtimeQuote, StockInfo,
};
use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use reqwest::Client;
use std::time::Duration;
use tokio::time::sleep;

/// 新浪财经 API 基础 URL
const SINA_BASE_URL: &str = "https://hq.sinajs.cn";
const SINA_HISTORY_URL: &str = "https://money.finance.sina.com.cn/quotes_service/api/json_v2.php";

/// 新浪财经数据源
pub struct SinaSource {
    client: Client,
    config: CrawlerConfig,
}

impl SinaSource {
    /// 创建新的新浪财经数据源
    pub fn new(config: CrawlerConfig) -> Self {
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(config.timeout));

        if let Some(ref proxy) = config.proxy {
            let proxy_url = if let (Some(ref user), Some(ref pass)) = (&proxy.username, &proxy.password) {
                format!("{}:{}@{}", user, pass, proxy.url)
            } else {
                proxy.url.clone()
            };
            if let Ok(proxy) = reqwest::Proxy::all(&proxy_url) {
                builder = builder.proxy(proxy);
            }
        }

        let client = builder.build().unwrap_or_else(|_| Client::new());

        Self { client, config }
    }

    /// 构建请求头
    fn build_headers() -> Vec<(&'static str, &'static str)> {
        vec![
            ("Accept", "*/*"),
            ("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8"),
            ("Referer", "https://finance.sina.com.cn/"),
            ("Connection", "keep-alive"),
        ]
    }

    /// 解析实时行情响应
    /// 响应格式: var hq_str_sh600000="浦发银行,9.88,9.89,9.90,9.91,9.87,9.90,9.91,37143368,368076074.00,..."
    fn parse_realtime_response(symbol: &str, response: &str) -> CrawlerResult<RealtimeQuote> {
        let data_start = response.find('"')
            .ok_or_else(|| CrawlerError::ParseError("No data found in response".to_string()))? + 1;
        let data_end = response.rfind('"')
            .ok_or_else(|| CrawlerError::ParseError("No end quote found".to_string()))?;

        let data_str = &response[data_start..data_end];
        let parts: Vec<&str> = data_str.split(',').collect();

        if parts.len() < 32 {
            return Err(CrawlerError::ParseError(format!(
                "Invalid data format, expected at least 32 fields, got {}",
                parts.len()
            )));
        }

        let name = parts[0].to_string();
        let open = parts[1].parse::<f64>()
            .map_err(|_| CrawlerError::ParseError("Invalid open price".to_string()))?;
        let pre_close = parts[2].parse::<f64>()
            .map_err(|_| CrawlerError::ParseError("Invalid pre-close price".to_string()))?;
        let price = parts[3].parse::<f64>()
            .map_err(|_| CrawlerError::ParseError("Invalid current price".to_string()))?;
        let high = parts[4].parse::<f64>()
            .map_err(|_| CrawlerError::ParseError("Invalid high price".to_string()))?;
        let low = parts[5].parse::<f64>()
            .map_err(|_| CrawlerError::ParseError("Invalid low price".to_string()))?;
        let bid1 = parts[6].parse::<f64>().ok();
        let ask1 = parts[7].parse::<f64>().ok();
        let volume = parts[8].parse::<f64>()
            .map_err(|_| CrawlerError::ParseError("Invalid volume".to_string()))? as u64;
        let amount = parts[9].parse::<f64>()
            .map_err(|_| CrawlerError::ParseError("Invalid amount".to_string()))?;

        let bid1_volume = if !parts[10].is_empty() {
            parts[10].parse::<u64>().ok()
        } else {
            None
        };
        let ask1_volume = if !parts[11].is_empty() {
            parts[11].parse::<u64>().ok()
        } else {
            None
        };

        // 获取日期和时间
        let date_str = parts[30];
        let time_str = parts[31];

        let timestamp = if !date_str.is_empty() && !time_str.is_empty() {
            Self::parse_datetime(date_str, time_str)
        } else {
            Utc::now()
        };

        let mut quote = RealtimeQuote {
            symbol: symbol.to_string(),
            name,
            price,
            pre_close,
            open,
            high,
            low,
            volume,
            amount,
            change: 0.0,
            change_percent: 0.0,
            bid1,
            ask1,
            bid1_volume,
            ask1_volume,
            timestamp,
            source: "sina".to_string(),
        };

        quote.calculate_change();
        Ok(quote)
    }

    /// 解析日期时间字符串
    fn parse_datetime(_date_str: &str, _time_str: &str) -> DateTime<Utc> {
        // 简单处理：返回当前时间
        // 实际项目中应该解析日期时间字符串
        Utc::now()
    }

    /// 格式化股票代码为新浪格式
    fn format_symbol(symbol: &str) -> String {
        let lower = symbol.to_lowercase();
        if lower.starts_with("sh") || lower.starts_with("sz") || lower.starts_with("bj") {
            return lower;
        }

        // 自动判断市场
        if let Some(market) = Market::from_symbol(symbol) {
            return format!("{}{}", market.prefix(), symbol);
        }

        symbol.to_string()
    }

    /// 批量获取行情的URL
    fn build_batch_url(symbols: &[String]) -> String {
        let formatted: Vec<String> = symbols.iter()
            .map(|s| Self::format_symbol(s))
            .collect();

        let list = formatted.join(",");
        format!("{}/list={}", SINA_BASE_URL, list)
    }
}

#[async_trait]
impl DataSource for SinaSource {
    fn name(&self) -> &'static str {
        "sina"
    }

    fn priority(&self) -> u8 {
        10 // 新浪财经优先级较高
    }

    fn supports_batch(&self) -> bool {
        true // 新浪支持批量请求
    }

    async fn get_realtime_quote(&self, symbol: &str) -> CrawlerResult<RealtimeQuote> {
        let formatted_symbol = Self::format_symbol(symbol);
        let url = format!("{}/list={}", SINA_BASE_URL, formatted_symbol);

        let mut request = self.client.get(&url);
        for (key, value) in Self::build_headers() {
            request = request.header(key, value);
        }
        if let Some(ref ua) = self.config.user_agent {
            request = request.header("User-Agent", ua);
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            return Err(CrawlerError::SourceError(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        let text = response.text().await?;

        // 添加延时避免请求过快
        sleep(Duration::from_millis(self.config.request_interval)).await;

        Self::parse_realtime_response(symbol, &text)
    }

    async fn get_realtime_quotes(&self, symbols: &[String]) -> CrawlerResult<Vec<RealtimeQuote>> {
        if symbols.is_empty() {
            return Ok(Vec::new());
        }

        // 批量请求最多支持 200 个
        const BATCH_SIZE: usize = 200;
        let mut all_quotes = Vec::new();

        for chunk in symbols.chunks(BATCH_SIZE) {
            let url = Self::build_batch_url(chunk);

            let mut request = self.client.get(&url);
            for (key, value) in Self::build_headers() {
                request = request.header(key, value);
            }
            if let Some(ref ua) = self.config.user_agent {
                request = request.header("User-Agent", ua);
            }

            let response = request.send().await?;

            if !response.status().is_success() {
                return Err(CrawlerError::SourceError(format!(
                    "HTTP error: {}",
                    response.status()
                )));
            }

            let text = response.text().await?;

            // 解析每个股票的响应
            // 响应中每行包含一个股票的数据
            for line in text.lines() {
                if let Some(start) = line.find("hq_str_") {
                    let symbol_start = start + 7; // "hq_str_".len()
                    let symbol_end = line[symbol_start..]
                        .find('=')
                        .map(|pos| symbol_start + pos)
                        .unwrap_or(line.len());

                    let symbol = &line[symbol_start..symbol_end];

                    if let Ok(quote) = Self::parse_realtime_response(symbol, line) {
                        all_quotes.push(quote);
                    }
                }
            }

            // 添加延时避免请求过快
            sleep(Duration::from_millis(self.config.request_interval)).await;
        }

        Ok(all_quotes)
    }

    async fn get_kline(
        &self,
        symbol: &str,
        kline_type: KlineType,
        limit: usize,
    ) -> CrawlerResult<Vec<KlineData>> {
        let formatted_symbol = Self::format_symbol(symbol);
        let scale = match kline_type {
            KlineType::Min1 => "1min",
            KlineType::Min5 => "5min",
            KlineType::Min15 => "15min",
            KlineType::Min30 => "30min",
            KlineType::Min60 => "60min",
            KlineType::Day => "daily",
            KlineType::Week => "weekly",
            KlineType::Month => "monthly",
        };

        // 新浪历史数据 API
        let url = format!(
            "{}?symbol={}&scale={}&ma=no&datalen={}",
            SINA_HISTORY_URL, formatted_symbol, scale, limit
        );

        let mut request = self.client.get(&url);
        for (key, value) in Self::build_headers() {
            request = request.header(key, value);
        }
        if let Some(ref ua) = self.config.user_agent {
            request = request.header("User-Agent", ua);
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            return Err(CrawlerError::SourceError(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        let text = response.text().await?;

        // 解析 JSON 响应
        // 新浪返回的是一种特殊格式的 JSON，需要解析
        if let Ok(data) = serde_json::from_str::<Vec<serde_json::Value>>(&text) {
            let klines = data.iter().filter_map(|item| {
                let arr = item.as_array()?;
                if arr.len() < 6 {
                    return None;
                }

                let date_str = arr[0].as_str()?;
                let open = arr[1].as_f64()?;
                let high = arr[2].as_f64()?;
                let low = arr[3].as_f64()?;
                let close = arr[4].as_f64()?;
                let volume = arr[5].as_f64()? as u64;

                // 解析日期（无时间信息时按 00:00:00 处理）
                let timestamp = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                    .ok()
                    .and_then(|date| date.and_hms_opt(0, 0, 0))
                    .map(|dt| Utc.from_utc_datetime(&dt))
                    .unwrap_or_else(Utc::now);

                let change = close - open;
                let change_percent = if open > 0.0 {
                    (change / open) * 100.0
                } else {
                    0.0
                };

                Some(KlineData {
                    symbol: symbol.to_string(),
                    kline_type,
                    timestamp: timestamp.timestamp(),
                    open,
                    high,
                    low,
                    close,
                    volume,
                    amount: 0.0, // 新浪 API 可能不返回成交额
                    change_percent,
                    change,
                    turnover_rate: None,
                })
            }).collect();

            Ok(klines)
        } else {
            // 返回空数据而不是错误
            Ok(Vec::new())
        }
    }

    async fn get_stock_list(&self, _market: Option<Market>) -> CrawlerResult<Vec<StockInfo>> {
        // 新浪没有直接的股票列表 API
        // 可以通过其他方式获取，这里暂时返回空
        // 实际项目中可以从本地文件或数据库加载

        Ok(Vec::new())
    }

    async fn health_check(&self) -> CrawlerResult<bool> {
        // 尝试获取一个大盘指数
        let url = format!("{}/list=sh000001", SINA_BASE_URL);

        let response = self.client.get(&url)
            .timeout(Duration::from_secs(5))
            .send()
            .await?;

        Ok(response.status().is_success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sina_get_realtime_quote() {
        let source = SinaSource::new(CrawlerConfig::default());

        match source.get_realtime_quote("sh600000").await {
            Ok(quote) => {
                println!("Quote: {:?}", quote);
                assert_eq!(quote.symbol, "sh600000");
                assert!(!quote.name.is_empty());
            }
            Err(e) => {
                eprintln!("Error: {:?}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_sina_get_realtime_quotes() {
        let source = SinaSource::new(CrawlerConfig::default());

        let symbols = vec![
            "sh600000".to_string(),
            "sz000001".to_string(),
            "sh600519".to_string(),
        ];

        match source.get_realtime_quotes(&symbols).await {
            Ok(quotes) => {
                println!("Got {} quotes", quotes.len());
                for quote in &quotes {
                    println!("  {}: {} - {}", quote.symbol, quote.name, quote.price);
                }
            }
            Err(e) => {
                eprintln!("Error: {:?}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_sina_health_check() {
        let source = SinaSource::new(CrawlerConfig::default());

        let healthy = source.health_check().await.unwrap_or(false);
        println!("Sina health check: {}", healthy);
    }
}
