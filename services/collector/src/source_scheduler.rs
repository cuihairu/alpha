//! 数据源任务调度器模块
//!
//! 负责任务调度、并发控制和优先级管理

use crate::sources::{DataSource, KlineType, RealtimeQuote};
use crate::cleaner::DataCleaner;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock, Semaphore};
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

/// 任务优先级
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SourceTaskPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Urgent = 3,
}

impl Default for SourceTaskPriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// 任务状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScheduledTaskStatus {
    /// 等待执行
    Pending,
    /// 正在执行
    Running,
    /// 已完成
    Completed,
    /// 失败
    Failed,
    /// 已取消
    Cancelled,
}

/// 爬虫任务类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SourceTaskType {
    /// 获取实时行情
    RealtimeQuote { symbols: Vec<String> },
    /// 获取 K线数据
    KlineData { symbol: String, kline_type: KlineType, limit: usize },
    /// 获取股票列表
    StockList,
    /// 健康检查
    HealthCheck,
}

/// 爬虫任务
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceTask {
    /// 任务ID
    pub id: String,
    /// 任务类型
    pub task_type: SourceTaskType,
    /// 任务优先级
    pub priority: SourceTaskPriority,
    /// 任务状态
    pub status: ScheduledTaskStatus,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 开始时间
    pub started_at: Option<DateTime<Utc>>,
    /// 完成时间
    pub completed_at: Option<DateTime<Utc>>,
    /// 重试次数
    pub retry_count: usize,
    /// 最大重试次数
    pub max_retries: usize,
    /// 错误信息
    pub error: Option<String>,
    /// 元数据
    pub metadata: HashMap<String, String>,
}

impl SourceTask {
    /// 创建新任务
    pub fn new(task_type: SourceTaskType, priority: SourceTaskPriority) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            task_type,
            priority,
            status: ScheduledTaskStatus::Pending,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            retry_count: 0,
            max_retries: 3,
            error: None,
            metadata: HashMap::new(),
        }
    }

    /// 获取执行耗时（毫秒）
    pub fn execution_time_ms(&self) -> Option<i64> {
        if let (Some(started), Some(completed)) = (self.started_at, self.completed_at) {
            Some((completed - started).num_milliseconds())
        } else {
            None
        }
    }

    /// 是否可以重试
    pub fn can_retry(&self) -> bool {
        self.retry_count < self.max_retries && self.status == ScheduledTaskStatus::Failed
    }
}

/// 任务执行结果
#[derive(Debug, Clone)]
pub enum SourceTaskResult {
    RealtimeQuotes(Vec<RealtimeQuote>),
    Empty,
}

/// 调度器配置
#[derive(Debug, Clone)]
pub struct SourceSchedulerConfig {
    /// 最大并发任务数
    pub max_concurrent_tasks: usize,
    /// 任务队列最大长度
    pub max_queue_size: usize,
    /// 任务超时时间（秒）
    pub task_timeout: u64,
    /// 任务重试间隔（毫秒）
    pub retry_interval: u64,
    /// 是否启用数据清洗
    pub enable_cleaning: bool,
}

impl Default for SourceSchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_tasks: 10,
            max_queue_size: 1000,
            task_timeout: 30,
            retry_interval: 1000,
            enable_cleaning: true,
        }
    }
}

/// 任务队列
struct SourceTaskQueue {
    /// 高优先级队列
    urgent: VecDeque<SourceTask>,
    /// 高优先级队列
    high: VecDeque<SourceTask>,
    /// 普通优先级队列
    normal: VecDeque<SourceTask>,
    /// 低优先级队列
    low: VecDeque<SourceTask>,
}

impl SourceTaskQueue {
    fn new() -> Self {
        Self {
            urgent: VecDeque::new(),
            high: VecDeque::new(),
            normal: VecDeque::new(),
            low: VecDeque::new(),
        }
    }

    /// 添加任务
    fn push(&mut self, task: SourceTask) -> Result<(), String> {
        match task.priority {
            SourceTaskPriority::Urgent => self.urgent.push_back(task),
            SourceTaskPriority::High => self.high.push_back(task),
            SourceTaskPriority::Normal => self.normal.push_back(task),
            SourceTaskPriority::Low => self.low.push_back(task),
        }
        Ok(())
    }

