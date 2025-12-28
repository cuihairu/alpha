//! 腾讯财经数据源
//!
//! 提供腾讯财经的股票行情数据获取

use super::{
    CrawlerConfig, CrawlerError, CrawlerResult, DataSource, KlineData, KlineType, Market,
    RealtimeQuote, StockInfo,
};
use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use reqwest::Client;
use std::time::Duration;
use tokio::time::sleep;

/// 腾讯财经 API 基础 URL
const TENCENT_BASE_URL: &str = "https://qt.gtimg.cn";
const TENCENT_HISTORY_URL: &str = "https://web.ifzq.gtimg.cn";

/// 腾讯财经数据源
pub struct TencentSource {
    client: Client,
    config: CrawlerConfig,
}

impl TencentSource {
    /// 创建新的腾讯财经数据源
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
            ("Referer", "https://stockapp.finance.qq.com/"),
            ("Connection", "keep-alive"),
        ]
    }

    /// 格式化股票代码为腾讯格式 (sh600000)
    fn format_symbol(symbol: &str) -> String {
        let lower = symbol.to_lowercase();
        if lower.starts_with("sh") || lower.starts_with("sz") {
            return lower;
        }

        // 自动判断市场
        if let Some(market) = Market::from_symbol(symbol) {
            return format!("{}{}", market.prefix(), symbol);
        }

        symbol.to_string()
    }

    /// 解析实时行情响应
    /// 腾讯响应格式: v_sh600000="1~浦发银行~600000~9.88~..."
    fn parse_realtime_response(symbol: &str, response: &str) -> CrawlerResult<RealtimeQuote> {
        let data_start = response.find('"')
            .ok_or_else(|| CrawlerError::ParseError("No data found in response".to_string()))? + 1;
        let data_end = response.rfind('"')
            .ok_or_else(|| CrawlerError::ParseError("No end quote found".to_string()))?;

        let data_str = &response[data_start..data_end];
        let parts: Vec<&str> = data_str.split('~').collect();

        if parts.len() < 40 {
            return Err(CrawlerError::ParseError(format!(
                "Invalid data format, expected at least 40 fields, got {}",
                parts.len()
            )));
        }

        let name = parts[1].to_string();
        let price = parts[3].parse::<f64>()
            .map_err(|_| CrawlerError::ParseError("Invalid current price".to_string()))?;
        let pre_close = parts[4].parse::<f64>()
            .map_err(|_| CrawlerError::ParseError("Invalid pre-close price".to_string()))?;
        let open = parts[5].parse::<f64>()
            .map_err(|_| CrawlerError::ParseError("Invalid open price".to_string()))?;
        let volume = parts[6].parse::<f64>()
            .map_err(|_| CrawlerError::ParseError("Invalid volume".to_string()))? as u64;
        let ask1 = parts[9].parse::<f64>().ok();
        let bid1 = parts[10].parse::<f64>().ok();
        let high = parts[33].parse::<f64>()
            .map_err(|_| CrawlerError::ParseError("Invalid high price".to_string()))?;
        let low = parts[34].parse::<f64>()
            .map_err(|_| CrawlerError::ParseError("Invalid low price".to_string()))?;
        let amount = parts[37].parse::<f64>()
            .map_err(|_| CrawlerError::ParseError("Invalid amount".to_string()))?;
        let bid1_volume = parts[10].parse::<u64>().ok();
        let ask1_volume = parts[9].parse::<u64>().ok();

        let timestamp = Utc::now();

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
            source: "tencent".to_string(),
        };

        quote.calculate_change();
        Ok(quote)
    }

    /// 构建批量请求 URL
    fn build_batch_url(symbols: &[String]) -> String {
        let formatted: Vec<String> = symbols.iter()
            .map(|s| Self::format_symbol(s))
            .collect();

        let vars = formatted.iter()
            .map(|s| format!("v_{}", s))
            .collect::<Vec<_>>()
            .join(",");

        let symbols_str = formatted.join(",");
        format!("{}/q={}?env=cd&{}", TENCENT_BASE_URL, symbols_str, vars)
    }
}

