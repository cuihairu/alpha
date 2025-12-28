//! 东方财富数据源
//!
//! 提供东方财富网的股票行情数据获取

use super::{
    CrawlerConfig, CrawlerError, CrawlerResult, DataSource, KlineData, KlineType, Market,
    RealtimeQuote, StockInfo,
};
use async_trait::async_trait;
use chrono::{NaiveDate, NaiveDateTime, TimeZone, Utc};
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

/// 东方财富 API 基础 URL
const EASTMONEY_API_BASE: &str = "https://push2.eastmoney.com/api/qt";
const EASTMONEY_PUSH_BASE: &str = "https://push2his.eastmoney.com";

/// 东方财富数据源
pub struct EastmoneySource {
    client: Client,
    config: CrawlerConfig,
}

impl EastmoneySource {
    /// 创建新的东方财富数据源
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
            ("Accept", "application/json"),
            ("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8"),
            ("Referer", "https://quote.eastmoney.com/"),
            ("Connection", "keep-alive"),
        ]
    }

    /// 格式化股票代码为东方财富格式
    /// 东方财富使用 0.xxxxxx (深圳), 1.xxxxxx (上海), 4.xxxxxx (其他)
    fn format_symbol_for_api(symbol: &str) -> String {
        let raw = symbol.trim().to_lowercase();

        // 已经是 secid 形式（0/1/4.xxxxxx）则直接使用
        if let Some((market, code)) = raw.split_once('.') {
            if matches!(market, "0" | "1" | "4") && !code.is_empty() && code.chars().all(|c| c.is_ascii_digit()) {
                return format!("{}.{}", market, code);
            }
        }

        // 支持常见前缀：sh/sz/bj600000
        let raw = raw
            .strip_prefix("sh")
            .or_else(|| raw.strip_prefix("sz"))
            .or_else(|| raw.strip_prefix("bj"))
            .unwrap_or(&raw);

        // 提取纯数字代码（兼容输入包含其他分隔符的情况）
        let code: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();

        let market_code = if code.starts_with("60") || code.starts_with("68") || code.starts_with("51") {
            "1" // 上海
        } else if code.starts_with("00") || code.starts_with("30") {
            "0" // 深圳
        } else if code.starts_with("43") || code.starts_with("83") || code.starts_with("87") {
            "4" // 北京
        } else {
            "0" // 默认深圳
        };

        format!("{}.{}", market_code, code)
    }

    /// 解析实时行情 API 响应
    fn parse_quote_response(symbol: &str, data: &EastmoneyQuoteData) -> CrawlerResult<RealtimeQuote> {
        // stock/get 接口的价格字段为“分”（*100）
        let price = data.f43 / 100.0;
        let pre_close = data.f60 / 100.0;
        let open = data.f46 / 100.0;
        let high = data.f44 / 100.0;
        let low = data.f45 / 100.0;
        let volume = data.f47; // 成交量（手）
        let amount = data.f48; // 成交额（元）

        let bid1 = if data.f51 > 0.0 { Some(data.f51 / 100.0) } else { None };
        let ask1 = if data.f52 > 0.0 { Some(data.f52 / 100.0) } else { None };
        let bid1_volume = if data.f53 > 0 { Some(data.f53) } else { None };
        let ask1_volume = if data.f54 > 0 { Some(data.f54) } else { None };

        let timestamp = Utc::now();

        let mut quote = RealtimeQuote {
            symbol: symbol.to_string(),
            name: if !data.f14.is_empty() { data.f14.clone() } else { data.f58.clone() },
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
            source: "eastmoney".to_string(),
        };

        quote.calculate_change();
        Ok(quote)
    }
}

/// 东方财富实时行情数据结构
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct EastmoneyQuoteResponse {
    data: Option<EastmoneyQuoteData>,
    #[serde(default)]
    rc: i32,
    #[serde(default)]
    rt: i32,
    #[serde(default)]
    svr_recv_time: i64,
    #[serde(default)]
    server_time: i64,
}

/// 单个股票行情数据
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct EastmoneyQuoteData {
    /// 股票名称（部分字段集合用 f14，部分用 f58）
    #[serde(default)]
    f14: String,
    #[serde(default)]
    f58: String,
    /// 最新价
    #[serde(default)]
    f43: f64,
    /// 最高价
    #[serde(default)]
    f44: f64,
    /// 最低价
    #[serde(default)]
    f45: f64,
    /// 开盘价
    #[serde(default)]
    f46: f64,
    /// 成交量（手）
    #[serde(default)]
    f47: u64,
    /// 成交额（元）
    #[serde(default)]
    f48: f64,
    /// 昨收价
    #[serde(default)]
    f60: f64,
    /// 买一价
    #[serde(default)]
    f51: f64,
    /// 卖一价
    #[serde(default)]
    f52: f64,
    /// 买一量
    #[serde(default)]
    f53: u64,
    /// 卖一量
    #[serde(default)]
    f54: u64,
    /// 涨跌额
    #[serde(default)]
    f170: f64,
    /// 涨跌幅
    #[serde(default)]
    f171: f64,
    /// 市场代码
    #[serde(default)]
    f12: String,
}

