use serde::{Deserialize, Serialize};

/// 数字货币交易所
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CryptoExchange {
    /// 币安
    Binance,
    /// 币圈
    Circle,
    /// Coinbase
    Coinbase,
    /// Kraken
    Kraken,
    /// Bitfinex
    Bitfinex,
    /// Huobi
    Huobi,
    /// OKX
    OKX,
    /// Bybit
    Bybit,
    /// Gate.io
    Gate,
}

/// 外汇数据源
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ForexDataSource {
    /// OANDA
    Oanda,
    /// FXCM
    Fxcm,
    /// Forex.com
    ForexCom,
    /// DailyFX
    DailyFx,
    /// MetaTrader
    MetaTrader,
}

/// 大宗商品类别
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CommodityCategory {
    /// 贵金属
    PreciousMetals,
    /// 能源
    Energy,
    /// 农产品
    Agriculture,
    /// 工业金属
    IndustrialMetals,
    /// 软商品
    SoftCommodities,
}

/// 债券市场
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BondMarket {
    /// 中国国债
    ChinaGovernment,
    /// 美国国债
    USGovernment,
    /// 企业债券
    Corporate,
    /// 地方政府债券
    Municipal,
    /// 国际债券
    International,
}

/// 基金市场
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FundMarket {
    /// 中国公募基金
    ChinaMutual,
    /// 美国共同基金
    USMutual,
    /// ETF基金
    ETF,
    /// 对冲基金
    HedgeFund,
    /// 指数基金
    IndexFund,
}

/// 新闻数据源
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NewsDataSource {
    /// 新浪财经
    Sina,
    /// 腾讯财经
    Tencent,
    /// 网易财经
    NetEase,
    /// 雅虎财经
    Yahoo,
    /// 路透社
    Reuters,
    /// 彭博社
    Bloomberg,
    /// 财联社
    Xinhua,
    /// Google News
    Google,
}

/// 社交媒体平台
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SocialPlatform {
    /// 微博
    Weibo,
    /// 微信公众号
    WeChat,
    /// Twitter
    Twitter,
    /// Reddit
    Reddit,
    /// 知乎
    Zhihu,
    /// 雪球
    Xueqiu,
    /// 东方财富股吧
    EastMoney,
}

/// 公告类别
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AnnouncementCategory {
    /// 重大事项
    MajorEvents,
    /// 股东大会
    ShareholderMeeting,
    /// 分红派息
    Dividend,
    /// 质押解押
    Pledge,
    /// 资产重组
    Restructuring,
    /// 关联交易
    RelatedTransaction,
    /// 业绩预告
    EarningsAnnouncement,
    /// 监管问询
    RegulatoryInquiry,
}

/// ESG数据提供商
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ESGProvider {
    /// MSCI
    MSCI,
    /// Sustainalytics
    Sustainalytics,
    /// Refinitiv
    Refinitiv,
    /// BloombergESG
    BloombergESG,
    /// 中国ESG机构
    ChinaESG,
}

/// ESG指标
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ESGMetric {
    /// 环境评分
    Environmental,
    /// 社会评分
    Social,
    /// 治理评分
    Governance,
    /// 综合ESG评分
    ESGScore,
    /// 碳排放
    CarbonEmissions,
    /// 可持续发展
    Sustainability,
}

/// 研报提供商
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ResearchProvider {
    /// 中金公司
    CICC,
    /// 中信证券
    Cicc,
    /// 海通证券
    Haitong,
    /// 申万宏源
    Shenwan,
    /// 华泰证券
    Huatai,
    /// 晨星资讯
    Morningstar,
    /// 标普全球
    SPGlobal,
    /// 摩根士丹利
    MorganStanley,
}

/// 研报类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ResearchType {
    /// 公司研报
    CompanyReport,
    /// 行业分析
    IndustryAnalysis,
    /// 宏观研究
    MacroResearch,
    /// 策略报告
    StrategyReport,
    /// 量化分析
    QuantitativeAnalysis,
    /// 估值分析
    ValuationAnalysis,
    /// 风险评估
    RiskAssessment,
}
