//! 数据收集器类型定义

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

/// 任务状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    /// 等待执行
    Pending,
    /// 正在执行
    Running,
    /// 执行成功
    Completed,
    /// 执行失败
    Failed,
    /// 已取消
    Cancelled,
}

/// 任务执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    /// 任务ID
    pub task_id: String,
    /// 任务名称
    pub task_name: String,
    /// 任务类型（数据源）
    pub source: TaskSource,
    /// 执行状态
    pub status: TaskStatus,
    /// 执行数据
    pub data: Option<Value>,
    /// 错误信息
    pub error: Option<String>,
    /// 开始时间
    pub start_time: DateTime<Utc>,
    /// 结束时间
    pub end_time: DateTime<Utc>,
    /// 执行耗时（秒）
    pub execution_time: Option<u64>,
    /// 元数据
    pub metadata: HashMap<String, Value>,
}

/// 任务定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDefinition {
    /// 任务唯一ID
    pub id: String,
    /// 任务类型（数据源）
    pub source: TaskSource,
    /// 任务名称
    pub name: String,
    /// 任务描述
    pub description: Option<String>,
    /// 调度表达式（cron格式）
    pub schedule: Option<String>,
    /// 任务优先级
    pub priority: TaskPriority,
    /// 任务配置
    pub config: TaskConfig,
    /// 重试策略
    pub retry_policy: RetryPolicy,
    /// 超时设置
    pub timeout: Option<u64>,
    /// 任务依赖
    pub dependencies: Vec<String>,
    /// 标签
    pub tags: Vec<String>,
    /// 任务状态
    pub status: TaskStatus,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 更新时间
    pub updated_at: DateTime<Utc>,
}

/// 数据源类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskSource {
    /// A股数据
    AShare {
        source: AShareDataSource,
        symbols: Vec<String>,
    },
    /// 港股数据
    HKShare {
        source: HKShareDataSource,
        symbols: Vec<String>,
    },
    /// 美股数据
    USShare {
        source: USShareDataSource,
        symbols: Vec<String>,
    },
    /// 期货数据
    Futures {
        exchange: String,
        contracts: Vec<String>,
    },
    /// 数字货币数据
    Cryptocurrency {
        exchanges: Vec<CryptoExchange>,
        symbols: Vec<String>,
    },
    /// 外汇数据
    Forex {
        sources: Vec<ForexDataSource>,
        currency_pairs: Vec<String>,
    },
    /// 大宗商品数据
    Commodities {
        categories: Vec<CommodityCategory>,
        symbols: Vec<String>,
    },
    /// 债券数据
    Bonds {
        markets: Vec<BondMarket>,
        symbols: Vec<String>,
    },
    /// 基金数据
    Funds {
        markets: Vec<FundMarket>,
        symbols: Vec<String>,
    },
    /// 经济指标数据
    EconomicIndicators {
        countries: Vec<String>,
        indicators: Vec<String>,
    },
    /// 新闻数据
    News {
        sources: Vec<NewsDataSource>,
        keywords: Vec<String>,
        languages: Vec<String>,
    },
    /// 社交媒体数据
    SocialMedia {
        platforms: Vec<SocialPlatform>,
        keywords: Vec<String>,
        sentiment_analysis: bool,
    },
    /// 公告数据
    Announcements {
        exchanges: Vec<String>,
        categories: Vec<AnnouncementCategory>,
    },
    /// 财报数据
    FinancialReports {
        symbols: Vec<String>,
        report_types: Vec<ReportType>,
    },
    /// ESG数据
    ESGData {
        providers: Vec<ESGProvider>,
        symbols: Vec<String>,
        metrics: Vec<ESGMetric>,
    },
    /// 研报数据
    ResearchReports {
        providers: Vec<ResearchProvider>,
        symbols: Vec<String>,
        report_types: Vec<ResearchType>,
    },
    /// 自定义数据源
    Custom {
        source_type: String,
        endpoint: String,
        params: HashMap<String, String>,
    },
}

impl TaskSource {
    pub fn source_type(&self) -> &'static str {
        match self {
            Self::AShare { .. } => "ashare",
            Self::HKShare { .. } => "hkshare",
            Self::USShare { .. } => "usshare",
            Self::Futures { .. } => "futures",
            Self::Cryptocurrency { .. } => "cryptocurrency",
            Self::Forex { .. } => "forex",
            Self::Commodities { .. } => "commodities",
            Self::Bonds { .. } => "bonds",
            Self::Funds { .. } => "funds",
            Self::EconomicIndicators { .. } => "economic_indicators",
            Self::News { .. } => "news",
            Self::SocialMedia { .. } => "social_media",
            Self::Announcements { .. } => "announcements",
            Self::FinancialReports { .. } => "financial_reports",
            Self::ESGData { .. } => "esg",
            Self::ResearchReports { .. } => "research_reports",
            Self::Custom { .. } => "custom",
        }
    }
}

