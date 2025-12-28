//! 网易财经数据源
//!
//! 提供网易财经的股票行情数据获取

use super::{
    CrawlerConfig, CrawlerError, CrawlerResult, DataSource, KlineData, KlineType, Market,
    RealtimeQuote, StockInfo,
};
use async_trait::async_trait;
use chrono::Utc;
use reqwest::Client;
use std::time::Duration;

/// 网易财经 API 基础 URL
const NETEASE_BASE_URL: &str = "https://api.money.126.net";
const NETEASE_DATA_URL: &str = "https://quotes.money.126.net";

/// 网易财经数据源
pub struct Netease163Source {
    client: Client,
    config: CrawlerConfig,
}

impl Netease163Source {
    /// 创建新的网易财经数据源
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
            ("Accept", "application/json, text/plain, */*"),
            ("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8"),
            ("Referer", "https://money.163.com/"),
            ("Connection", "keep-alive"),
        ]
    }

    /// 格式化股票代码为网易格式
    /// 网易使用 0(深圳) | 代码, 1(上海) | 代码
    fn format_symbol_for_api(symbol: &str) -> String {
        let (market_code, code) = if symbol.starts_with("60") || symbol.starts_with("68") || symbol.starts_with("51") {
            ("1", symbol) // 上海
        } else if symbol.starts_with("00") || symbol.starts_with("30") {
            ("0", symbol) // 深圳
        } else if symbol.starts_with("43") || symbol.starts_with("83") || symbol.starts_with("87") {
            ("2", symbol) // 北京
        } else {
            // 尝试从 lowercase 格式解析
            let lower = symbol.to_lowercase();
            if lower.starts_with("sh") {
                ("1", &symbol[2..])
            } else if lower.starts_with("sz") {
                ("0", &symbol[2..])
            } else if lower.starts_with("bj") {
                ("2", &symbol[2..])
            } else {
                ("0", symbol)
            }
        };

        format!("{}{}", market_code, code)
    }

    /// 格式化股票代码为推送接口格式 (0600000 for sh600000)
    #[allow(dead_code)]
    fn format_symbol_for_push(symbol: &str) -> String {
        let (prefix, code) = if symbol.starts_with("60") || symbol.starts_with("68") || symbol.starts_with("51") {
            ("1", symbol)
        } else if symbol.starts_with("00") || symbol.starts_with("30") {
            ("0", symbol)
        } else if symbol.starts_with("43") || symbol.starts_with("83") || symbol.starts_with("87") {
            ("2", symbol)
        } else {
            // 尝试从 lowercase 格式解析
            let lower = symbol.to_lowercase();
            if lower.starts_with("sh") {
                ("1", &symbol[2..])
            } else if lower.starts_with("sz") {
                ("0", &symbol[2..])
            } else if lower.starts_with("bj") {
                ("2", &symbol[2..])
            } else {
                ("0", symbol)
            }
        };

        format!("{}{}", prefix, code)
    }

    /// 解析实时行情 JSON 响应
    fn parse_quote_json(symbol: &str, data: &serde_json::Value) -> CrawlerResult<RealtimeQuote> {
        let name = data["name"].as_str().unwrap_or("").to_string();
        let price = data["price"].as_f64().unwrap_or(0.0);
        let pre_close = data["yestclose"].as_f64().unwrap_or(0.0);
        let open = data["open"].as_f64().unwrap_or(0.0);
        let high = data["high"].as_f64().unwrap_or(0.0);
        let low = data["low"].as_f64().unwrap_or(0.0);
        let volume = data["volume"].as_u64().unwrap_or(0);
        let amount = data["turnover"].as_f64().unwrap_or(0.0);

        let bid1 = data["bid1"].as_f64();
        let ask1 = data["ask1"].as_f64();
        let bid1_volume = data["bid1_volume"].as_u64();
        let ask1_volume = data["ask1_volume"].as_u64();

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
            source: "163".to_string(),
        };

        quote.calculate_change();
        Ok(quote)
    }
}

