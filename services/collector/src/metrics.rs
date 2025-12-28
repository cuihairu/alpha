//! 监控指标和健康检查模块
//!
//! 提供爬虫服务的监控指标收集、健康检查和 Prometheus 导出功能

use chrono::{DateTime, Utc};
use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounterVec, IntGauge, IntGaugeVec, Opts, Registry,
    TextEncoder,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// 监控指标收集器
#[derive(Clone)]
pub struct CollectorMetrics {
    /// 注册表
    registry: Arc<Registry>,

    /// 请求数计数器
    requests_total: IntCounterVec,
    /// 成功请求数计数器
    requests_success_total: IntCounterVec,
    /// 失败请求数计数器
    requests_failed_total: IntCounterVec,

    /// 请求延迟直方图
    request_duration_seconds: HistogramVec,

    /// 当前活跃请求数
    active_requests: IntGaugeVec,

    /// 数据采集计数器
    data_points_collected: IntCounterVec,

    /// 队列长度
    queue_length: IntGaugeVec,

    /// 数据源健康状态
    source_healthy: IntGaugeVec,

    /// 代理池可用代理数
    proxy_pool_size: IntGauge,
}

impl CollectorMetrics {
    /// 创建新的监控指标收集器
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let registry = Arc::new(Registry::new());

        // 请求数计数器
        let requests_total = IntCounterVec::new(
            Opts::new("collector_requests_total", "Total number of requests"),
            &["source", "task_type"],
        )?;

        // 成功请求数计数器
        let requests_success_total = IntCounterVec::new(
            Opts::new("collector_requests_success_total", "Total number of successful requests"),
            &["source", "task_type"],
        )?;

        // 失败请求数计数器
        let requests_failed_total = IntCounterVec::new(
            Opts::new("collector_requests_failed_total", "Total number of failed requests"),
            &["source", "task_type", "error_type"],
        )?;

        // 请求延迟直方图
        let request_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "collector_request_duration_seconds",
                "Request duration in seconds",
            )
            .buckets(vec![
                0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
            ]),
            &["source", "task_type"],
        )?;

        // 当前活跃请求数
        let active_requests = IntGaugeVec::new(
            Opts::new("collector_active_requests", "Number of active requests"),
            &["source"],
        )?;

        // 数据采集计数器
        let data_points_collected = IntCounterVec::new(
            Opts::new("collector_data_points_collected", "Total number of data points collected"),
            &["source", "data_type"],
        )?;

        // 队列长度
        let queue_length = IntGaugeVec::new(
            Opts::new("collector_queue_length", "Current queue length"),
            &["priority"],
        )?;

        // 数据源健康状态
        let source_healthy = IntGaugeVec::new(
            Opts::new("collector_source_healthy", "Data source health status (1=healthy, 0=unhealthy)"),
            &["source"],
        )?;

        // 代理池可用代理数
        let proxy_pool_size = IntGauge::new(
            "collector_proxy_pool_size",
            "Number of available proxies"
        )?;

        // 注册所有指标
        registry.register(Box::new(requests_total.clone()))?;
        registry.register(Box::new(requests_success_total.clone()))?;
        registry.register(Box::new(requests_failed_total.clone()))?;
        registry.register(Box::new(request_duration_seconds.clone()))?;
        registry.register(Box::new(active_requests.clone()))?;
        registry.register(Box::new(data_points_collected.clone()))?;
        registry.register(Box::new(queue_length.clone()))?;
        registry.register(Box::new(source_healthy.clone()))?;
        registry.register(Box::new(proxy_pool_size.clone()))?;

        Ok(Self {
            registry,
            requests_total,
            requests_success_total,
            requests_failed_total,
            request_duration_seconds,
            active_requests,
            data_points_collected,
            queue_length,
            source_healthy,
            proxy_pool_size,
        })
    }

    /// 记录请求开始
    pub fn record_request_start(&self, source: &str, task_type: &str) {
        self.requests_total
            .with_label_values(&[source, task_type])
            .inc();
        self.active_requests
            .with_label_values(&[source])
            .inc();
    }

    /// 记录请求成功
    pub fn record_request_success(&self, source: &str, task_type: &str, duration_secs: f64) {
        self.requests_success_total
            .with_label_values(&[source, task_type])
            .inc();
        self.request_duration_seconds
            .with_label_values(&[source, task_type])
            .observe(duration_secs);
        self.active_requests.with_label_values(&[source]).dec();
    }

    /// 记录请求失败
    pub fn record_request_failure(&self, source: &str, task_type: &str, error_type: &str) {
        self.requests_failed_total
            .with_label_values(&[source, task_type, error_type])
            .inc();
        self.active_requests.with_label_values(&[source]).dec();
    }

    /// 记录数据点采集
    pub fn record_data_points(&self, source: &str, data_type: &str, count: u64) {
        self.data_points_collected
            .with_label_values(&[source, data_type])
            .inc_by(count);
    }

    /// 更新队列长度
    pub fn update_queue_length(&self, priority: &str, length: i64) {
        self.queue_length
            .with_label_values(&[priority])
            .set(length);
    }

    /// 更新数据源健康状态
    pub fn update_source_health(&self, source: &str, healthy: bool) {
        self.source_healthy
            .with_label_values(&[source])
            .set(if healthy { 1 } else { 0 });
    }

    /// 更新代理池大小
    pub fn update_proxy_pool_size(&self, size: i64) {
        self.proxy_pool_size.set(size);
    }

    /// 导出 Prometheus 格式的指标
    pub fn export_prometheus(&self) -> Result<String, Box<dyn std::error::Error>> {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer)?;
        Ok(String::from_utf8(buffer)?)
    }

    /// 获取注册表引用
    pub fn registry(&self) -> &Registry {
        &self.registry
    }
}

