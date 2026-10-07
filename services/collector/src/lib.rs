//! Alpha Collector Service library.
//!
//! A股数据采集服务，支持多种数据源的实时行情和历史数据获取

pub mod main_simple;
pub mod multilang_simple;
pub mod types;

// 数据源模块
pub mod sources;

// 数据清洗和标准化模块
pub mod cleaner;

// 原有调度器模块（多语言任务调度）
pub mod scheduler;

// 数据源任务调度器
pub mod source_scheduler;

// 限流和代理模块
pub mod rate_limiter;

// 存储层模块
pub mod storage;

// 采集任务模板（YAML/JSON 声明式任务定义，architecture §24）
pub mod task_templates;

// Cron 调度（architecture §24 刷新频率执行面）
pub mod cron_scheduler;

// 原始响应归档（MinIO/S3 取证底座，env 门控默认关）
pub mod raw_archive;

// 数据源健康面（L504：执行结果推导三态 + /sources/health + gauge）
pub mod source_health;

// 预导出模块
pub mod prelude;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// 重新导出常用类型
pub use sources::{
    CrawlerConfig, CrawlerError, CrawlerResult, DataSource, EastmoneySource, KlineData, KlineType,
    Market, Netease163Source, RealtimeQuote, SinaSource, StockInfo, StockStatus, StockType,
    TencentSource,
};

// 重新导出数据清洗器
pub use cleaner::{
    CleanResult, DataCleaner, DataQuality, PriceNormalizer, SymbolNormalizer, ValidationRules,
};

// 重新导出调度器
pub use source_scheduler::{
    ScheduledTaskStatus, SourceScheduler, SourceSchedulerConfig, SourceTask, SourceTaskGenerator,
    SourceTaskPriority, SourceTaskType,
};

// 重新导出限流器
pub use rate_limiter::{
    DomainRateLimiter, MultiLevelRateLimiter, ProxyConfig, ProxyPool, ProxyStatus, ProxyType,
    RateLimiterConfig, RateLimiterFactory, SlidingWindowRateLimiter, TokenBucketRateLimiter,
};

// 重新导出存储层
pub use storage::{
    PostgresConfig, RedisConfig, StorageConfig, StorageError, StorageLayer, StorageLayerHandle,
};
