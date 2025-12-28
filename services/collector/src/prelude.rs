//! Alpha Collector 预导出模块
//!
//! 包含最常用的类型和 trait，方便使用

pub use crate::sources::{
    DataSource, RealtimeQuote, KlineData, KlineType, Market,
    CrawlerConfig, CrawlerResult, CrawlerError,
    SinaSource, EastmoneySource, TencentSource, Netease163Source,
    StockInfo, StockStatus, StockType,
};

pub use crate::cleaner::{
    DataCleaner, DataQuality, CleanResult, ValidationRules,
    PriceNormalizer, SymbolNormalizer,
};

pub use crate::source_scheduler::{
    SourceScheduler, SourceTask, SourceTaskType, SourceTaskPriority,
    SourceSchedulerConfig, SourceTaskGenerator, ScheduledTaskStatus,
};

pub use crate::rate_limiter::{
    ProxyPool, ProxyConfig, ProxyType, ProxyStatus,
    TokenBucketRateLimiter, SlidingWindowRateLimiter,
    MultiLevelRateLimiter, DomainRateLimiter,
    RateLimiterConfig, RateLimiterFactory,
};

pub use crate::metrics::{
    CollectorMetrics, HealthChecker, HealthCheckResult,
    HealthStatus, ComponentHealth, RequestTimer,
};

pub use crate::storage::{
    StorageLayer, StorageLayerHandle, StorageConfig,
    StorageError, PostgresConfig, RedisConfig,
};