    /// 取出下一个任务
    fn pop(&mut self) -> Option<SourceTask> {
        if !self.urgent.is_empty() {
            self.urgent.pop_front()
        } else if !self.high.is_empty() {
            self.high.pop_front()
        } else if !self.normal.is_empty() {
            self.normal.pop_front()
        } else if !self.low.is_empty() {
            self.low.pop_front()
        } else {
            None
        }
    }

    /// 队列长度
    fn len(&self) -> usize {
        self.urgent.len() + self.high.len() + self.normal.len() + self.low.len()
    }

    fn remove_by_id(&mut self, task_id: &str) -> bool {
        for queue in [&mut self.urgent, &mut self.high, &mut self.normal, &mut self.low] {
            if let Some(pos) = queue.iter().position(|task| task.id == task_id) {
                queue.remove(pos);
                return true;
            }
        }
        false
    }

    /// 清空所有队列
    fn clear(&mut self) {
        self.urgent.clear();
        self.high.clear();
        self.normal.clear();
        self.low.clear();
    }
}

/// 爬虫调度器
pub struct SourceScheduler {
    /// 配置
    config: SourceSchedulerConfig,
    /// 任务队列
    queue: Arc<Mutex<SourceTaskQueue>>,
    /// 并发控制信号量
    semaphore: Arc<Semaphore>,
    /// 数据源集合
    sources: Vec<Arc<dyn DataSource>>,
    /// 数据清洗器
    cleaner: Arc<Mutex<DataCleaner>>,
    /// 任务执行状态
    running_tasks: Arc<RwLock<HashMap<String, SourceTask>>>,
    /// 统计信息
    stats: Arc<RwLock<SourceSchedulerStats>>,
}

/// 调度器统计信息
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SourceSchedulerStats {
    /// 总任务数
    pub total_tasks: u64,
    /// 成功任务数
    pub success_tasks: u64,
    /// 失败任务数
    pub failed_tasks: u64,
    /// 当前运行任务数
    pub running_tasks: u64,
    /// 平均执行时间（毫秒）
    pub avg_execution_time_ms: u64,
    /// 最后更新时间
    pub last_updated: Option<DateTime<Utc>>,
}

