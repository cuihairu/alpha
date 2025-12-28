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

// 监控模块
pub mod metrics;

// 存储层模块
pub mod storage;

// 预导出模块
pub mod prelude;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// 重新导出常用类型
pub use sources::{
    DataSource, RealtimeQuote, KlineData, KlineType, Market,
    CrawlerConfig, CrawlerResult, CrawlerError,
    SinaSource, EastmoneySource, TencentSource, Netease163Source,
    StockInfo, StockStatus, StockType,
};

// 重新导出数据清洗器
pub use cleaner::{
    DataCleaner, DataQuality, CleanResult, ValidationRules,
    PriceNormalizer, SymbolNormalizer,
};

// 重新导出调度器
pub use source_scheduler::{
    SourceScheduler, SourceTask, SourceTaskType, SourceTaskPriority,
    SourceSchedulerConfig, SourceTaskGenerator, ScheduledTaskStatus,
};

// 重新导出限流器
pub use rate_limiter::{
    ProxyPool, ProxyConfig, ProxyType, ProxyStatus,
    TokenBucketRateLimiter, SlidingWindowRateLimiter,
    MultiLevelRateLimiter, DomainRateLimiter,
    RateLimiterConfig, RateLimiterFactory,
};

// 重新导出监控模块
pub use metrics::{
    CollectorMetrics, HealthChecker, HealthCheckResult,
    HealthStatus, ComponentHealth, RequestTimer,
};

// 重新导出存储层
pub use storage::{
    StorageLayer, StorageLayerHandle, StorageConfig,
    StorageError, PostgresConfig, RedisConfig,
};