impl Default for CollectorMetrics {
    fn default() -> Self {
        Self::new().expect("Failed to create CollectorMetrics")
    }
}

/// 健康检查状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    /// 健康
    Healthy,
    /// 不健康
    Unhealthy,
    /// 降级服务
    Degraded,
}

/// 健康检查结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    /// 总体状态
    pub status: HealthStatus,
    /// 检查时间
    pub checked_at: DateTime<Utc>,
    /// 各组件状态
    pub components: HashMap<String, ComponentHealth>,
}

/// 组件健康状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentHealth {
    /// 组件名称
    pub name: String,
    /// 健康状态
    pub status: HealthStatus,
    /// 状态消息
    pub message: Option<String>,
    /// 额外信息
    pub metadata: HashMap<String, String>,
}

impl ComponentHealth {
    /// 创建健康的组件状态
    pub fn healthy(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: HealthStatus::Healthy,
            message: None,
            metadata: HashMap::new(),
        }
    }

    /// 创建不健康的组件状态
    pub fn unhealthy(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: HealthStatus::Unhealthy,
            message: Some(message.into()),
            metadata: HashMap::new(),
        }
    }

    /// 创建降级的组件状态
    pub fn degraded(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: HealthStatus::Degraded,
            message: Some(message.into()),
            metadata: HashMap::new(),
        }
    }
}

/// 健康检查器
pub struct HealthChecker {
    /// 监控指标
    metrics: Arc<CollectorMetrics>,
    /// 各组件检查函数
    component_checkers: Arc<RwLock<HashMap<String, ComponentCheckFn>>>,
}

/// 组件检查函数类型
type ComponentCheckFn = Arc<dyn Fn() -> ComponentHealth + Send + Sync>;

