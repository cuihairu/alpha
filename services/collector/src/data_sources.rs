//! 扩展数据源模块
//!
//! 支持更多金融数据获取平台的多语言爬虫系统
//! 包括数字货币、外汇、大宗商品、债券、基金、经济指标等

use std::collections::HashMap;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use super::CrawlerConfig;

/// 数字货币交易所配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryptoExchangeConfig {
    pub exchange: CryptoExchange,
    pub api_key: String,
    pub base_url: String,
    pub rate_limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CryptoExchange {
    Binance,
    Coinbase,
    Kraken,
    Bitfinex,
    Huobi,
    OKX,
    Bybit,
    Gate,
}

/// 数字货币数据源配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryptoSourceConfig {
    pub api_key: String,
    pub symbols: Vec<String>,
    pub exchanges: Vec<String>,
}

/// 外汇数据源配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForexSourceConfig {
    pub source: ForexDataSource,
    pub api_key: String,
    pub base_url: String,
    pub supported_pairs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ForexDataSource {
    Oanda,
    FXCM,
    ForexCom,
    DailyFX,
    MetaTrader,
}

/// 大宗商品数据源配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommoditySourceConfig {
    pub category: CommodityCategory,
    pub api_key: String,
    pub exchanges: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommodityCategory {
    PreciousMetals,
    Energy,
    Agriculture,
    IndustrialMetals,
    SoftCommodities,
}

/// 债券数据源配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondSourceConfig {
    pub market: BondMarket,
    pub api_key: String,
    pub data_sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BondMarket {
    ChinaGovernment,
    USGovernment,
    Corporate,
    Municipal,
    International,
}

