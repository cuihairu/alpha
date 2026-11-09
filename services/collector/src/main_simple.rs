//! Alpha Collector Service - 简化版
//!
//! 多语言异步爬虫与数据采集引擎，提供任务调度、限流和状态查询功能
//! 支持 Python、Node.js、Go、Rust、Shell 等多种语言的爬虫执行

use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Instant};

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::{broadcast, RwLock};

use crate::multilang_simple::MultilangCrawler;
use crate::raw_archive::RawArchiver;
use crate::source_health::SourceHealthTracker;
use crate::types::{TaskDefinition, TaskResult, TaskStatus};

const DEFAULT_REDIS_URL: &str = "redis://localhost:6379";
const QUOTES_STREAM: &str = "quotes.raw";

/// 简化版的数据收集器
pub struct SimpleCollector {
    /// 工作空间根目录（用于脚本路径、工作目录等）
    workspace_root: PathBuf,
    /// per-source 序列号计数器（Envelope v2 / 数据质量：sequence 断档检测的
    /// 生产面。进程内单调递增；重启归零由消费面 Regression 语义处理）。
    sequence_counters: Arc<std::sync::Mutex<HashMap<String, u64>>>,
    /// 任务存储（cron_scheduler 只读扫描）
    pub(crate) tasks: Arc<RwLock<HashMap<String, TaskDefinition>>>,
    /// 运行中任务（cron_scheduler 判定重入）
    pub(crate) running_tasks: Arc<RwLock<HashMap<String, TaskStatus>>>,
    /// 多语言爬虫执行器
    crawler: Arc<MultilangCrawler>,
    /// 事件广播
    event_tx: broadcast::Sender<CollectorEvent>,
    /// 原始响应归档器（env 门控，默认 None=关闭）
    raw_archive: Option<RawArchiver>,
    /// 数据源健康台账（L504：执行结果推导三态，/sources/health 暴露）
    health: SourceHealthTracker,
    /// 启动时间（用于 uptime 统计）
    started_at: Instant,
}

/// 收集器事件
///（TaskSubmitted 变体携带完整任务定义，属低频管理面事件，不做 Box 装箱）
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize)]
pub enum CollectorEvent {
    /// 任务提交
    TaskSubmitted {
        task_id: String,
        task: TaskDefinition,
    },
    /// 任务状态更新
    TaskStatusUpdated { task_id: String, status: TaskStatus },
    /// 任务完成
    TaskCompleted { task_id: String, result: TaskResult },
    /// 任务失败
    TaskFailed { task_id: String, error: String },
    /// 系统状态更新
    SystemStatus {
        status: String,
        timestamp: DateTime<Utc>,
    },
}

mod executor;
mod routes;
#[cfg(test)]
mod tests;

pub use routes::build_router;

/// cron 调度执行面：直接复用真实执行链路（execute_task），cron 派发与
/// HTTP POST /tasks/:id/execute 走同一条执行路径
#[async_trait::async_trait]
impl crate::cron_scheduler::TaskRunner for SimpleCollector {
    async fn run(&self, task_id: &str) -> Result<String, String> {
        self.execute_task(task_id).await
    }
}