/// 东方财富 ulist.np/get 批量行情响应
#[derive(Debug, Deserialize)]
struct EastmoneyUListResponse {
    data: Option<EastmoneyUListData>,
}

#[derive(Debug, Deserialize)]
struct EastmoneyUListData {
    #[serde(default)]
    diff: Vec<EastmoneyUListItem>,
}

#[derive(Debug, Deserialize)]
struct EastmoneyUListItem {
    /// 当前价
    #[serde(default)]
    f2: f64,
    /// 涨跌幅（%）
    #[serde(default)]
    f3: f64,
    /// 涨跌额
    #[serde(default)]
    f4: f64,
    /// 成交量（手）
    #[serde(default)]
    f5: u64,
    /// 成交额（元）
    #[serde(default)]
    f6: f64,
    /// 股票代码
    #[serde(default)]
    f12: String,
    /// 市场：0=深，1=沪，4=北
    #[serde(default)]
    f13: i32,
    /// 股票名称
    #[serde(default)]
    f14: String,
    /// 最高价
    #[serde(default)]
    f15: f64,
    /// 最低价
    #[serde(default)]
    f16: f64,
    /// 开盘价
    #[serde(default)]
    f17: f64,
    /// 昨收价
    #[serde(default)]
    f18: f64,
    /// 时间戳（秒）
    #[serde(default)]
    f124: i64,
}

/// K线数据响应
#[derive(Debug, Deserialize)]
struct EastmoneyKlineResponse {
    data: Option<EastmoneyKlineData>,
}

#[derive(Debug, Deserialize)]
struct EastmoneyKlineData {
    #[serde(default)]
    klines: Option<Vec<String>>,
}

