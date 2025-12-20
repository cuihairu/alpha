//! 多语言数据收集调度器
//!
//! 提供智能的任务调度、负载均衡和资源管理功能
//! 支持基于语言特性和资源需求的任务分配

use std::{
    collections::{BinaryHeap, HashMap},
    cmp::Reverse,
    sync::Arc,
    time::Instant,
};
use tokio::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Utc};
use tracing::{debug, error, info, warn};
use tokio::{
    sync::{mpsc, RwLock},
};
use uuid::Uuid;

use crate::multilang_simple::{CrawlerConfig, CrawlerLanguage, MultilangCrawler};
use crate::types::{
    TaskDefinition, TaskPriority, TaskResult, TaskSource, TaskStatus,
};

/// 任务调度器
pub struct TaskScheduler {
    /// 任务队列（按优先级排序）
    task_queue: Arc<RwLock<BinaryHeap<Reverse<TaskPriorityNode>>>>,
    /// 等待调度的任务
    pending_tasks: Arc<RwLock<HashMap<String, TaskDefinition>>>,
    /// 正在执行的任务
    running_tasks: Arc<RwLock<HashMap<String, RunningTaskInfo>>>,
    /// 任务依赖关系
    task_dependencies: Arc<RwLock<HashMap<String, Vec<String>>>>,
    /// 语言资源池
    language_pools: Arc<RwLock<HashMap<CrawlerLanguage, LanguageResourcePool>>>,
    /// 调度配置
    config: SchedulerConfig,
    /// 采集器实例
    crawler: Arc<MultilangCrawler>,
    /// 统计信息
    stats: Arc<RwLock<SchedulerStats>>,
    /// 任务结果发送器
    result_tx: mpsc::UnboundedSender<TaskResult>,
    /// 关闭信号
    shutdown_tx: Option<mpsc::UnboundedSender<()>>,
}

/// 优先级任务节点
#[derive(Debug, Clone)]
struct TaskPriorityNode {
    task_id: String,
    priority: TaskPriority,
    weight: u8,
    submit_time: DateTime<Utc>,
}

impl Ord for TaskPriorityNode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // 首先按优先级排序（高优先级在前）
        match other.priority.cmp(&self.priority) {
            std::cmp::Ordering::Equal => {
                // 相同优先级按权重排序
                match other.weight.cmp(&self.weight) {
                    std::cmp::Ordering::Equal => {
                        // 相同权重按提交时间排序（早提交在前）
                        self.submit_time.cmp(&other.submit_time)
                    }
                    other => other,
                }
            }
            other => other,
        }
    }
}

impl PartialOrd for TaskPriorityNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for TaskPriorityNode {
    fn eq(&self, other: &Self) -> bool {
        self.task_id == other.task_id
    }
}

impl Eq for TaskPriorityNode {}

/// 运行中任务信息
#[derive(Debug, Clone)]
struct RunningTaskInfo {
    task_id: String,
    language: CrawlerLanguage,
    start_time: Instant,
    worker_id: String,
    resource_requirements: ResourceRequirements,
}

/// 资源需求
#[derive(Debug, Clone)]
struct ResourceRequirements {
    /// CPU需求（1-10）
    cpu_cores: u8,
    /// 内存需求（MB）
    memory_mb: u64,
    /// 网络带宽需求（Mbps）
    bandwidth_mbps: f64,
    /// 磁盘IO需求
    disk_io: bool,
}

/// 语言资源池
#[derive(Debug, Clone)]
struct LanguageResourcePool {
    language: CrawlerLanguage,
    /// 最大并发数
    max_concurrent: usize,
    /// 当前运行数
    current_running: usize,
    /// 语言特性权重
    language_weights: HashMap<String, f64>,
    /// 可用性检查
    is_available: bool,
}

/// 调度器配置
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// 最大并发任务数
    max_concurrent_tasks: usize,
    /// 调度间隔
    scheduling_interval: Duration,
    /// 任务超时时间
    default_task_timeout: Duration,
    /// 资源限制
    resource_limits: ResourceLimits,
    /// 负载均衡策略
    load_balancing_strategy: LoadBalancingStrategy,
    /// 语言优先级
    language_priorities: Vec<CrawlerLanguage>,
}

