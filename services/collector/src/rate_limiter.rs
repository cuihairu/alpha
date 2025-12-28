//! 限流和代理池模块
//!
//! 提供 API 请求限流、代理池管理和智能轮换功能

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tracing::{info, warn};

/// 代理配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// 代理地址
    pub url: String,
    /// 用户名
    pub username: Option<String>,
    /// 密码
    pub password: Option<String>,
    /// 代理类型
    #[serde(default)]
    pub proxy_type: ProxyType,
}

/// 代理类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ProxyType {
    /// HTTP 代理
    #[default]
    Http,
    /// HTTPS 代理
    Https,
    /// SOCKS5 代理
    Socks5,
}

impl std::fmt::Display for ProxyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProxyType::Http => write!(f, "http"),
            ProxyType::Https => write!(f, "https"),
            ProxyType::Socks5 => write!(f, "socks5"),
        }
    }
}

impl ProxyConfig {
    /// 构建完整的代理 URL
    pub fn build_url(&self) -> String {
        if let (Some(username), Some(password)) = (&self.username, &self.password) {
            format!("{}://{}:{}@{}", self.proxy_type, username, password, self.url)
        } else {
            format!("{}://{}", self.proxy_type, self.url)
        }
    }
}

/// 代理状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyStatus {
    /// 代理配置
    pub config: ProxyConfig,
    /// 是否可用
    pub is_available: bool,
    /// 成功请求数
    pub success_count: u64,
    /// 失败请求数
    pub failure_count: u64,
    /// 平均响应时间（毫秒）
    pub avg_response_time_ms: u64,
    /// 最后使用时间
    pub last_used: Option<DateTime<Utc>>,
    /// 最后检查时间
    pub last_checked: Option<DateTime<Utc>>,
}

impl ProxyStatus {
    pub fn new(config: ProxyConfig) -> Self {
        Self {
            config,
            is_available: true,
            success_count: 0,
            failure_count: 0,
            avg_response_time_ms: 0,
            last_used: None,
            last_checked: Some(Utc::now()),
        }
    }

    /// 获取成功率
    pub fn success_rate(&self) -> f64 {
        let total = self.success_count + self.failure_count;
        if total == 0 {
            return 1.0;
        }
        self.success_count as f64 / total as f64
    }

    /// 是否健康（成功率大于 50%）
    pub fn is_healthy(&self) -> bool {
        self.success_rate() > 0.5
    }

    /// 记录成功请求
    pub fn record_success(&mut self, response_time_ms: u64) {
        self.success_count += 1;
        self.last_used = Some(Utc::now());

        // 更新平均响应时间（简单移动平均）
        if self.avg_response_time_ms == 0 {
            self.avg_response_time_ms = response_time_ms;
        } else {
            self.avg_response_time_ms =
                (self.avg_response_time_ms + response_time_ms) / 2;
        }
    }

    /// 记录失败请求
    pub fn record_failure(&mut self) {
        self.failure_count += 1;
        self.last_used = Some(Utc::now());

        // 如果失败率过高，标记为不可用
        if !self.is_healthy() {
            self.is_available = false;
        }
    }
}

/// 代理池配置
#[derive(Debug, Clone)]
pub struct ProxyPoolConfig {
    /// 最小健康代理数量
    pub min_healthy_proxies: usize,
    /// 代理健康检查间隔（秒）
    pub health_check_interval: u64,
    /// 代理最大连续失败次数
    pub max_consecutive_failures: u32,
    /// 代理超时时间（秒）
    pub proxy_timeout: u64,
}

impl Default for ProxyPoolConfig {
    fn default() -> Self {
        Self {
            min_healthy_proxies: 1,
            health_check_interval: 60,
            max_consecutive_failures: 5,
            proxy_timeout: 10,
        }
    }
}

/// 代理池
pub struct ProxyPool {
    /// 所有代理
    proxies: Arc<RwLock<Vec<ProxyStatus>>>,
    /// 当前代理索引（轮询）
    current_index: Arc<Mutex<usize>>,
    /// 配置
    config: ProxyPoolConfig,
}

impl ProxyPool {
    /// 创建新的代理池
    pub fn new(proxies: Vec<ProxyConfig>, config: ProxyPoolConfig) -> Self {
        let proxy_statuses: Vec<ProxyStatus> = proxies
            .into_iter()
            .map(ProxyStatus::new)
            .collect();

        Self {
            proxies: Arc::new(RwLock::new(proxy_statuses)),
            current_index: Arc::new(Mutex::new(0)),
            config,
        }
    }