impl HealthChecker {
    /// 创建新的健康检查器
    pub fn new(metrics: Arc<CollectorMetrics>) -> Self {
        Self {
            metrics,
            component_checkers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册组件检查函数
    pub async fn register_checker<F>(&self, name: impl Into<String>, checker: F)
    where
        F: Fn() -> ComponentHealth + Send + Sync + 'static,
    {
        let name = name.into();
        let mut checkers = self.component_checkers.write().await;
        checkers.insert(name.clone(), Arc::new(checker));
        info!("Registered health checker for component: {}", name);
    }

    /// 执行健康检查
    pub async fn check(&self) -> HealthCheckResult {
        let mut components = HashMap::new();
        let mut overall_status = HealthStatus::Healthy;

        let checkers = self.component_checkers.read().await;

        for (name, checker) in checkers.iter() {
            let health = checker();
            self.metrics.update_source_health(name, health.status == HealthStatus::Healthy);

            match health.status {
                HealthStatus::Unhealthy => {
                    overall_status = HealthStatus::Unhealthy;
                }
                HealthStatus::Degraded if overall_status == HealthStatus::Healthy => {
                    overall_status = HealthStatus::Degraded;
                }
                _ => {}
            }

            components.insert(name.clone(), health);
        }

        HealthCheckResult {
            status: overall_status,
            checked_at: Utc::now(),
            components,
        }
    }

    /// 快速检查（只检查关键组件）
    pub async fn quick_check(&self) -> bool {
        let result = self.check().await;
        result.status == HealthStatus::Healthy
    }
}

/// 请求计时器
pub struct RequestTimer {
    start: Instant,
    source: String,
    task_type: String,
    metrics: Arc<CollectorMetrics>,
    success: bool,
}

impl RequestTimer {
    /// 开始计时
    pub fn start(metrics: Arc<CollectorMetrics>, source: impl Into<String>, task_type: impl Into<String>) -> Self {
        let source = source.into();
        let task_type = task_type.into();

        metrics.record_request_start(&source, &task_type);

        Self {
            start: Instant::now(),
            source,
            task_type,
            metrics,
            success: false,
        }
    }

    /// 标记为成功并停止计时
    pub fn succeed(mut self) {
        self.success = true;
        let duration = self.start.elapsed().as_secs_f64();
        self.metrics
            .record_request_success(&self.source, &self.task_type, duration);
        debug!(
            "Request succeeded: {} {} took {:.3}s",
            self.source, self.task_type, duration
        );
    }

    /// 标记为失败并停止计时
    pub fn fail(mut self, error_type: impl Into<String>) {
        self.success = false;
        let error_type = error_type.into();
        self.metrics
            .record_request_failure(&self.source, &self.task_type, &error_type);
        warn!(
            "Request failed: {} {} - {}",
            self.source, self.task_type, error_type
        );
    }
}

impl Drop for RequestTimer {
    fn drop(&mut self) {
        if !self.success {
            // 如果没有显式调用 succeed 或 fail，记录为失败
            self.metrics
                .record_request_failure(&self.source, &self.task_type, "unknown");
        }
    }
}

/// 数据源监控器
pub struct SourceMonitor {
    metrics: Arc<CollectorMetrics>,
    source_stats: Arc<RwLock<HashMap<String, SourceStats>>>,
}

/// 数据源统计信息
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceStats {
    pub total_requests: u64,
    pub success_requests: u64,
    pub failed_requests: u64,
    pub total_duration_ms: u64,
    pub last_check: Option<DateTime<Utc>>,
    pub is_healthy: bool,
}

impl SourceStats {
    pub fn success_rate(&self) -> f64 {
        if self.total_requests == 0 {
            return 1.0;
        }
        self.success_requests as f64 / self.total_requests as f64
    }

    pub fn avg_duration_ms(&self) -> f64 {
        if self.total_requests == 0 {
            return 0.0;
        }
        self.total_duration_ms as f64 / self.total_requests as f64
    }
}

impl SourceMonitor {
    /// 创建新的数据源监控器
    pub fn new(metrics: Arc<CollectorMetrics>) -> Self {
        Self {
            metrics,
            source_stats: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 记录请求成功
    pub async fn record_success(&self, source: &str, duration_ms: u64) {
        let mut stats = self.source_stats.write().await;
        let source_stats = stats.entry(source.to_string()).or_default();

        source_stats.total_requests += 1;
        source_stats.success_requests += 1;
        source_stats.total_duration_ms += duration_ms;
        source_stats.last_check = Some(Utc::now());

        // 更新健康状态（成功率 > 80% 且平均响应时间 < 5秒）
        source_stats.is_healthy = source_stats.success_rate() > 0.8 && source_stats.avg_duration_ms() < 5000.0;

        self.metrics
            .update_source_health(source, source_stats.is_healthy);
    }

    /// 记录请求失败
    pub async fn record_failure(&self, source: &str) {
        let mut stats = self.source_stats.write().await;
        let source_stats = stats.entry(source.to_string()).or_default();

        source_stats.total_requests += 1;
        source_stats.failed_requests += 1;
        source_stats.last_check = Some(Utc::now());

        source_stats.is_healthy = source_stats.success_rate() > 0.8;

        self.metrics
            .update_source_health(source, source_stats.is_healthy);
    }

    /// 获取数据源统计信息
    pub async fn get_stats(&self, source: &str) -> Option<SourceStats> {
        let stats = self.source_stats.read().await;
        stats.get(source).cloned()
    }

    /// 获取所有数据源统计信息
    pub async fn get_all_stats(&self) -> HashMap<String, SourceStats> {
        let stats = self.source_stats.read().await;
        stats.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collector_metrics_creation() {
        let metrics = CollectorMetrics::new();
        assert!(metrics.is_ok());
    }

    #[test]
    fn test_request_timer() {
        let metrics = Arc::new(CollectorMetrics::default());
        let timer = RequestTimer::start(metrics.clone(), "test_source", "test_task");

        // 模拟一些处理
        std::thread::sleep(std::time::Duration::from_millis(10));

        timer.succeed();

        // 验证指标已更新
        let output = metrics.export_prometheus().unwrap();
        assert!(output.contains("collector_requests_total"));
        assert!(output.contains("collector_requests_success_total"));
    }

    #[tokio::test]
    async fn test_health_checker() {
        let metrics = Arc::new(CollectorMetrics::default());
        let checker = HealthChecker::new(metrics);

        checker
            .register_checker("test_component", || ComponentHealth::healthy("test_component"))
            .await;

        let result = checker.check().await;
        assert_eq!(result.status, HealthStatus::Healthy);
        assert!(result.components.contains_key("test_component"));
    }

    #[tokio::test]
    async fn test_source_monitor() {
        let metrics = Arc::new(CollectorMetrics::default());
        let monitor = SourceMonitor::new(metrics);

        monitor.record_success("test_source", 100).await;
        monitor.record_success("test_source", 200).await;
        monitor.record_failure("test_source").await;

        let stats = monitor.get_stats("test_source").await;
        assert!(stats.is_some());

        let stats = stats.unwrap();
        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.success_requests, 2);
        assert_eq!(stats.failed_requests, 1);
    }
}