/// 资源限制
#[derive(Debug, Clone)]
struct ResourceLimits {
    /// 总CPU核心数
    total_cpu_cores: u8,
    /// 总内存（MB）
    total_memory_mb: u64,
    /// 总带宽（Mbps）
    total_bandwidth_mbps: f64,
}

/// 负载均衡策略
#[derive(Debug, Clone)]
pub enum LoadBalancingStrategy {
    /// 轮询
    RoundRobin,
    /// 最少连接
    LeastConnections,
    /// 加权轮询
    WeightedRoundRobin,
    /// 基于资源使用率
    ResourceBased,
    /// 语言优先级
    LanguagePriority,
}

/// 调度器统计信息
#[derive(Debug, Clone, Default)]
struct SchedulerStats {
    /// 总调度次数
    total_scheduled: u64,
    /// 成功执行次数
    successful_executions: u64,
    /// 失败执行次数
    failed_executions: u64,
    /// 平均调度延迟
    avg_scheduling_latency: Duration,
    /// 资源利用率
    resource_utilization: ResourceUtilization,
}

/// 资源利用率
#[derive(Debug, Clone, Default)]
struct ResourceUtilization {
    cpu_usage: f64,
    memory_usage: f64,
    bandwidth_usage: f64,
}

impl TaskScheduler {
    /// 创建新的任务调度器
    pub fn new(
        config: SchedulerConfig,
        crawler: Arc<MultilangCrawler>,
        result_tx: mpsc::UnboundedSender<TaskResult>,
    ) -> Self {
        let language_pools = Self::create_language_pools(&config);

        Self {
            task_queue: Arc::new(RwLock::new(BinaryHeap::new())),
            pending_tasks: Arc::new(RwLock::new(HashMap::new())),
            running_tasks: Arc::new(RwLock::new(HashMap::new())),
            task_dependencies: Arc::new(RwLock::new(HashMap::new())),
            language_pools: Arc::new(RwLock::new(language_pools)),
            config,
            crawler,
            stats: Arc::new(RwLock::new(SchedulerStats::default())),
            result_tx,
            shutdown_tx: None,
        }
    }

    /// 启动调度器
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting task scheduler...");

        // 初始化采集器（如果需要的话）
        // self.crawler.initialize().await?;

        let (shutdown_tx, mut shutdown_rx) = mpsc::unbounded_channel();
        self.shutdown_tx = Some(shutdown_tx);

        let scheduler_handle = tokio::spawn({
            let config = self.config.clone();
            let task_queue = Arc::clone(&self.task_queue);
            let pending_tasks = Arc::clone(&self.pending_tasks);
            let running_tasks = Arc::clone(&self.running_tasks);
            let task_dependencies = Arc::clone(&self.task_dependencies);
            let language_pools = Arc::clone(&self.language_pools);
            let crawler = Arc::clone(&self.crawler);
            let stats = Arc::clone(&self.stats);
            let result_tx = self.result_tx.clone();

            async move {
                let mut scheduling_interval = tokio::time::interval(config.scheduling_interval);

                loop {
                    tokio::select! {
                        _ = scheduling_interval.tick() => {
                            if let Err(e) = Self::schedule_tasks(
                                &task_queue,
                                &pending_tasks,
                                &running_tasks,
                                &task_dependencies,
                                &language_pools,
                                &crawler,
                                &result_tx,
                                &config,
                                &stats,
                            ).await {
                                error!("Error in scheduling loop: {}", e);
                            }
                        }
                        _ = shutdown_rx.recv() => {
                            info!("Scheduler received shutdown signal");
                            break;
                        }
                    }
                }
            }
        });