    /// 获取下一个可用的代理
    pub async fn get_next_proxy(&self) -> Option<ProxyConfig> {
        let proxies = self.proxies.read().await;
        let mut index = self.current_index.lock().await;

        let healthy_proxies: Vec<_> = proxies
            .iter()
            .enumerate()
            .filter(|(_, p)| p.is_available && p.is_healthy())
            .collect();

        if healthy_proxies.is_empty() {
            warn!("No healthy proxies available");
            return None;
        }

        // 轮询选择
        *index = (*index + 1) % healthy_proxies.len();
        let (_, proxy_status) = healthy_proxies.get(*index)?;

        Some(proxy_status.config.clone())
    }

    /// 获取最佳代理（基于响应时间和成功率）
    pub async fn get_best_proxy(&self) -> Option<ProxyConfig> {
        let proxies = self.proxies.read().await;

        proxies
            .iter()
            .filter(|p| p.is_available && p.is_healthy())
            .min_by_key(|p| {
                // 综合评分：响应时间 + 失败率惩罚
                let score = p.avg_response_time_ms as f64 / p.success_rate();
                (score * 100.0) as u64
            })
            .map(|p| p.config.clone())
    }

    /// 记录代理请求结果
    pub async fn record_result(&self, proxy_config: &ProxyConfig, success: bool, response_time_ms: u64) {
        let mut proxies = self.proxies.write().await;

        if let Some(proxy_status) = proxies
            .iter_mut()
            .find(|p| p.config.url == proxy_config.url)
        {
            if success {
                proxy_status.record_success(response_time_ms);
                // 恢复可用状态
                if !proxy_status.is_available && proxy_status.is_healthy() {
                    proxy_status.is_available = true;
                }
            } else {
                proxy_status.record_failure();
            }
        }
    }

    /// 获取代理统计信息
    pub async fn get_stats(&self) -> Vec<ProxyStatus> {
        let proxies = self.proxies.read().await;
        proxies.clone()
    }

    /// 添加新代理
    pub async fn add_proxy(&self, config: ProxyConfig) {
        let mut proxies = self.proxies.write().await;
        proxies.push(ProxyStatus::new(config));
    }

    /// 移除代理
    pub async fn remove_proxy(&self, url: &str) {
        let mut proxies = self.proxies.write().await;
        proxies.retain(|p| p.config.url != url);
    }

    /// 启动健康检查任务
    pub async fn start_health_check(&self) {
        let proxies = Arc::clone(&self.proxies);
        let config = self.config.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(config.health_check_interval));

            loop {
                interval.tick().await;

                let mut proxies_write = proxies.write().await;
                info!("Running proxy health check...");

                for proxy in &mut *proxies_write {
                    // 简单的健康检查逻辑
                    // 实际项目中应该发送真实的测试请求
                    if !proxy.is_healthy() {
                        warn!("Proxy {} is unhealthy, marking as unavailable", proxy.config.url);
                        proxy.is_available = false;
                    } else if !proxy.is_available && proxy.success_rate() > 0.5 {
                        info!("Proxy {} recovered, marking as available", proxy.config.url);
                        proxy.is_available = true;
                    }
                }
            }
        });
    }
}

/// 限流器配置
#[derive(Debug, Clone)]
pub struct RateLimiterConfig {
    /// 每秒请求数限制
    pub requests_per_second: u64,
    /// 每分钟请求数限制
    pub requests_per_minute: Option<u64>,
    /// 每小时请求数限制
    pub requests_per_hour: Option<u64>,
    /// 令牌桶初始容量
    pub initial_burst: u64,
}

impl Default for RateLimiterConfig {
    fn default() -> Self {
        Self {
            requests_per_second: 10,
            requests_per_minute: Some(600),
            requests_per_hour: Some(10000),
            initial_burst: 20,
        }
    }
}

/// 令牌桶限流器
pub struct TokenBucketRateLimiter {
    /// 令牌桶容量
    capacity: u64,
    /// 令牌补充速率（每秒）
    refill_rate: u64,
    /// 当前令牌数
    tokens: Arc<Mutex<u64>>,
    /// 上次补充时间
    last_refill: Arc<Mutex<Instant>>,
}

impl TokenBucketRateLimiter {
    /// 创建新的令牌桶限流器
    pub fn new(capacity: u64, refill_rate: u64) -> Self {
        Self {
            capacity,
            refill_rate,
            tokens: Arc::new(Mutex::new(capacity)),
            last_refill: Arc::new(Mutex::new(Instant::now())),
        }
    }