/// A股数据源
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AShareDataSource {
    /// 新浪财经
    Sina,
    /// 腾讯财经
    Tencent,
    /// 网易财经
    NetEase,
    /// 雪球
    Xueqiu,
    /// 东方财富
    EastMoney,
    /// 同花顺
    THS,
    /// Wind
    Wind,
    /// Choice
    Choice,
}

/// 港股数据源
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HKShareDataSource {
    /// 港交所
    HKEX,
    /// 雅虎财经
    Yahoo,
    /// 新浪港股
    SinaHK,
    /// 富途
    Futu,
}

/// 美股数据源
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum USShareDataSource {
    /// 雅虎财经
    Yahoo,
    /// IEX Cloud
    IEX,
    /// Alpha Vantage
    AlphaVantage,
    /// Polygon.io
    Polygon,
    /// Finnhub
    Finnhub,
    /// Quandl
    Quandl,
}

/// 财报类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReportType {
    /// 季报
    Quarterly,
    /// 年报
    Annual,
    /// 半年报
    SemiAnnual,
    /// 业绩快报
    Preliminary,
    /// 业绩预告
    Forecast,
}

/// 任务优先级
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskPriority {
    Critical = 5,
    High = 4,
    Medium = 3,
    Low = 2,
    Background = 1,
}

/// 任务配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskConfig {
    /// 请求配置
    pub request: RequestConfig,
    /// 解析配置
    pub parser: ParserConfig,
    /// 存储配置
    pub storage: StorageConfig,
    /// 通知配置
    pub notification: Option<NotificationConfig>,
}

/// 请求配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestConfig {
    /// 请求方法
    pub method: String,
    /// 请求URL
    pub url: String,
    /// 请求头
    pub headers: HashMap<String, String>,
    /// 请求参数
    pub params: HashMap<String, String>,
    /// 请求体
    pub body: Option<String>,
    /// 代理配置
    pub proxy: Option<ProxyConfig>,
    /// 用户代理配置
    pub user_agents: Vec<String>,
    /// 请求间隔（毫秒）
    pub request_interval: u64,
    /// 重试间隔（毫秒）
    pub retry_interval: u64,
}

/// 解析配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParserConfig {
    /// 解析器类型
    pub parser_type: ParserType,
    /// 解析规则
    pub rules: Vec<ParseRule>,
    /// 数据格式
    pub data_format: DataFormat,
    /// 字段映射
    pub field_mapping: HashMap<String, String>,
}

/// 存储配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// 存储类型
    pub storage_type: StorageType,
    /// 存储目标
    pub target: String,
    /// 数据表名
    pub table: Option<String>,
    /// 批量大小
    pub batch_size: Option<usize>,
    /// 压缩选项
    pub compression: Option<CompressionType>,
}

/// 通知配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    /// 通知渠道
    pub channels: Vec<NotificationChannel>,
    /// 通知条件
    pub conditions: Vec<NotificationCondition>,
}

/// 代理配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// 代理类型
    pub proxy_type: ProxyType,
    /// 代理地址
    pub host: String,
    /// 代理端口
    pub port: u16,
    /// 认证信息
    pub auth: Option<ProxyAuth>,
    /// 连接超时（秒）
    pub timeout: Option<u64>,
}

/// 解析器类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ParserType {
    /// HTML解析
    HTML,
    /// JSON解析
    JSON,
    /// XML解析
    XML,
    /// CSV解析
    CSV,
    /// 正则表达式解析
    Regex,
    /// JavaScript解析
    JavaScript,
    /// Python脚本解析
    Python,
    /// 自定义解析器
    Custom(String),
}

/// 解析规则
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseRule {
    /// 规则名称
    pub name: String,
    /// 选择器（CSS选择器、XPath等）
    pub selector: Option<String>,
    /// 正则表达式
    pub regex: Option<String>,
    /// 属性提取
    pub attribute: Option<String>,
    /// 转换函数
    pub transform: Option<String>,
    /// 默认值
    pub default: Option<String>,
    /// 是否必需
    pub required: bool,
}