/// 基金数据源配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundSourceConfig {
    pub market: FundMarket,
    pub api_key: String,
    pub fund_types: Vec<FundType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FundMarket {
    ChinaMutual,
    USMutual,
    ETF,
    HedgeFund,
    IndexFund,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FundType {
    Stock,
    Bond,
    MoneyMarket,
    Commodity,
    Mixed,
}

/// ESG数据源配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ESGSourceConfig {
    pub provider: ESGProvider,
    pub api_key: String,
    pub metrics: Vec<ESGMetric>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ESGProvider {
    MSCI,
    Sustainalytics,
    Refinitiv,
    BloombergESG,
    ChinaESG,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ESGMetric {
    Environmental,
    Social,
    Governance,
    ESGScore,
    CarbonEmissions,
    Sustainability,
}

/// 研报数据源配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchSourceConfig {
    pub provider: ResearchProvider,
    pub api_key: String,
    pub report_types: Vec<ResearchType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResearchProvider {
    CICC,
    Haitong,
    Shenwan,
    Huatai,
    Morningstar,
    SSGlobal,
    MorganStanley,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResearchType {
    CompanyReport,
    IndustryAnalysis,
    MacroResearch,
    StrategyReport,
    QuantitativeAnalysis,
    ValuationAnalysis,
    RiskAssessment,
}

/// 扩展数据源采集器
pub struct ExtendedDataCollector {
    workspace_root: std::path::PathBuf,
    temp_dir: std::path::PathBuf,
}

impl ExtendedDataCollector {
    pub fn new<P: AsRef<std::path::Path>>(workspace_root: P) -> Self {
        let workspace_root = workspace_root.as_ref().to_path_buf();
        let temp_dir = workspace_root.join("temp");

        Self {
            workspace_root,
            temp_dir,
        }
    }

    /// 生成数字货币数据采集脚本
    pub fn generate_crypto_script(
        &self,
        symbols: &[String],
        config: &CryptoSourceConfig,
    ) -> Result<String> {
        let symbols_json = serde_json::to_string(symbols)?;

        let script = format!(r#"
import requests
import json
import time
import hmac
import hashlib
from datetime import datetime

class {exchange_name}DataCollector:
    def __init__(self, api_key, symbols):
        self.api_key = api_key
        self.symbols = symbols
        self.base_url = "https://api.binance.com"
        self.headers = {{
            'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36'
        }}
        self.rate_limit = {rate_limit}

    def _generate_signature(self, params):
        """生成HMAC-SHA256签名"""
        query_string = '&'.join([f"{{k}}={{v}}" for k, v in sorted(params.items())])
        signature = hmac.new(
            self.api_key.encode('utf-8'),
            query_string.encode('utf-8'),
            hashlib.sha256
        ).hexdigest()
        return f"{{signature}}"

    def get_ticker_data(self, symbol):
        """获取单个币种数据"""
        try:
            endpoint = format!("{}/api/v3/ticker/price", self.base_url)
            params = {{
                'symbol': symbol.upper(),
            }}

            response = requests.get(endpoint, params=params, headers=self.headers, timeout=30)
            response.raise_for_status()

            data = response.json()

            if data and 'symbol' in data:
                ticker_data = data['symbol']
                result = {{
                    'symbol': symbol.upper(),
                    'price': float(ticker_data.get('lastPrice', 0)),
                    'price_change': float(ticker_data.get('priceChange', 0)),
                    'price_change_percent': float(ticker_data.get('priceChangePercent', 0)),
                    'high_price': float(ticker_data.get('highPrice', 0)),
                    'low_price': float(ticker_data.get('lowPrice', 0)),
                    'volume': float(ticker_data.get('volume', 0)),
                    'timestamp': datetime.now().isoformat(),
                    'exchange': '{exchange_name}'
                }}
            else:
                result = {{
                    'symbol': symbol.upper(),
                    'error': 'No data available',
                    'timestamp': datetime.now().isoformat()
                }}

            return result

        except Exception as e:
            print(f"Error fetching data for {{symbol}}: {{e}}")
            return {{
                'symbol': symbol.upper(),
                'error': str(e),
                'timestamp': datetime.now().isoformat()
            }}

def main():
    symbols = {symbols_json}
    collector = {exchange_name}DataCollector(
        api_key='{api_key}',
        symbols=symbols
    )

    all_data = []
    for symbol in symbols:
        print(f"Fetching crypto data for {{symbol}}...")
        data = collector.get_ticker_data(symbol)
        all_data.append(data)
        time.sleep(0.1)  # 避免超出API限制

    # 输出JSON格式数据
    print(json.dumps({{
        'exchange': '{exchange_name}',
        'data': all_data,
        'total_count': len(all_data),
        'timestamp': datetime.now().isoformat()
    }}, ensure_ascii=False, indent=2))
"#,
            exchange_name = match config.exchange {
                CryptoExchange::Binance => "Binance",
                CryptoExchange::Coinbase => "Coinbase",
                CryptoExchange::Kraken => "Kraken",
                CryptoExchange::Bitfinex => "Bitfinex",
                CryptoExchange::Huobi => "Huobi",
                CryptoExchange::OKX => "OKX",
                CryptoExchange::Bybit => "Bybit",
                CryptoExchange::Gate => "Gate",
            }
        );

        Ok(script)
    }

    /// 生成外汇数据采集脚本
    pub fn generate_forex_script(
        &self,
        pairs: &[String],
        config: &ForexSourceConfig,
    ) -> Result<String> {
        let pairs_json = serde_json::to_string(pairs)?;

        let script = format!(r#"
import requests
import json
import time
from datetime import datetime

class {source_name}DataCollector:
    def __init__(self, api_key, pairs):
        self.api_key = api_key
        self.pairs = pairs
        self.base_url = "{base_url}"
        self.headers = {{
            'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36'
        }}

    def get_exchange_rate(self, pair):
        """获取汇率数据"""
        try:
            endpoint = format!("{}/api/v4/latest", self.base_url)
            params = {{
                'access_key': self.api_key,
                'currencies': pair,
                'format': 'json'
            }}

            response = requests.get(endpoint, params=params, headers=self.headers, timeout=30)
            response.raise_for_status()

            data = response.json()

            if 'rates' in data:
                rates = data['rates']
                if pair in rates:
                    exchange_rate = rates[pair]
                    result = {{
                        'pair': pair,
                        'exchange_rate': exchange_rate,
                        'base_currency': pair.split('_')[0],
                        'quote_currency': pair.split('_')[1],
                        'timestamp': datetime.now().isoformat(),
                        'source': '{source_name}'
                    }}
                else:
                    result = {{
                        'pair': pair,
                        'error': 'Exchange rate not available',
                        'timestamp': datetime.now().isoformat()
                    }}
            else:
                result = {{
                    'pair': pair,
                    'error': 'No rates data available',
                    'timestamp': datetime.now().isoformat()
                }}

            return result

        except Exception as e:
            print(f"Error fetching forex data for {{pair}}: {{e}}")
            return {{
                'pair': pair,
                'error': str(e),
                'timestamp': datetime.now().isoformat()
            }}

def main():
    pairs = {pairs_json}
    collector = {source_name}DataCollector(
        api_key='{api_key}',
        pairs=pairs
    )

    all_data = []
    for pair in pairs:
        print(f"Fetching forex data for {{pair}}...")
        data = collector.get_exchange_rate(pair)
        all_data.append(data)
        time.sleep(1)  # 避免请求过于频繁

    # 输出JSON格式数据
    print(json.dumps({{
        'source': '{source_name}',
        'data': all_data,
        'total_count': len(all_data),
        'timestamp': datetime.now().isoformat()
    }}, ensure_ascii=False, indent=2))
"#,
            source_name = match config.source {
                ForexDataSource::Oanda => "Oanda",
                ForexDataSource::FXCM => "FXCM",
                ForexDataSource::ForexCom => "ForexCom",
                ForexDataSource::DailyFX => "DailyFX",
                ForexDataSource::MetaTrader => "MetaTrader",
            }
        );

        Ok(script)
    }

    /// 生成新闻数据采集脚本（扩展版）
    pub fn generate_extended_news_script(
        &self,
        keywords: &[String],
        languages: &[String],
        sources: &[NewsDataSource],
    ) -> Result<String> {
        let keywords_json = serde_json::to_string(keywords)?;
        let languages_json = serde_json::to_string(languages)?;

        let script = format!(r#"
import requests
import json
import time
import re
from datetime import datetime, timedelta

class ExtendedNewsDataCollector:
    def __init__(self, api_key, keywords, languages, sources):
        self.api_key = api_key
        self.keywords = keywords
        self.languages = languages
        self.sources = sources
        self.headers = {{
            'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36'
        }}

    def _clean_text(self, text):
        """清理文本"""
        # 移除HTML标签
        text = re.sub(r'<[^>]+>', '', text)
        # 移除多余空白
        text = ' '.join(text.split())
        return text.strip()

    def search_news_by_keyword(self, keyword, language='zh'):
        """根据关键词搜索新闻"""
        articles = []

        for source in self.sources:
            if source == NewsDataSource::Sina:
                articles.extend(self._search_sina_news(keyword, language))
            elif source == NewsDataSource::Tencent:
                articles.extend(self._search_tencent_news(keyword, language))
            elif source == NewsDataSource::NetEase:
                articles.extend(self._search_netease_news(keyword, language))
            elif source == NewsDataSource::Yahoo:
                articles.extend(self._search_yahoo_news(keyword, language))
            elif source == NewsDataSource::Reuters:
                articles.extend(self._search_reuters_news(keyword, language))
            elif source == NewsDataSource::Xinhua:
                articles.extend(self._search_xinhua_news(keyword, language))
            elif source == NewsDataSource::Google:
                articles.extend(self._search_google_news(keyword, language))

        return articles

    def _search_sina_news(self, keyword, language):
        """搜索新浪新闻"""
        try:
            url = f"https://news.sina.com.cn/search"
            params = {{
                'q': keyword,
                'lang': language,
                'pageSize': 20,
                'page': 1
            }}

            response = requests.get(url, params=params, headers=self.headers, timeout=30)
            response.raise_for_status()

            data = response.json()

            articles = []
            if 'data' in data and 'list' in data['data']:
                for item in data['data']['list'][:10]:
                    articles.append({{
                        'keyword': keyword,
                        'title': self._clean_text(item.get('title', '')),
                        'description': self._clean_text(item.get('content', '')),
                        'url': item.get('url', ''),
                        'source': 'Sina',
                        'published_at': item.get('stime', ''),
                        'language': language
                    }})

            return articles

        except Exception as e:
            warn!("Sina news search error: {e}");
            return []

    def _search_tencent_news(self, keyword, language):
        """搜索腾讯新闻"""
        try:
            url = f"https://api.inews.qq.com/search"
            params = {{
                'q': keyword,
                'lang': language,
                'pageSize': 20
            }}

            response = requests.get(url, params=params, headers=self.headers, timeout=30)
            response.raise_for_status()

            data = response.json()

            articles = []
            if 'data' in data and 'list' in data['data']:
                for item in data['data']['list'][:10]:
                    articles.append({{
                        'keyword': keyword,
                        'title': self._clean_text(item.get('title', '')),
                        'description': self._clean_text(item.get('summary', '')),
                        'url': item.get('url', ''),
                        'source': 'Tencent',
                        'published_at': item.get('publishTime', ''),
                        'language': language
                    }})

            return articles

        except Exception as e:
            warn!("Tencent news search error: {e}");
            return []

    def _search_yahoo_news(self, keyword, language):
        """搜索Yahoo新闻"""
        try:
            url = f"https://news.yahoo.com/rss/search"
            params = {{
                'p': keyword,
                'lang': language
            }}

            response = requests.get(url, params=params, headers=self.headers, timeout=30)
            response.raise_for_status()

            # Yahoo RSS 解析
            import feedparser
            feed = feedparser.parse(response.content)

            articles = []
            for entry in feed.entries[:10]:
                articles.append({{
                    'keyword': keyword,
                    'title': self._clean_text(entry.title),
                    'description': self._clean_text(entry.description),
                    'url': entry.link,
                    'source': 'Yahoo',
                    'published_at': entry.published,
                    'language': language
                }})

            return articles

        except Exception as e:
            warn!("Yahoo news search error: {e}");
            return []

    def _search_reuters_news(self, keyword, language):
        """搜索路透社新闻"""
        try:
            url = f"https://www.reuters.com/api/search"
            params = {{
                'q': keyword,
                'lang': language,
                'size': 10
            }}

            response = requests.get(url, params=params, headers=self.headers, timeout=30)
            response.raise_for_status()

            data = response.json()

            articles = []
            if 'data' in data and 'results' in data['data']:
                for item in data['data']['results']['list'][:10]:
                    articles.append({{
                        'keyword': keyword,
                        'title': self._clean_text(item.get('headline', '')),
                        'description': self._clean_text(item.get('body', '')),
                        'url': item.get('url', ''),
                        'source': 'Reuters',
                        'published_at': item.get('created', ''),
                        'language': language
                    }})

            return articles

        except Exception as e:
            warn!("Reuters news search error: {e}");
            return []

    def _search_xinhua_news(self, keyword, language):
        """搜索新华网新闻"""
        try:
            url = f"https://api.xinhuanet.com/search"
            params = {{
                'q': keyword,
                'lang': language,
                'pageSize': 20
            }}

            response = requests.get(url, params=params, headers=self.headers, timeout=30)
            response.raise_for_status()

            data = response.json()

            articles = []
            if 'data' in data and 'list' in data['data']:
                for item in data['data']['list'][:10]:
                    articles.append({{
                        'keyword': keyword,
                        'title': self._clean_text(item.get('title', '')),
                        'description': self._clean_text(item.get('content', '')),
                        'url': item.get('url', ''),
                        'source': 'Xinhua',
                        'published_at': item.get('publishTime', ''),
                        'language': language
                    }})

            return articles

        except Exception as e:
            warn!("Xinhua news search error: {e}");
            return []

    def _search_google_news(self, keyword, language):
        """搜索Google新闻"""
        try:
            url = "https://news.googleapis.com/v1/search"
            params = {{
                'q': keyword,
                'language': language,
                'pageSize': 10,
                'apiKey': self.api_key
            }}

            response = requests.get(url, params=params, headers=self.headers, timeout=30)
            response.raise_for_status()

            data = response.json()

            articles = []
            if 'articles' in data:
                for item in data['articles'][:10]:
                    articles.append({{
                        'keyword': keyword,
                        'title': self._clean_text(item.get('title', '')),
                        'description': self._clean_text(item.get('description', '')),
                        'url': item.get('url', ''),
                        'source': 'Google',
                        'published_at': item.get('publishedAt', ''),
                        'language': language
                    }})

            return articles

        except Exception as e:
            warn!("Google news search error: {e}");
            return []

def main():
    keywords = {keywords_json}
    languages = {languages_json}
    collector = ExtendedNewsDataCollector(
        api_key='YOUR_NEWS_API_KEY',
        keywords=keywords,
        languages=languages,
        sources=[
            NewsDataSource::Sina,
            NewsDataSource::Tencent,
            NewsDataSource::NetEase,
            NewsDataSource::Yahoo,
            NewsDataSource::Reuters,
            NewsDataSource::Xinhua,
            NewsDataSource::Google
        ]
    )

    all_articles = []
    for keyword in keywords:
        print(f"Searching news for keyword: {{keyword}}...")
        articles = collector.search_news_by_keyword(keyword, languages[0] if languages else 'zh')
        all_articles.extend(articles)
        time.sleep(0.5)  # 避免请求过于频繁

    # 输出JSON格式数据
    print(json.dumps({{
        'keywords': keywords,
        'languages': languages,
        'articles': all_articles,
        'total_count': len(all_articles),
        'timestamp': datetime.now().isoformat()
    }}, ensure_ascii=False, indent=2))
"#);

        Ok(script)
    }

    /// 生成大宗商品数据采集脚本
    pub fn generate_commodity_script(
        &self,
        symbols: &[String],
        config: &CommoditySourceConfig,
    ) -> Result<String> {
        let symbols_json = serde_json::to_string(symbols)?;

        let script = format!(r#"
import requests
import json
import time
from datetime import datetime

class {category_name}DataCollector:
    def __init__(self, api_key, symbols, exchanges):
        self.api_key = api_key
        self.symbols = symbols
        self.exchanges = exchanges
        self.headers = {{
            'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36'
        }}

    def get_commodity_data(self, symbol):
        """获取大宗商品数据"""
        try:
            # 示例：LME（伦敦金属交易所）API
            url = f"https://api.lme.com/api/v1/commodities"
            params = {{
                'symbol': symbol,
                'access_key': self.api_key
            }}

            response = requests.get(url, params=params, headers=self.headers, timeout=30)
            response.raise_for_status()

            data = response.json()

            if data and 'commodity' in data:
                commodity_data = data['commodity']
                result = {{
                    'symbol': symbol,
                    'name': commodity_data.get('name', ''),
                    'price': float(commodity_data.get('price', 0)),
                    'change': float(commodity_data.get('change', 0)),
                    'change_percent': float(commodity_data.get('changePercent', 0)),
                    'unit': commodity_data.get('unit', ''),
                    'exchange': commodity_data.get('exchange', ''),
                    'timestamp': datetime.now().isoformat(),
                    'category': '{category_name}'
                }}
            else:
                result = {{
                    'symbol': symbol,
                    'error': 'No data available',
                    'timestamp': datetime.now().isoformat()
                }}

            return result

        except Exception as e:
            print(f"Error fetching commodity data for {{symbol}}: {{e}}")
            return {{
                'symbol': symbol,
                'error': str(e),
                'timestamp': datetime.now().isoformat()
            }}

def main():
    symbols = {symbols_json}
    collector = {category_name}DataCollector(
        api_key='{api_key}',
        symbols=symbols,
        exchanges=json.dumps({{symbol: symbol for symbol in symbols}})
    )

    all_data = []
    for symbol in symbols:
        print(f"Fetching commodity data for {{symbol}}...")
        data = collector.get_commodity_data(symbol)
        all_data.append(data)
        time.sleep(1)  # 避免请求过于频繁

    # 输出JSON格式数据
    print(json.dumps({{
        'category': '{category_name}',
        'data': all_data,
        'total_count': len(all_data),
        'timestamp': datetime.now().isoformat()
    }}, ensure_ascii=False, indent=2))
"#,
            category_name = match config.category {
                CommodityCategory::PreciousMetals => "PreciousMetals",
                CommodityCategory::Energy => "Energy",
                CommodityCategory::Agriculture => "Agriculture",
                CommodityCategory::IndustrialMetals => "IndustrialMetals",
                CommodityCategory::SoftCommodities => "SoftCommodities",
            }
        );

        Ok(script)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crypto_exchange_enum() {
        let exchange = CryptoExchange::Binance;
        assert!(matches!(exchange, CryptoExchange::Binance));
    }

    #[test]
    fn test_forex_data_source_enum() {
        let source = ForexDataSource::Oanda;
        assert!(matches!(source, ForexDataSource::Oanda));
    }

    #[test]
    fn test_commodity_category_enum() {
        let category = CommodityCategory::Energy;
        assert!(matches!(category, CommodityCategory::Energy));
    }
}