impl SourceScheduler {
    /// 创建新的调度器
    pub fn new(
        config: SourceSchedulerConfig,
        sources: Vec<Arc<dyn DataSource>>,
        cleaner: DataCleaner,
    ) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_tasks));

        Self {
            config,
            queue: Arc::new(Mutex::new(SourceTaskQueue::new())),
            semaphore,
            sources,
            cleaner: Arc::new(Mutex::new(cleaner)),
            running_tasks: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(SourceSchedulerStats::default())),
        }
    }

    /// 使用默认配置创建调度器
    pub fn with_defaults(sources: Vec<Arc<dyn DataSource>>) -> Self {
        Self::new(
            SourceSchedulerConfig::default(),
            sources,
            DataCleaner::with_default_rules(),
        )
    }

    /// 添加任务
    pub async fn submit_task(&self, task: SourceTask) -> Result<(), String> {
        let mut queue = self.queue.lock().await;

        if queue.len() >= self.config.max_queue_size {
            return Err("Task queue is full".to_string());
        }

        queue.push(task.clone())?;
        info!("Task submitted: {} (priority: {:?})", task.id, task.priority);

        // 更新统计
        let mut stats = self.stats.write().await;
        stats.total_tasks += 1;
        stats.last_updated = Some(Utc::now());

        Ok(())
    }

    /// 批量添加任务
    pub async fn submit_tasks(&self, tasks: Vec<SourceTask>) -> Result<(), String> {
        for task in tasks {
            self.submit_task(task).await?;
        }
        Ok(())
    }

    /// 启动调度器
    pub async fn start(&self) {
        info!("Source scheduler started");

        loop {
            // 获取下一个任务
            let task = {
                let mut queue = self.queue.lock().await;
                queue.pop()
            };

            if let Some(task) = task {
                // 执行任务
                self.execute_task(task).await;
            } else {
                // 队列为空，等待一段时间
                sleep(Duration::from_millis(100)).await;
            }
        }
    }

    /// 执行单个任务
    async fn execute_task(&self, mut task: SourceTask) {
        // 获取信号量许可
        let permit = match self.semaphore.try_acquire() {
            Ok(permit) => permit,
            Err(_) => {
                // 并发数已达上限，重新放回队列
                warn!("Max concurrent tasks reached, re-queueing task: {}", task.id);
                let _ = self.submit_task(task).await;
                return;
            }
        };

        // 更新任务状态
        task.status = ScheduledTaskStatus::Running;
        task.started_at = Some(Utc::now());

        let task_id = task.id.clone();
        let task_type = task.task_type.clone();

        // 记录运行中的任务
        {
            let mut running = self.running_tasks.write().await;
            running.insert(task_id.clone(), task.clone());
        }

        // 更新统计
        {
            let mut stats = self.stats.write().await;
            stats.running_tasks += 1;
        }

        debug!("Executing task: {} ({:?})", task_id, task_type);

        // 执行任务
        let result = tokio::time::timeout(
            Duration::from_secs(self.config.task_timeout),
            self.do_execute_task(&task),
        ).await;

        // 任务完成
        let (status, error) = match result {
            Ok(Ok(_)) => {
                info!("Task completed successfully: {}", task_id);
                (ScheduledTaskStatus::Completed, None)
            }
            Ok(Err(e)) => {
                error!("Task failed: {} - {}", task_id, e);
                (ScheduledTaskStatus::Failed, Some(e.to_string()))
            }
            Err(_) => {
                error!("Task timeout: {}", task_id);
                (ScheduledTaskStatus::Failed, Some("Task timeout".to_string()))
            }
        };

        // 更新任务状态
        task.status = status.clone();
        task.completed_at = Some(Utc::now());
        task.error = error.clone();

        // 如果失败且可以重试，重新加入队列
        if status == ScheduledTaskStatus::Failed && task.can_retry() {
            task.retry_count += 1;
            task.status = ScheduledTaskStatus::Pending;
            task.started_at = None;
            task.completed_at = None;

            sleep(Duration::from_millis(self.config.retry_interval)).await;

            if let Err(e) = self.submit_task(task.clone()).await {
                error!("Failed to re-queue task {}: {}", task_id, e);
            }
        }

        // 更新运行任务列表
        {
            let mut running = self.running_tasks.write().await;
            running.remove(&task_id);
        }

        // 更新统计
        {
            let mut stats = self.stats.write().await;
            stats.running_tasks -= 1;
            match status {
                ScheduledTaskStatus::Completed => stats.success_tasks += 1,
                ScheduledTaskStatus::Failed => stats.failed_tasks += 1,
                _ => {}
            }

            // 更新平均执行时间
            if let Some(exec_time) = task.execution_time_ms() {
                let total = stats.success_tasks + stats.failed_tasks;
                stats.avg_execution_time_ms =
                    ((stats.avg_execution_time_ms * (total - 1) as u64) + exec_time as u64) / total as u64;
            }

            stats.last_updated = Some(Utc::now());
        }

        // 释放信号量许可
        drop(permit);
    }

    /// 实际执行任务
    async fn do_execute_task(&self, task: &SourceTask) -> Result<SourceTaskResult, Box<dyn std::error::Error + Send + Sync>> {
        // 获取可用的数据源
        let source = self.get_best_source().await?;

        match &task.task_type {
            SourceTaskType::RealtimeQuote { symbols } => {
                let quotes = if source.supports_batch() {
                    source.get_realtime_quotes(symbols).await?
                } else {
                    let mut all_quotes = Vec::new();
                    for symbol in symbols {
                        let quote = source.get_realtime_quote(symbol).await?;
                        all_quotes.push(quote);
                    }
                    all_quotes
                };

                // 数据清洗
                if self.config.enable_cleaning {
                    let mut cleaner = self.cleaner.lock().await;
                    let cleaned = cleaner.clean_realtime_quotes(quotes);
                    let valid_quotes: Vec<_> = cleaned.into_iter()
                        .filter_map(|r| r.data)
                        .collect();
                    Ok(SourceTaskResult::RealtimeQuotes(valid_quotes))
                } else {
                    Ok(SourceTaskResult::RealtimeQuotes(quotes))
                }
            }
            SourceTaskType::KlineData { symbol, kline_type, limit } => {
                let _klines = source.get_kline(symbol, *kline_type, *limit).await?;
                Ok(SourceTaskResult::Empty)
            }
            SourceTaskType::StockList => {
                let _list = source.get_stock_list(None).await?;
                Ok(SourceTaskResult::Empty)
            }
            SourceTaskType::HealthCheck => {
                source.health_check().await?;
                Ok(SourceTaskResult::Empty)
            }
        }
    }

    /// 获取最佳数据源
    async fn get_best_source(&self) -> Result<Arc<dyn DataSource>, Box<dyn std::error::Error + Send + Sync>> {
        if self.sources.is_empty() {
            return Err("No data sources available".into());
        }

        // 简单策略：返回优先级最高的数据源
        let best = self.sources.iter()
            .min_by_key(|s| s.priority())
            .ok_or("No data sources available")?;

        Ok(Arc::clone(best))
    }

    /// 获取统计信息
    pub async fn get_stats(&self) -> SourceSchedulerStats {
        let stats = self.stats.read().await;
        stats.clone()
    }

    /// 获取队列长度
    pub async fn queue_length(&self) -> usize {
        let queue = self.queue.lock().await;
        queue.len()
    }

    /// 获取运行中的任务
    pub async fn get_running_tasks(&self) -> Vec<SourceTask> {
        let running = self.running_tasks.read().await;
        running.values().cloned().collect()
    }

    /// 取消任务
    pub async fn cancel_task(&self, task_id: &str) -> Result<(), String> {
        // 检查运行中的任务
        {
            let mut running = self.running_tasks.write().await;
            if let Some(task) = running.get_mut(task_id) {
                task.status = ScheduledTaskStatus::Cancelled;
                return Ok(());
            }
        }

        // 检查队列中的任务
        let mut queue = self.queue.lock().await;
        if queue.remove_by_id(task_id) {
            info!("Task cancelled from queue: {}", task_id);
            return Ok(());
        }

        Err("Task not found".to_string())
    }

    /// 清空任务队列
    pub async fn clear_queue(&self) {
        let mut queue = self.queue.lock().await;
        queue.clear();
        info!("Task queue cleared");
    }
}