    /// 尝试获取令牌
    pub async fn try_acquire(&self, tokens: u64) -> bool {
        self.refill_tokens().await;

        let mut current_tokens = self.tokens.lock().await;

        if *current_tokens >= tokens {
            *current_tokens -= tokens;
            true
        } else {
            false
        }
    }

    /// 等待并获取令牌
    pub async fn acquire(&self, tokens: u64) {
        loop {
            self.refill_tokens().await;

            let mut current_tokens = self.tokens.lock().await;

            if *current_tokens >= tokens {
                *current_tokens -= tokens;
                return;
            }

            // 计算需要等待的时间
            let needed = tokens - *current_tokens;
            let wait_duration = Duration::from_secs(needed / self.refill_rate + 1);
            drop(current_tokens);
            tokio::time::sleep(wait_duration).await;
        }
    }

    /// 补充令牌
    async fn refill_tokens(&self) {
        let mut last_refill = self.last_refill.lock().await;
        let now = Instant::now();
        let elapsed = now.duration_since(*last_refill);

        if elapsed.as_secs() > 0 {
            let new_tokens = (elapsed.as_secs() * self.refill_rate).min(self.capacity);

            let mut tokens = self.tokens.lock().await;
            *tokens = (*tokens + new_tokens).min(self.capacity);

            *last_refill = now;
        }
    }

    /// 获取当前令牌数
    pub async fn available_tokens(&self) -> u64 {
        self.refill_tokens().await;
        *self.tokens.lock().await
    }
}

/// 滑动窗口限流器
pub struct SlidingWindowRateLimiter {
    /// 时间窗口大小（秒）
    window_size: u64,
    /// 窗口内最大请求数
    max_requests: u64,
    /// 请求记录
    requests: Arc<Mutex<Vec<Instant>>>,
}