#[async_trait]
impl DataSource for TencentSource {
    fn name(&self) -> &'static str {
        "tencent"
    }

    fn priority(&self) -> u8 {
        15 // 腾讯财经优先级中等
    }

    fn supports_batch(&self) -> bool {
        true // 腾讯支持批量请求
    }

    async fn get_realtime_quote(&self, symbol: &str) -> CrawlerResult<RealtimeQuote> {
        let formatted_symbol = Self::format_symbol(symbol);
        let url = format!("{}/q={}?env=cd&v_={}", TENCENT_BASE_URL, formatted_symbol, formatted_symbol);

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

        // 批量请求
        let url = Self::build_batch_url(symbols);

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
        let mut all_quotes = Vec::new();

        // 腾讯返回格式是多个变量赋值
        // 例如: v_sh600000="1~浦发银行~...";v_sz000001="1~平安银行~..."
        for part in text.split(';') {
            if let Some(start) = part.find("v_") {
                let var_start = start + 2;
                if let Some(equal_pos) = part.find('=') {
                    let symbol = &part[var_start..equal_pos];

                    if let Ok(quote) = Self::parse_realtime_response(symbol, part) {
                        all_quotes.push(quote);
                    }
                }
            }
        }

        // 添加延时避免请求过快
        sleep(Duration::from_millis(self.config.request_interval)).await;

        Ok(all_quotes)
    }

    async fn get_kline(
        &self,
        symbol: &str,
        kline_type: KlineType,
        limit: usize,
    ) -> CrawlerResult<Vec<KlineData>> {
        let formatted_symbol = Self::format_symbol(symbol);

        let kline_param = match kline_type {
            KlineType::Min1 => "min",
            KlineType::Min5 => "min5",
            KlineType::Min15 => "min15",
            KlineType::Min30 => "min30",
            KlineType::Min60 => "min60",
            KlineType::Day => "day",
            KlineType::Week => "week",
            KlineType::Month => "month",
        };

        // 腾讯历史数据 API
        let url = format!(
            "{}/appstock/app/fqkline/get?param={},{},{},{},qfq",
            TENCENT_HISTORY_URL, formatted_symbol, kline_param, 2000, "1970-01-01"
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
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(data) = json["data"][formatted_symbol.as_str()][kline_param].as_array() {
                let klines = data.iter()
                    .filter_map(|item| {
                        let arr = item.as_array()?;
                        if arr.len() < 6 {
                            return None;
                        }

                        // 格式: [日期, 开盘, 收盘, 最高, 最低, 成交量]
                        let date_str = arr[0].as_str()?;
                        let open = arr[1].as_f64()?;
                        let close = arr[2].as_f64()?;
                        let high = arr[3].as_f64()?;
                        let low = arr[4].as_f64()?;
                        let volume = arr[5].as_str()?.parse::<u64>().ok()?;

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
                            amount: 0.0,
                            change_percent,
                            change,
                            turnover_rate: None,
                        })
                    })
                    .rev() // 腾讯返回的是倒序的，需要反转
                    .take(limit)
                    .collect();

                return Ok(klines);
            }
        }

        Ok(Vec::new())
    }

    async fn get_stock_list(&self, _market: Option<Market>) -> CrawlerResult<Vec<StockInfo>> {
        // 腾讯没有直接的股票列表 API
        Ok(Vec::new())
    }

    async fn health_check(&self) -> CrawlerResult<bool> {
        let url = format!("{}/q=sh000001?env=cd", TENCENT_BASE_URL);

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
    async fn test_tencent_get_realtime_quote() {
        let source = TencentSource::new(CrawlerConfig::default());

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
    async fn test_tencent_get_realtime_quotes() {
        let source = TencentSource::new(CrawlerConfig::default());

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
    async fn test_tencent_health_check() {
        let source = TencentSource::new(CrawlerConfig::default());

        let healthy = source.health_check().await.unwrap_or(false);
        println!("Tencent health check: {}", healthy);
    }
}