#[async_trait]
impl DataSource for Netease163Source {
    fn name(&self) -> &'static str {
        "163"
    }

    fn priority(&self) -> u8 {
        20 // 网易财经优先级较低
    }

    fn supports_batch(&self) -> bool {
        true // 网易支持批量请求
    }

    async fn get_realtime_quote(&self, symbol: &str) -> CrawlerResult<RealtimeQuote> {
        let api_symbol = Self::format_symbol_for_api(symbol);

        let url = format!(
            "{}/data/feed/{},money.api",
            NETEASE_DATA_URL, api_symbol
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

        // 移除可能的前缀
        let json_str = text.trim_start_matches("new_prod_").trim();

        if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
            if let Some(data) = json.get(&api_symbol) {
                return Self::parse_quote_json(symbol, data);
            }
        }

        Err(CrawlerError::ParseError("Invalid response format".to_string()))
    }

    async fn get_realtime_quotes(&self, symbols: &[String]) -> CrawlerResult<Vec<RealtimeQuote>> {
        if symbols.is_empty() {
            return Ok(Vec::new());
        }

        // 构建批量请求参数
        let api_symbols: Vec<String> = symbols.iter()
            .map(|s| Self::format_symbol_for_api(s))
            .collect();

        let symbols_param = api_symbols.join(",");

        let url = format!(
            "{}/data/feed/{},money.api",
            NETEASE_DATA_URL, symbols_param
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

        // 移除可能的前缀
        let json_str = text.trim_start_matches("new_prod_").trim();

        if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
            let mut quotes = Vec::new();

            for (i, symbol) in symbols.iter().enumerate() {
                if let Some(api_symbol) = api_symbols.get(i) {
                    if let Some(data) = json.get(api_symbol) {
                        if let Ok(quote) = Self::parse_quote_json(symbol, data) {
                            quotes.push(quote);
                        }
                    }
                }
            }

            return Ok(quotes);
        }

        Ok(Vec::new())
    }

    async fn get_kline(
        &self,
        symbol: &str,
        kline_type: KlineType,
        limit: usize,
    ) -> CrawlerResult<Vec<KlineData>> {
        let api_symbol = Self::format_symbol_for_api(symbol);

        // 网易历史数据接口可能需要调整
        // 这里提供一个简单的实现
        let url = format!(
            "{}/his/prices/{}.json?scale={}&unit=0&lo=0&hi={}&fields=TNO,SYMBOL,PRICE,OPEN,HIGH,LOW,VOLUME,TURNOVER,CHANGE,CHANGERATE,VOL_RATIO",
            NETEASE_BASE_URL, api_symbol, kline_type.as_str(), limit
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
            if let Some(data_array) = json["data"].as_array() {
                let klines = data_array.iter()
                    .filter_map(|item| {
                        // 网易返回格式可能是 JSON 数组，需要根据实际响应调整
                        let timestamp = item["TNO"].as_i64().unwrap_or(0);
                        let price = item["PRICE"].as_f64()?;
                        let open = item["OPEN"].as_f64()?;
                        let high = item["HIGH"].as_f64()?;
                        let low = item["LOW"].as_f64()?;
                        let volume = item["VOLUME"].as_u64()?;
                        let amount = item["TURNOVER"].as_f64().unwrap_or(0.0);
                        let change = item["CHANGE"].as_f64().unwrap_or(0.0);
                        let change_percent = item["CHANGERATE"].as_f64().unwrap_or(0.0);

                        Some(KlineData {
                            symbol: symbol.to_string(),
                            kline_type,
                            timestamp,
                            open,
                            high,
                            low,
                            close: price,
                            volume,
                            amount,
                            change_percent,
                            change,
                            turnover_rate: None,
                        })
                    })
                    .collect();

                return Ok(klines);
            }
        }

        Ok(Vec::new())
    }

    async fn get_stock_list(&self, _market: Option<Market>) -> CrawlerResult<Vec<StockInfo>> {
        // 网易没有直接的股票列表 API
        Ok(Vec::new())
    }

    async fn health_check(&self) -> CrawlerResult<bool> {
        let url = format!("{}/data/feed/1000001,money.api", NETEASE_DATA_URL);

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
    async fn test_netease_get_realtime_quote() {
        let source = Netease163Source::new(CrawlerConfig::default());

        match source.get_realtime_quote("000001").await {
            Ok(quote) => {
                println!("Quote: {:?}", quote);
                assert!(!quote.name.is_empty());
            }
            Err(e) => {
                eprintln!("Error: {:?}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_netease_get_realtime_quotes() {
        let source = Netease163Source::new(CrawlerConfig::default());

        let symbols = vec![
            "000001".to_string(),
            "600000".to_string(),
            "600519".to_string(),
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
    async fn test_netease_health_check() {
        let source = Netease163Source::new(CrawlerConfig::default());

        let healthy = source.health_check().await.unwrap_or(false);
        println!("Netease health check: {}", healthy);
    }
}