impl SlidingWindowRateLimiter {
    /// 创建新的滑动窗口限流器
    pub fn new(window_size: u64, max_requests: u64) -> Self {
        Self {
            window_size,
            max_requests,
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 尝试获取许可
    pub async fn try_acquire(&self) -> bool {
        let mut requests = self.requests.lock().await;
        let now = Instant::now();

        // 清理过期的请求记录
        let window_duration = Duration::from_secs(self.window_size);
        requests.retain(|&t| now.duration_since(t) < window_duration);

        // 检查是否超过限制
        if requests.len() < self.max_requests as usize {
            requests.push(now);
            true
        } else {
            false
        }
    }

    /// 等待并获取许可
    pub async fn acquire(&self) {
        while !self.try_acquire().await {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// 获取当前窗口内的请求数
    pub async fn current_requests(&self) -> usize {
        let mut requests = self.requests.lock().await;
        let now = Instant::now();

        // 清理过期的请求记录
        let window_duration = Duration::from_secs(self.window_size);
        requests.retain(|&t| now.duration_since(t) < window_duration);

        requests.len()
    }
}

/// 多级限流器
pub struct MultiLevelRateLimiter {
    /// 令牌桶限流器（秒级）
    token_limiter: TokenBucketRateLimiter,
    /// 滑动窗口限流器（分钟级）
    minute_limiter: Option<SlidingWindowRateLimiter>,
    /// 滑动窗口限流器（小时级）
    hour_limiter: Option<SlidingWindowRateLimiter>,
}

impl MultiLevelRateLimiter {
    /// 创建新的多级限流器
    pub fn new(config: RateLimiterConfig) -> Self {
        let token_limiter = TokenBucketRateLimiter::new(
            config.initial_burst,
            config.requests_per_second,
        );

        let minute_limiter = config.requests_per_minute.map(|rpm| {
            SlidingWindowRateLimiter::new(60, rpm)
        });

        let hour_limiter = config.requests_per_hour.map(|rph| {
            SlidingWindowRateLimiter::new(3600, rph)
        });

        Self {
            token_limiter,
            minute_limiter,
            hour_limiter,
        }
    }

    /// 尝试获取许可
    pub async fn try_acquire(&self) -> bool {
        // 检查所有级别的限流
        if !self.token_limiter.try_acquire(1).await {
            return false;
        }

        if let Some(ref minute) = self.minute_limiter {
            if !minute.try_acquire().await {
                return false;
            }
        }

        if let Some(ref hour) = self.hour_limiter {
            if !hour.try_acquire().await {
                return false;
            }
        }

        true
    }

    /// 等待并获取许可
    pub async fn acquire(&self) {
        self.token_limiter.acquire(1).await;

        if let Some(ref minute) = self.minute_limiter {
            minute.acquire().await;
        }

        if let Some(ref hour) = self.hour_limiter {
            hour.acquire().await;
        }
    }

    /// 获取可用令牌数
    pub async fn available_tokens(&self) -> u64 {
        self.token_limiter.available_tokens().await
    }
}

/// 限流器工厂
pub struct RateLimiterFactory;

impl RateLimiterFactory {
    /// 创建令牌桶限流器
    pub fn create_token_bucket(capacity: u64, refill_rate: u64) -> TokenBucketRateLimiter {
        TokenBucketRateLimiter::new(capacity, refill_rate)
    }

    /// 创建滑动窗口限流器
    pub fn create_sliding_window(window_size: u64, max_requests: u64) -> SlidingWindowRateLimiter {
        SlidingWindowRateLimiter::new(window_size, max_requests)
    }

    /// 创建多级限流器
    pub fn create_multi_level(config: RateLimiterConfig) -> MultiLevelRateLimiter {
        MultiLevelRateLimiter::new(config)
    }
}

/// 按域名分组的限流器管理器
pub struct DomainRateLimiter {
    /// 各域名的限流器
    limiters: Arc<RwLock<HashMap<String, Arc<MultiLevelRateLimiter>>>>,
    /// 默认限流器配置
    default_config: RateLimiterConfig,
}

impl DomainRateLimiter {
    /// 创建新的域名限流器管理器
    pub fn new(default_config: RateLimiterConfig) -> Self {
        Self {
            limiters: Arc::new(RwLock::new(HashMap::new())),
            default_config,
        }
    }

    /// 为指定域名获取或创建限流器
    pub async fn get_limiter(&self, domain: &str) -> Arc<MultiLevelRateLimiter> {
        let mut limiters = self.limiters.write().await;

        if let Some(limiter) = limiters.get(domain) {
            Arc::clone(limiter)
        } else {
            let limiter = Arc::new(MultiLevelRateLimiter::new(self.default_config.clone()));
            limiters.insert(domain.to_string(), Arc::clone(&limiter));
            limiter
        }
    }

    /// 尝试获取许可
    pub async fn try_acquire(&self, domain: &str) -> bool {
        let limiter = self.get_limiter(domain).await;
        limiter.try_acquire().await
    }

    /// 等待并获取许可
    pub async fn acquire(&self, domain: &str) {
        let limiter = self.get_limiter(domain).await;
        limiter.acquire().await
    }

    /// 获取所有域名的限流统计
    pub async fn get_stats(&self) -> HashMap<String, u64> {
        let limiters = self.limiters.read().await;
        let mut stats = HashMap::new();

        for (domain, limiter) in limiters.iter() {
            stats.insert(domain.clone(), limiter.available_tokens().await);
        }

        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_token_bucket_rate_limiter() {
        let limiter = TokenBucketRateLimiter::new(10, 5); // 容量10，每秒补充5个

        // 初始应该有10个令牌
        assert_eq!(limiter.available_tokens().await, 10);

        // 获取5个令牌
        limiter.acquire(5).await;
        assert_eq!(limiter.available_tokens().await, 5);

        // 尝试获取10个令牌应该失败
        assert!(!limiter.try_acquire(10).await);
    }

    #[tokio::test]
    async fn test_sliding_window_rate_limiter() {
        let limiter = SlidingWindowRateLimiter::new(1, 2); // 1秒内最多2个请求

        // 第一次请求应该成功
        assert!(limiter.try_acquire().await);

        // 第二次请求应该成功
        assert!(limiter.try_acquire().await);

        // 第三次请求应该失败
        assert!(!limiter.try_acquire().await);

        // 等待窗口过期
        tokio::time::sleep(Duration::from_secs(2)).await;

        // 现在应该可以再次获取
        assert!(limiter.try_acquire().await);
    }

    #[tokio::test]
    async fn test_proxy_pool() {
        let proxies = vec![
            ProxyConfig {
                url: "proxy1.example.com:8080".to_string(),
                username: None,
                password: None,
                proxy_type: ProxyType::Http,
            },
            ProxyConfig {
                url: "proxy2.example.com:8080".to_string(),
                username: None,
                password: None,
                proxy_type: ProxyType::Http,
            },
        ];

        let pool = ProxyPool::new(proxies, ProxyPoolConfig::default());

        // 应该能获取代理
        let proxy = pool.get_next_proxy().await;
        assert!(proxy.is_some());

        // 记录成功
        if let Some(ref p) = proxy {
            pool.record_result(p, true, 100).await;
        }
    }
}