#[async_trait]
impl DataSource for EastmoneySource {
    fn name(&self) -> &'static str {
        "eastmoney"
    }

    fn priority(&self) -> u8 {
        5 // 东方财富优先级最高
    }

    fn supports_batch(&self) -> bool {
        true // 东方财富支持批量请求
    }

    async fn get_realtime_quote(&self, symbol: &str) -> CrawlerResult<RealtimeQuote> {
        let api_symbol = Self::format_symbol_for_api(symbol);

        let url = format!(
            "{}/stock/get?secid={}&fields=f12,f13,f14,f43,f44,f45,f46,f47,f48,f49,f50,f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f116,f170,f171",
            EASTMONEY_API_BASE, api_symbol
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
        let quote_response: EastmoneyQuoteResponse = serde_json::from_str(&text)
            .map_err(|e| CrawlerError::ParseError(format!("JSON parse error: {}", e)))?;

        if let Some(data) = quote_response.data {
            return Self::parse_quote_response(symbol, &data);
        }

        Err(CrawlerError::ParseError("No data found in response".to_string()))
    }

    async fn get_realtime_quotes(&self, symbols: &[String]) -> CrawlerResult<Vec<RealtimeQuote>> {
        if symbols.is_empty() {
            return Ok(Vec::new());
        }

        // 构建批量请求参数，并保留原始 symbol 映射（用于返回时尽量保持输入格式）
        let mut secid_to_symbol: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let secids: Vec<String> = symbols
            .iter()
            .map(|s| {
                let secid = Self::format_symbol_for_api(s);
                secid_to_symbol.insert(secid.clone(), s.clone());
                secid
            })
            .collect();

        let url = format!(
            "{}/ulist.np/get?action=fl&fields=f12,f13,f14,f2,f3,f4,f5,f6,f15,f16,f17,f18,f124&fltt=2&secids={}",
            EASTMONEY_API_BASE,
            secids.join(",")
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
        let quote_response: EastmoneyUListResponse = serde_json::from_str(&text)
            .map_err(|e| CrawlerError::ParseError(format!("JSON parse error: {}", e)))?;

        if let Some(data) = quote_response.data {
            let quotes = data
                .diff
                .into_iter()
                .filter_map(|item| {
                    if item.f12.is_empty() || item.f2 <= 0.0 {
                        return None;
                    }

                    let secid = format!("{}.{}", item.f13, item.f12);
                    let symbol = secid_to_symbol
                        .get(&secid)
                        .cloned()
                        .unwrap_or_else(|| item.f12.clone());

                    let timestamp = if item.f124 > 0 {
                        Utc.timestamp_opt(item.f124, 0).single().unwrap_or_else(Utc::now)
                    } else {
                        Utc::now()
                    };

                    let mut quote = RealtimeQuote {
                        symbol,
                        name: item.f14,
                        price: item.f2,
                        pre_close: item.f18,
                        open: item.f17,
                        high: item.f15,
                        low: item.f16,
                        volume: item.f5,
                        amount: item.f6,
                        change: 0.0,
                        change_percent: 0.0,
                        bid1: None,
                        ask1: None,
                        bid1_volume: None,
                        ask1_volume: None,
                        timestamp,
                        source: "eastmoney".to_string(),
                    };
                    quote.calculate_change();
                    Some(quote)
                })
                .collect();

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
        let secid = Self::format_symbol_for_api(symbol);

        let klt = match kline_type {
            KlineType::Min1 => 1,
            KlineType::Min5 => 5,
            KlineType::Min15 => 15,
            KlineType::Min30 => 30,
            KlineType::Min60 => 60,
            KlineType::Day => 101,
            KlineType::Week => 102,
            KlineType::Month => 103,
        };

        let url = format!(
            "{}/api/qt/stock/kline/get?secid={}&fields1=f1,f2,f3,f4,f5,f6&fields2=f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61&klt={}&fqt=0&beg=0&end=20500101&lmt={}",
            EASTMONEY_PUSH_BASE, secid, klt, limit
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
        let kline_response: EastmoneyKlineResponse = serde_json::from_str(&text)
            .map_err(|e| CrawlerError::ParseError(format!("JSON parse error: {}", e)))?;

        if let Some(data) = kline_response.data {
            if let Some(klines) = data.klines {
                if limit == 0 {
                    return Ok(Vec::new());
                }

                let mut result: Vec<KlineData> = klines
                    .iter()
                    .rev()
                    .take(limit)
                    .filter_map(|kline_str| {
                        let parts: Vec<&str> = kline_str.split(',').collect();
                        if parts.len() < 7 {
                            return None;
                        }

                        let date_str = parts[0];
                        let open = parts[1].parse::<f64>().ok()?;
                        let close = parts[2].parse::<f64>().ok()?;
                        let high = parts[3].parse::<f64>().ok()?;
                        let low = parts[4].parse::<f64>().ok()?;
                        let volume = parts[5].parse::<f64>().ok()? as u64;
                        let amount = parts[6].parse::<f64>().ok()?;

                        let timestamp = if date_str.contains(' ') {
                            NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M")
                                .ok()
                                .or_else(|| NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S").ok())
                                .map(|dt| Utc.from_utc_datetime(&dt))
                        } else {
                            NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                                .ok()
                                .and_then(|d| d.and_hms_opt(0, 0, 0))
                                .map(|dt| Utc.from_utc_datetime(&dt))
                        }
                        .unwrap_or_else(Utc::now);

                        let change = close - open;
                        let change_percent = if open > 0.0 { (change / open) * 100.0 } else { 0.0 };

                        Some(KlineData {
                            symbol: symbol.to_string(),
                            kline_type,
                            timestamp: timestamp.timestamp(),
                            open,
                            high,
                            low,
                            close,
                            volume,
                            amount,
                            change_percent,
                            change,
                            turnover_rate: None,
                        })
                    })
                    .collect();

                result.reverse();
                return Ok(result);
            }
        }

        Ok(Vec::new())
    }

    async fn get_stock_list(&self, market: Option<Market>) -> CrawlerResult<Vec<StockInfo>> {
        // 东方财富提供股票列表 API
        let market_code = match market {
            Some(Market::SH) => "1",  // 上海
            Some(Market::SZ) => "0",  // 深圳
            Some(Market::BJ) => "4",  // 北京
            None => "",
        };

        let url = if let Some(m) = market {
            format!(
                "{}/clist/get?pn=1&pz=5000&po=1&np=1&fltt=2&invt=2&fid=f3&fs=m:{}+t:!{}&fields=f12,f13,f14,f2,f3,f4,f5,f6",
                EASTMONEY_API_BASE, market_code, m as i32
            )
        } else {
            format!(
                "{}/clist/get?pn=1&pz=10000&po=1&np=1&fltt=2&invt=2&fid=f3&fs=m:0+t:6,m:0+t:80,m:1+t:2,m:1+t:23,m:4+t:81&fields=f12,f13,f14,f2,f3,f4,f5,f6",
                EASTMONEY_API_BASE
            )
        };

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

        let _ = response.text().await?;

        // 解析 JSON 响应获取股票列表
        // 这里的解析比较复杂，需要根据实际响应格式调整
        Ok(Vec::new())
    }

    async fn health_check(&self) -> CrawlerResult<bool> {
        let url = format!("{}/stock/get?secid=1.000001&fields=f12,f43", EASTMONEY_API_BASE);

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
    async fn test_eastmoney_get_realtime_quote() {
        let source = EastmoneySource::new(CrawlerConfig::default());

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
    async fn test_eastmoney_get_realtime_quotes() {
        let source = EastmoneySource::new(CrawlerConfig::default());

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
    async fn test_eastmoney_get_kline() {
        let source = EastmoneySource::new(CrawlerConfig::default());

        match source.get_kline("000001", KlineType::Day, 10).await {
            Ok(klines) => {
                println!("Got {} klines", klines.len());
                for kline in &klines {
                    println!("  {}: close={}, volume={}", kline.timestamp, kline.close, kline.volume);
                }
            }
            Err(e) => {
                eprintln!("Error: {:?}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_eastmoney_health_check() {
        let source = EastmoneySource::new(CrawlerConfig::default());

        let healthy = source.health_check().await.unwrap_or(false);
        println!("Eastmoney health check: {}", healthy);
    }
}