/// 数据格式
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DataFormat {
    /// JSON
    JSON,
    /// XML
    XML,
    /// CSV
    CSV,
    /// TSV
    TSV,
    /// Parquet
    Parquet,
    /// Avro
    Avro,
    /// 原始文本
    Text,
    /// 二进制数据
    Binary,
}

/// 存储类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StorageType {
    /// 内存存储
    Memory,
    /// 文件存储
    File,
    /// 数据库存储
    Database,
    /// 消息队列
    MessageQueue,
    /// 对象存储
    ObjectStorage,
}

/// 压缩类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CompressionType {
    /// Gzip压缩
    Gzip,
    /// Brotli压缩
    Brotli,
    /// LZ4压缩
    LZ4,
    /// Snappy压缩
    Snappy,
    /// Zstd压缩
    Zstd,
}

/// 通知渠道
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NotificationChannel {
    /// 邮件通知
    Email,
    /// 短信通知
    SMS,
    /// Slack通知
    Slack,
    /// 钉钉通知
    DingTalk,
    /// 企业微信通知
    WeChat,
    /// Webhook通知
    Webhook(String),
}

/// 通知条件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationCondition {
    /// 条件类型
    pub condition_type: ConditionType,
    /// 条件值
    pub value: String,
    /// 阈值
    pub threshold: Option<f64>,
    /// 严重级别
    pub severity: SeverityLevel,
}

/// 条件类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConditionType {
    /// 任务失败
    TaskFailed,
    /// 数据量异常
    DataAnomaly,
    /// 响应时间过长
    SlowResponse,
    /// 错误率过高
    HighErrorRate,
    /// 自定义条件
    Custom(String),
}

/// 严重级别
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum SeverityLevel {
    Info = 1,
    Warning = 2,
    Error = 3,
    Critical = 4,
}

/// 代理类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProxyType {
    /// HTTP代理
    HTTP,
    /// HTTPS代理
    HTTPS,
    /// SOCKS4代理
    SOCKS4,
    /// SOCKS5代理
    SOCKS5,
    /// 透明代理
    Transparent,
}

/// 代理认证
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyAuth {
    /// 用户名
    pub username: String,
    /// 密码
    pub password: String,
}

/// 重试策略
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// 最大重试次数
    pub max_retries: u32,
    /// 基础延迟（毫秒）
    pub base_delay: u64,
    /// 最大延迟（毫秒）
    pub max_delay: u64,
    /// 退避策略
    pub backoff_strategy: BackoffStrategy,
    /// 重试条件
    pub retry_conditions: Vec<RetryCondition>,
}

/// 退避策略
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BackoffStrategy {
    /// 固定延迟
    Fixed,
    /// 线性退避
    Linear,
    /// 指数退避
    Exponential,
    /// 带抖动的指数退避
    ExponentialWithJitter,
}

/// 重试条件
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RetryCondition {
    /// HTTP错误码
    HttpError(Vec<u16>),
    /// 网络错误
    NetworkError,
    /// 超时错误
    TimeoutError,
    /// 解析错误
    ParseError,
    /// 数据验证错误
    ValidationError,
}


/// 采集统计信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionStats {
    /// 采集时间
    pub timestamp: DateTime<Utc>,
    /// 数据源
    pub source: TaskSource,
    /// 成功次数
    pub success_count: u64,
    /// 失败次数
    pub failure_count: u64,
    /// 平均响应时间
    pub avg_response_time: f64,
    /// 数据量大小
    pub data_size: u64,
    /// 记录数量
    pub record_count: u64,
    /// 错误率
    pub error_rate: f64,
}

impl TaskDefinition {
    /// 创建新任务
    pub fn new<S: Into<String>>(
        id: S,
        source: TaskSource,
        name: S,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: id.into(),
            source,
            name: name.into(),
            description: None,
            schedule: None,
            priority: TaskPriority::Medium,
            config: TaskConfig::default(),
            retry_policy: RetryPolicy::default(),
            timeout: None,
            dependencies: Vec::new(),
            tags: Vec::new(),
            status: TaskStatus::Pending,
            created_at: now,
            updated_at: now,
        }
    }

    /// 检查任务是否可以执行
    pub fn can_execute(&self) -> bool {
        match &self.status {
            TaskStatus::Pending => true,
            TaskStatus::Failed => true, // 可以重试
            _ => false,
        }
    }

    /// 获取任务权重（用于调度）
    pub fn weight(&self) -> u8 {
        self.priority.clone() as u8
    }

    /// 设置调度
    pub fn with_schedule<S: Into<String>>(mut self, schedule: S) -> Self {
        self.schedule = Some(schedule.into());
        self
    }

    /// 设置优先级
    pub fn with_priority(mut self, priority: TaskPriority) -> Self {
        self.priority = priority;
        self
    }

    /// 设置描述
    pub fn with_description<S: Into<String>>(mut self, description: S) -> Self {
        self.description = Some(description.into());
        self
    }

    /// 添加标签
    pub fn with_tag<S: Into<String>>(mut self, tag: S) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// 添加依赖
    pub fn with_dependency<S: Into<String>>(mut self, dependency: S) -> Self {
        self.dependencies.push(dependency.into());
        self
    }
}