/// 任务生成器
pub struct SourceTaskGenerator;

impl SourceTaskGenerator {
    /// 生成实时行情任务
    pub fn realtime_quotes(symbols: Vec<String>, priority: SourceTaskPriority) -> SourceTask {
        SourceTask::new(
            SourceTaskType::RealtimeQuote { symbols },
            priority,
        )
    }

    /// 生成 K线任务
    pub fn kline_data(symbol: String, kline_type: KlineType, limit: usize) -> SourceTask {
        SourceTask::new(
            SourceTaskType::KlineData { symbol, kline_type, limit },
            SourceTaskPriority::Normal,
        )
    }

    /// 生成健康检查任务
    pub fn health_check() -> SourceTask {
        SourceTask::new(
            SourceTaskType::HealthCheck,
            SourceTaskPriority::Low,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::{CrawlerConfig, EastmoneySource, SinaSource};

    #[tokio::test]
    async fn test_scheduler() {
        let sources: Vec<Arc<dyn DataSource>> = vec![
            Arc::new(EastmoneySource::new(CrawlerConfig::default())),
            Arc::new(SinaSource::new(CrawlerConfig::default())),
        ];

        let scheduler = SourceScheduler::with_defaults(sources);

        // 添加任务
        let task = SourceTaskGenerator::realtime_quotes(
            vec!["000001".to_string(), "600000".to_string()],
            SourceTaskPriority::High,
        );

        scheduler.submit_task(task).await.unwrap();

        // 检查队列长度
        assert_eq!(scheduler.queue_length().await, 1);
    }

    #[test]
    fn test_task_queue() {
        let mut queue = SourceTaskQueue::new();

        let task1 = SourceTask::new(
            SourceTaskType::HealthCheck,
            SourceTaskPriority::Low,
        );

        let task2 = SourceTask::new(
            SourceTaskType::HealthCheck,
            SourceTaskPriority::High,
        );

        queue.push(task1).unwrap();
        queue.push(task2).unwrap();

        // 高优先级任务应该先出队
        let next = queue.pop().unwrap();
        assert_eq!(next.priority, SourceTaskPriority::High);
    }
}