        info!("Task scheduler started successfully");
        Ok(())
    }

    /// 停止调度器
    pub async fn shutdown(&mut self) -> Result<()> {
        info!("Shutting down task scheduler...");

        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }

        // 等待所有运行中的任务完成
        let running_count = {
            let running_tasks = self.running_tasks.read().await;
            running_tasks.len()
        };

        if running_count > 0 {
            info!("Waiting for {} running tasks to complete...", running_count);
            tokio::time::sleep(Duration::from_secs(30)).await;
        }

        info!("Task scheduler shutdown completed");
        Ok(())
    }

    /// 提交任务
    pub async fn submit_task(&self, task: TaskDefinition) -> Result<()> {
        debug!("Submitting task: {}", task.id);

        // 检查依赖关系
        if !self.check_dependencies(&task).await? {
            return Err(anyhow::anyhow!("Task dependencies not satisfied: {}", task.id));
        }

        // 添加到待处理队列
        {
            let mut pending = self.pending_tasks.write().await;
            pending.insert(task.id.clone(), task.clone());
        }

        // 添加到优先级队列
        {
            let mut queue = self.task_queue.write().await;
            queue.push(Reverse(TaskPriorityNode {
                task_id: task.id.clone(),
                priority: task.priority.clone(),
                weight: task.weight(),
                submit_time: Utc::now(),
            }));
        }

        // 更新依赖关系
        if !task.dependencies.is_empty() {
            let mut dependencies = self.task_dependencies.write().await;
            dependencies.insert(task.id.clone(), task.dependencies.clone());
        }

        info!("Task submitted successfully: {}", task.id);
        Ok(())
    }

    /// 取消任务
    pub async fn cancel_task(&self, task_id: &str) -> Result<bool> {
        debug!("Cancelling task: {}", task_id);

        // 从待处理队列中移除
        let removed_from_pending = {
            let mut pending = self.pending_tasks.write().await;
            pending.remove(task_id).is_some()
        };

        // 从运行中任务中移除
        let removed_from_running = {
            let running = self.running_tasks.read().await;
            running.contains_key(task_id)
        };

        if removed_from_pending || removed_from_running {
            info!("Task cancelled successfully: {}", task_id);
            Ok(true)
        } else {
            warn!("Task not found for cancellation: {}", task_id);
            Ok(false)
        }
    }

    /// 获取任务状态
    pub async fn get_task_status(&self, task_id: &str) -> Option<TaskStatus> {
        // 检查待处理队列
        {
            let pending = self.pending_tasks.read().await;
            if let Some(_task) = pending.get(task_id) {
                return Some(TaskStatus::Pending);
            }
        }

        // 检查运行中任务
        {
            let running = self.running_tasks.read().await;
            if let Some(_) = running.get(task_id) {
                return Some(TaskStatus::Running);
            }
        }

        None
    }

    /// 获取统计信息
    pub async fn get_stats(&self) -> SchedulerStats {
        let stats = self.stats.read().await;
        stats.clone()
    }

    /// 创建语言资源池
    fn create_language_pools(_config: &SchedulerConfig) -> HashMap<CrawlerLanguage, LanguageResourcePool> {
        let mut pools = HashMap::new();

        // 定义每种语言的资源配置
        let language_configs = vec![
            (CrawlerLanguage::Python, 4, 1024),      // 4并发, 1GB内存
            (CrawlerLanguage::NodeJs, 6, 512),         // 6并发, 512MB内存
            (CrawlerLanguage::Go, 8, 256),             // 8并发, 256MB内存
            (CrawlerLanguage::Rust, 2, 2048),          // 2并发, 2GB内存（编译需要更多内存）
            (CrawlerLanguage::Shell, 10, 128),          // 10并发, 128MB内存
        ];

        for (language, max_concurrent, _memory_mb) in language_configs {
            let language_clone = language.clone();
            pools.insert(language_clone.clone(), LanguageResourcePool {
                language: language_clone,
                max_concurrent,
                current_running: 0,
                language_weights: Self::create_language_weights(&language),
                is_available: true,
            });
        }

        pools
    }

    /// 创建语言权重映射
    fn create_language_weights(language: &CrawlerLanguage) -> HashMap<String, f64> {
        match language {
            CrawlerLanguage::Python => {
                let mut weights = HashMap::new();
                weights.insert("html_parsing".to_string(), 0.9);
                weights.insert("json_processing".to_string(), 0.9);
                weights.insert("data_analysis".to_string(), 0.95);
                weights.insert("web_scraping".to_string(), 0.9);
                weights.insert("async_processing".to_string(), 0.8);
                weights
            }
            CrawlerLanguage::NodeJs => {
                let mut weights = HashMap::new();
                weights.insert("api_calls".to_string(), 0.95);
                weights.insert("json_processing".to_string(), 0.95);
                weights.insert("real_time".to_string(), 0.9);
                weights.insert("websocket".to_string(), 0.9);
                weights.insert("javascript_heavy".to_string(), 1.0);
                weights
            }
            CrawlerLanguage::Go => {
                let mut weights = HashMap::new();
                weights.insert("high_concurrency".to_string(), 0.95);
                weights.insert("network_io".to_string(), 0.9);
                weights.insert("performance".to_string(), 0.9);
                weights.insert("binary_protocols".to_string(), 0.85);
                weights.insert("low_latency".to_string(), 0.9);
                weights
            }
            CrawlerLanguage::Rust => {
                let mut weights = HashMap::new();
                weights.insert("data_processing".to_string(), 0.95);
                weights.insert("memory_safety".to_string(), 1.0);
                weights.insert("high_performance".to_string(), 0.9);
                weights.insert("complex_algorithms".to_string(), 0.9);
                weights.insert("wasm_compatible".to_string(), 1.0);
                weights
            }
            CrawlerLanguage::Shell => {
                let mut weights = HashMap::new();
                weights.insert("system_operations".to_string(), 0.95);
                weights.insert("file_operations".to_string(), 0.9);
                weights.insert("process_management".to_string(), 0.85);
                weights.insert("script_execution".to_string(), 0.9);
                weights.insert("quick_tasks".to_string(), 0.95);
                weights
            }
        }
    }

    /// 调度任务的主循环
    async fn schedule_tasks(
        task_queue: &Arc<RwLock<BinaryHeap<Reverse<TaskPriorityNode>>>>,
        pending_tasks: &Arc<RwLock<HashMap<String, TaskDefinition>>>,
        running_tasks: &Arc<RwLock<HashMap<String, RunningTaskInfo>>>,
        task_dependencies: &Arc<RwLock<HashMap<String, Vec<String>>>>,
        language_pools: &Arc<RwLock<HashMap<CrawlerLanguage, LanguageResourcePool>>>,
        crawler: &Arc<MultilangCrawler>,
        result_tx: &mpsc::UnboundedSender<TaskResult>,
        config: &SchedulerConfig,
        stats: &Arc<RwLock<SchedulerStats>>,
    ) -> Result<()> {
        let start_time = Instant::now();

        // 获取可执行的任务
        let executable_tasks = Self::get_executable_tasks(
            task_queue,
            pending_tasks,
            running_tasks,
            task_dependencies,
        ).await?;

        if executable_tasks.is_empty() {
            return Ok(());
        }

        debug!("Found {} executable tasks", executable_tasks.len());

        // 为每个任务选择最佳语言并执行
        let mut scheduled_count = 0;
        for task_node in executable_tasks {
            if scheduled_count >= config.max_concurrent_tasks {
                break;
            }

            // 获取任务定义
            let task = {
                let pending = pending_tasks.read().await;
                match pending.get(&task_node.task_id) {
                    Some(task) => task.clone(),
                    None => continue,
                }
            };

            // 选择最佳语言
            if let Ok((language, crawler_config)) = Self::select_best_language(
                &task,
                language_pools,
                &config.load_balancing_strategy,
            ).await {
                // 检查资源可用性
                if Self::check_resource_availability(&task, language_pools, &language).await? {
                    // 执行任务
                    let task_id = task.id.clone();
                    let worker_id = Uuid::new_v4().to_string();

                    Self::execute_task(
                        task,
                        language,
                        crawler_config,
                        crawler.clone(),
                        result_tx.clone(),
                        running_tasks.clone(),
                        language_pools.clone(),
                        worker_id,
                    ).await;

                    scheduled_count += 1;
                } else {
                    debug!("No available resources for task: {}", task_node.task_id);
                }
            } else {
                warn!("No suitable language found for task: {}", task_node.task_id);
            }
        }

        // 更新统计信息
        {
            let mut stats_guard = stats.write().await;
            stats_guard.total_scheduled += scheduled_count as u64;
            let scheduling_latency = start_time.elapsed();
            stats_guard.avg_scheduling_latency =
                (stats_guard.avg_scheduling_latency + scheduling_latency) / 2;
        }

        if scheduled_count > 0 {
            info!("Scheduled {} tasks in {:?}", scheduled_count, start_time.elapsed());
        }

        Ok(())
    }

    /// 获取可执行的任务
    async fn get_executable_tasks(
        task_queue: &Arc<RwLock<BinaryHeap<Reverse<TaskPriorityNode>>>>,
        pending_tasks: &Arc<RwLock<HashMap<String, TaskDefinition>>>,
        running_tasks: &Arc<RwLock<HashMap<String, RunningTaskInfo>>>,
        task_dependencies: &Arc<RwLock<HashMap<String, Vec<String>>>>,
    ) -> Result<Vec<TaskPriorityNode>> {
        let mut executable_tasks = Vec::new();
        let mut queue_guard = task_queue.write().await;
        let dependencies_guard = task_dependencies.read().await;
        let running_guard = running_tasks.read().await;

        // 检查最多10个任务
        for _ in 0..10 {
            if let Some(Reverse(task_node)) = queue_guard.pop() {
                // 检查任务是否仍在待处理状态
                let pending_guard = pending_tasks.read().await;
                if !pending_guard.contains_key(&task_node.task_id) {
                    continue;
                }

                // 检查依赖是否满足
                if let Some(deps) = dependencies_guard.get(&task_node.task_id) {
                    let deps_satisfied = deps.iter().all(|dep| {
                        !running_guard.contains_key(dep) && !pending_guard.contains_key(dep)
                    });

                    if !deps_satisfied {
                        // 依赖未满足，重新放回队列
                        queue_guard.push(Reverse(task_node));
                        continue;
                    }
                }

                executable_tasks.push(task_node);
            } else {
                break;
            }
        }

        // 将未检查的任务重新放回队列
        for _ in executable_tasks.len()..queue_guard.len() {
            if let Some(Reverse(task_node)) = queue_guard.pop() {
                queue_guard.push(Reverse(task_node));
            }
        }

        Ok(executable_tasks)
    }

    /// 选择最佳执行语言
    async fn select_best_language(
        task: &TaskDefinition,
        language_pools: &Arc<RwLock<HashMap<CrawlerLanguage, LanguageResourcePool>>>,
        strategy: &LoadBalancingStrategy,
    ) -> Result<(CrawlerLanguage, CrawlerConfig)> {
        let pools = language_pools.read().await;
        let available_pools: Vec<_> = pools
            .values()
            .filter(|pool| pool.is_available && pool.current_running < pool.max_concurrent)
            .collect();

        if available_pools.is_empty() {
            return Err(anyhow::anyhow!("No available language pools"));
        }

        let selected_pool = match strategy {
            LoadBalancingStrategy::LeastConnections => {
                available_pools
                    .iter()
                    .min_by_key(|pool| pool.current_running)
                    .unwrap()
            }
            LoadBalancingStrategy::WeightedRoundRobin => {
                // 简化的加权选择
                available_pools
                    .iter()
                    .max_by_key(|pool| pool.max_concurrent - pool.current_running)
                    .unwrap()
            }
            LoadBalancingStrategy::ResourceBased => {
                // 基于资源使用率选择
                available_pools
                    .iter()
                    .max_by(|a, b| {
                        let utilization_a = a.current_running as f64 / a.max_concurrent as f64;
                        let utilization_b = b.current_running as f64 / b.max_concurrent as f64;
                        let score_a = (1.0 - utilization_a) * 100.0;
                        let score_b = (1.0 - utilization_b) * 100.0;
                        score_a.partial_cmp(&score_b).unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .unwrap()
            }
            LoadBalancingStrategy::LanguagePriority => {
                // 基于预设的语言优先级选择
                available_pools
                    .iter()
                    .find(|pool| matches!(pool.language, CrawlerLanguage::Python))
                    .or_else(|| available_pools.iter().find(|pool| matches!(pool.language, CrawlerLanguage::Go)))
                    .unwrap_or_else(|| available_pools.first().unwrap())
            }
            LoadBalancingStrategy::RoundRobin => {
                available_pools.first().unwrap()
            }
        };

        // 创建爬虫配置
        let crawler_config = Self::create_crawler_config(task, &selected_pool.language).await?;

        Ok((selected_pool.language.clone(), crawler_config))
    }

    /// 创建爬虫配置
    async fn create_crawler_config(
        task: &TaskDefinition,
        language: &CrawlerLanguage,
    ) -> Result<CrawlerConfig> {
        Ok(CrawlerConfig {
            language: language.clone(),
            script_path: Some(format!("scripts/{}/{}.{}",
                task.source.source_type(),
                task.id,
                language.extension()).into()),
            inline_code: None,
            working_directory: Some(format!("workspaces/{}", task.id).into()),
            environment: task.config.request.headers.clone(),
            timeout: Some(task.timeout.unwrap_or(300)),
            arguments: vec![],
        })
    }

    /// 检查资源可用性
    async fn check_resource_availability(
        _task: &TaskDefinition,
        language_pools: &Arc<RwLock<HashMap<CrawlerLanguage, LanguageResourcePool>>>,
        language: &CrawlerLanguage,
    ) -> Result<bool> {
        let pools = language_pools.read().await;

        if let Some(pool) = pools.get(language) {
            Ok(pool.current_running < pool.max_concurrent && pool.is_available)
        } else {
            Ok(false)
        }
    }

    /// 执行单个任务
    async fn execute_task(
        task: TaskDefinition,
        language: CrawlerLanguage,
        crawler_config: CrawlerConfig,
        crawler: Arc<MultilangCrawler>,
        result_tx: mpsc::UnboundedSender<TaskResult>,
        running_tasks: Arc<RwLock<HashMap<String, RunningTaskInfo>>>,
        language_pools: Arc<RwLock<HashMap<CrawlerLanguage, LanguageResourcePool>>>,
        worker_id: String,
    ) {
        let task_id = task.id.clone();

        // 更新运行状态
        {
            let mut running = running_tasks.write().await;
            running.insert(task_id.clone(), RunningTaskInfo {
                task_id: task_id.clone(),
                language: language.clone(),
                start_time: Instant::now(),
                worker_id,
                resource_requirements: ResourceRequirements {
                    cpu_cores: 2,
                    memory_mb: 512,
                    bandwidth_mbps: 10.0,
                    disk_io: true,
                },
            });
        }

        // 更新语言池计数
        {
            let mut pools = language_pools.write().await;
            if let Some(pool) = pools.get_mut(&language) {
                pool.current_running += 1;
            }
        }

        // 执行任务
        let result = crawler.execute_crawler(&task, &crawler_config).await;

        // 清理运行状态
        {
            let mut running = running_tasks.write().await;
            running.remove(&task_id);
        }

        // 更新语言池计数
        {
            let mut pools = language_pools.write().await;
            if let Some(pool) = pools.get_mut(&language) {
                pool.current_running = pool.current_running.saturating_sub(1);
            }
        }

        // 发送结果
        let task_result = match result {
            Ok(data) => TaskResult {
                task_id: task.id,
                status: TaskStatus::Completed,
                data: Some(serde_json::to_string(&data).unwrap_or_default()),
                error: None,
                executed_at: Utc::now(),
            },
            Err(error) => TaskResult {
                task_id: task.id,
                status: TaskStatus::Failed,
                data: None,
                error: Some(error.to_string()),
                executed_at: Utc::now(),
            },
        };
        let _ = result_tx.send(task_result).expect("Failed to send task result");
    }

    /// 检查任务依赖
    async fn check_dependencies(&self, task: &TaskDefinition) -> Result<bool> {
        if task.dependencies.is_empty() {
            return Ok(true);
        }

        let running = self.running_tasks.read().await;
        let pending = self.pending_tasks.read().await;

        for dep in &task.dependencies {
            if running.contains_key(dep) || pending.contains_key(dep) {
                return Ok(false);
            }
        }

        Ok(true)
    }
}

// 为TaskSource添加source_type方法
impl TaskSource {
    pub fn source_type(&self) -> String {
        match self {
            TaskSource::AShare { .. } => "ashare".to_string(),
            TaskSource::HKShare { .. } => "hkshare".to_string(),
            TaskSource::USShare { .. } => "usshare".to_string(),
            TaskSource::Cryptocurrency { .. } => "cryptocurrency".to_string(),
            TaskSource::Forex { .. } => "forex".to_string(),
            TaskSource::Commodities { .. } => "commodities".to_string(),
            TaskSource::Bonds { .. } => "bonds".to_string(),
            TaskSource::Funds { .. } => "funds".to_string(),
            TaskSource::Futures { .. } => "futures".to_string(),
            TaskSource::News { .. } => "news".to_string(),
            TaskSource::SocialMedia { .. } => "social_media".to_string(),
            TaskSource::Announcements { .. } => "announcements".to_string(),
            TaskSource::FinancialReports { .. } => "financials".to_string(),
            TaskSource::ESGData { .. } => "esg".to_string(),
            TaskSource::ResearchReports { .. } => "research".to_string(),
            TaskSource::EconomicIndicators { .. } => "economic".to_string(),
            TaskSource::Custom { source_type, .. } => source_type.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_task_priority_node_ordering() {
        let high_priority = TaskPriorityNode {
            task_id: "task1".to_string(),
            priority: TaskPriority::High,
            weight: 4,
            submit_time: Utc::now(),
        };

        let low_priority = TaskPriorityNode {
            task_id: "task2".to_string(),
            priority: TaskPriority::Low,
            weight: 2,
            submit_time: Utc::now(),
        };

        // 高优先级应该排在前面
        assert!(high_priority > low_priority);
    }

    #[test]
    fn test_language_pool_creation() {
        let config = SchedulerConfig {
            max_concurrent_tasks: 10,
            scheduling_interval: Duration::from_secs(1),
            default_task_timeout: Duration::from_secs(300),
            resource_limits: ResourceLimits {
                total_cpu_cores: 8,
                total_memory_mb: 16384,
                total_bandwidth_mbps: 1000.0,
            },
            load_balancing_strategy: LoadBalancingStrategy::LeastConnections,
            language_priorities: vec![
                CrawlerLanguage::Python,
                CrawlerLanguage::Go,
                CrawlerLanguage::NodeJs,
            ],
        };

        let pools = TaskScheduler::create_language_pools(&config);
        assert!(pools.contains_key(&CrawlerLanguage::Python));
        assert!(pools.contains_key(&CrawlerLanguage::Go));
        assert!(pools.contains_key(&CrawlerLanguage::NodeJs));

        let python_pool = pools.get(&CrawlerLanguage::Python).unwrap();
        assert_eq!(python_pool.max_concurrent, 4);
        assert_eq!(python_pool.current_running, 0);
    }

    #[test]
    fn test_load_balancing_strategies() {
        // 测试负载均衡策略的创建
        let strategies = vec![
            LoadBalancingStrategy::RoundRobin,
            LoadBalancingStrategy::LeastConnections,
            LoadBalancingStrategy::WeightedRoundRobin,
            LoadBalancingStrategy::ResourceBased,
            LoadBalancingStrategy::LanguagePriority,
        ];

        for strategy in strategies {
            // 确保所有策略都能创建
            match strategy {
                LoadBalancingStrategy::RoundRobin => {},
                LoadBalancingStrategy::LeastConnections => {},
                LoadBalancingStrategy::WeightedRoundRobin => {},
                LoadBalancingStrategy::ResourceBased => {},
                LoadBalancingStrategy::LanguagePriority => {},
            }
        }
    }
}