impl Default for TaskDefinition {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            source: TaskSource::AShare {
                source: AShareDataSource::Sina,
                symbols: vec![],
            },
            name: "默认任务".to_string(),
            description: None,
            schedule: None,
            priority: TaskPriority::Medium,
            config: TaskConfig::default(),
            retry_policy: RetryPolicy::default(),
            timeout: Some(300), // 5分钟
            dependencies: vec![],
            tags: vec![],
            status: TaskStatus::Pending,
            created_at: now,
            updated_at: now,
        }
    }
}

impl Default for TaskConfig {
    fn default() -> Self {
        Self {
            request: RequestConfig::default(),
            parser: ParserConfig::default(),
            storage: StorageConfig::default(),
            notification: None,
        }
    }
}

impl Default for RequestConfig {
    fn default() -> Self {
        Self {
            method: "GET".to_string(),
            url: String::new(),
            headers: HashMap::new(),
            params: HashMap::new(),
            body: None,
            proxy: None,
            user_agents: vec![
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36".to_string(),
            ],
            request_interval: 1000,
            retry_interval: 5000,
        }
    }
}

impl Default for ParserConfig {
    fn default() -> Self {
        Self {
            parser_type: ParserType::JSON,
            rules: Vec::new(),
            data_format: DataFormat::JSON,
            field_mapping: HashMap::new(),
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            storage_type: StorageType::Memory,
            target: String::new(),
            table: None,
            batch_size: Some(1000),
            compression: None,
        }
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: 1000,
            max_delay: 60000,
            backoff_strategy: BackoffStrategy::ExponentialWithJitter,
            retry_conditions: vec![
                RetryCondition::HttpError(vec![500, 502, 503, 504]),
                RetryCondition::NetworkError,
                RetryCondition::TimeoutError,
            ],
        }
    }
}

impl Default for TaskPriority {
    fn default() -> Self {
        TaskPriority::Medium
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_definition_creation() {
        let task = TaskDefinition::new(
            "test-task-1",
            TaskSource::AShare {
                source: AShareDataSource::Sina,
                symbols: vec!["000001".to_string(), "000002".to_string()],
            },
            "Sina A股数据采集",
        );

        assert_eq!(task.id, "test-task-1");
        assert_eq!(task.name, "Sina A股数据采集");
        assert_eq!(task.priority, TaskPriority::Medium);
    }

    #[test]
    fn test_task_builder_methods() {
        let task = TaskDefinition::new(
            "test-task-2",
            TaskSource::News {
                sources: vec![NewsDataSource::Xinhua],
                keywords: vec!["财经".to_string()],
                languages: vec!["zh".to_string()],
            },
            "新闻采集",
        )
        .with_priority(TaskPriority::High)
        .with_description("新华财经新闻采集")
        .with_tag("finance")
        .with_schedule("0 */5 * * * *")
        .with_dependency("previous-task");

        assert_eq!(task.priority, TaskPriority::High);
        assert_eq!(task.description, Some("新华财经新闻采集".to_string()));
        assert_eq!(task.tags, vec!["finance"]);
        assert_eq!(task.schedule, Some("0 */5 * * * *".to_string()));
        assert_eq!(task.dependencies, vec!["previous-task"]);
    }

    #[test]
    fn test_task_priority_ordering() {
        assert!(TaskPriority::Critical > TaskPriority::High);
        assert!(TaskPriority::High > TaskPriority::Medium);
        assert!(TaskPriority::Medium > TaskPriority::Low);
        assert!(TaskPriority::Low > TaskPriority::Background);
    }

    #[test]
    fn test_task_weight() {
        let critical_task = TaskDefinition::new("critical", TaskSource::Custom {
            source_type: "test".to_string(),
            endpoint: "".to_string(),
            params: HashMap::new(),
        }, "Critical Task").with_priority(TaskPriority::Critical);

        assert_eq!(critical_task.weight(), 5);
    }
}

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
