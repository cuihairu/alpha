//! Alpha Collector 预导出模块
//!
//! 包含最常用的类型和 trait，方便使用

pub use crate::sources::{
    CrawlerConfig, CrawlerError, CrawlerResult, DataSource, EastmoneySource, KlineData, KlineType,
    Market, Netease163Source, RealtimeQuote, SinaSource, StockInfo, StockStatus, StockType,
    TencentSource,
};

pub use crate::cleaner::{
    CleanResult, DataCleaner, DataQuality, PriceNormalizer, SymbolNormalizer, ValidationRules,
};

pub use crate::source_scheduler::{
    ScheduledTaskStatus, SourceScheduler, SourceSchedulerConfig, SourceTask, SourceTaskGenerator,
    SourceTaskPriority, SourceTaskType,
};

pub use crate::rate_limiter::{
    DomainRateLimiter, MultiLevelRateLimiter, ProxyConfig, ProxyPool, ProxyStatus, ProxyType,
    RateLimiterConfig, RateLimiterFactory, SlidingWindowRateLimiter, TokenBucketRateLimiter,
};

pub use crate::metrics::{
    CollectorMetrics, ComponentHealth, HealthCheckResult, HealthChecker, HealthStatus, RequestTimer,
};

pub use crate::storage::{
    PostgresConfig, RedisConfig, StorageConfig, StorageError, StorageLayer, StorageLayerHandle,
};
