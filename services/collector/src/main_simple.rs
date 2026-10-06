//! Alpha Collector Service - 简化版
//!
//! 多语言异步爬虫与数据采集引擎，提供任务调度、限流和状态查询功能
//! 支持 Python、Node.js、Go、Rust、Shell 等多种语言的爬虫执行

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use alpha_storage::{RedisStreamQueue, StreamEnvelope};
use axum::{
    body::Body,
    extract::State,
    http::StatusCode,
    middleware::Next,
    response::{
        sse::{Event as SseEvent, KeepAlive, Sse},
        IntoResponse, Json, Response,
    },
    routing::{get, post},
    Router,
};
use chrono::{DateTime, Utc};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use tokio::{
    sync::{broadcast, RwLock},
    time::interval,
};
use tokio_stream::{
    wrappers::{errors::BroadcastStreamRecvError, BroadcastStream},
    StreamExt,
};
use tracing::{debug, error, info};
use uuid::Uuid;

use crate::multilang_simple::{CrawlerConfig, CrawlerLanguage, MultilangCrawler};
use crate::raw_archive::RawArchiver;
use crate::sources::{
    CrawlerConfig as SourceCrawlerConfig, CrawlerError, DataSource, EastmoneySource,
};
use crate::types::{
    AShareDataSource, HKShareDataSource, NewsDataSource, ParserConfig, RequestConfig, RetryPolicy,
    StorageConfig, TaskConfig, TaskDefinition, TaskPriority, TaskResult, TaskSource, TaskStatus,
    USShareDataSource,
};

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

/// 新建任务请求
#[derive(Debug, Deserialize)]
pub struct NewTaskRequest {
    /// 任务ID
    pub id: Option<String>,
    /// 任务名称
    pub name: String,
    /// 任务类型
    pub source_type: String,
    /// 数据源URL
    pub url: String,
    /// HTTP方法
    pub method: Option<String>,
    /// 请求头
    pub headers: Option<HashMap<String, String>>,
    /// 请求体
    pub body: Option<String>,
    /// 优先级
    pub priority: Option<String>,
    /// 调度表达式
    pub schedule: Option<String>,
    /// 超时时间（秒）
    pub timeout: Option<u64>,
    /// 最大重试次数
    pub max_retries: Option<u32>,
    /// 语言选择
    pub language: Option<String>,
    /// 标签
    pub tags: Option<Vec<String>>,
}

/// 任务响应
#[derive(Debug, Serialize)]
pub struct TaskResponse {
    /// 任务ID
    pub task_id: String,
    /// 状态
    pub status: String,
    /// 消息
    pub message: Option<String>,
    /// 创建时间
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct PublishQuotesRequest {
    pub symbols: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct PublishQuotesResponse {
    pub stream: String,
    pub requested: usize,
    pub published: usize,
}

/// 健康检查响应
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// 服务状态
    pub status: String,
    /// 版本
    pub version: String,
    /// 运行时间
    pub uptime_seconds: u64,
    /// 任务统计
    pub task_stats: TaskStats,
}

/// 任务统计
#[derive(Debug, Serialize)]
pub struct TaskStats {
    /// 总任务数
    pub total: usize,
    /// 运行中
    pub running: usize,
    /// 已完成
    pub completed: usize,
    /// 失败
    pub failed: usize,
}

impl SimpleCollector {
    /// 创建新的简化收集器
    pub fn new<P: AsRef<std::path::Path>>(workspace_root: P) -> Self {
        let workspace_root = workspace_root.as_ref().to_path_buf();
        let (event_tx, _) = broadcast::channel(1000);

        Self {
            workspace_root: workspace_root.clone(),
            sequence_counters: Arc::new(std::sync::Mutex::new(HashMap::new())),
            tasks: Arc::new(RwLock::new(HashMap::new())),
            running_tasks: Arc::new(RwLock::new(HashMap::new())),
            crawler: Arc::new(MultilangCrawler::new(&workspace_root)),
            raw_archive: None,
            event_tx,
            started_at: Instant::now(),
        }
    }

    /// 挂载原始响应归档器（env 装配的 None = 关闭）。独立 builder 而非并进
    /// new()：配置错误要在 main 启动期响亮退出（连接串非法不静默降级），
    /// 而测试构造保持零参数零行为变化
    pub fn with_raw_archiver(mut self, archiver: Option<RawArchiver>) -> Self {
        self.raw_archive = archiver;
        self
    }

    /// 取某数据源的下一条 sequence（per-source 单调递增，从 1 起）。
    /// 进程内计数；重启归零由消费端 SequenceGapMonitor 的 Regression 语义处理。
    fn next_source_sequence(&self, source: &str) -> u64 {
        let mut counters = self
            .sequence_counters
            .lock()
            .expect("sequence counter mutex poisoned");
        let counter = counters.entry(source.to_string()).or_insert(0);
        *counter += 1;
        *counter
    }

    /// 启动收集器服务
    pub async fn start(&self) -> anyhow::Result<()> {
        info!("Starting simple collector service...");

        // 初始化多语言爬虫
        self.crawler.initialize().await?;

        // 启动系统状态广播
        let event_tx = self.event_tx.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                let event = CollectorEvent::SystemStatus {
                    status: "running".to_string(),
                    timestamp: Utc::now(),
                };
                let _ = event_tx.send(event);
            }
        });

        info!("Simple collector service started successfully");
        Ok(())
    }

    /// 提交新任务
    pub async fn submit_task(&self, request: NewTaskRequest) -> Result<TaskResponse, String> {
        let task_id = request.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let now = Utc::now();

        // 解析任务来源
        let source = self
            .parse_task_source(&request.source_type, &request.url)
            .await?;

        // 创建任务定义
        let task = TaskDefinition {
            id: task_id.clone(),
            source,
            name: request.name.clone(),
            description: None,
            schedule: request.schedule,
            priority: self.parse_priority(&request.priority),
            config: TaskConfig {
                request: RequestConfig {
                    method: request.method.unwrap_or_else(|| "GET".to_string()),
                    url: request.url.clone(),
                    headers: request.headers.unwrap_or_default(),
                    params: HashMap::new(),
                    body: request.body,
                    proxy: None,
                    user_agents: vec!["Mozilla/5.0 (compatible; AlphaCollector/1.0)".to_string()],
                    request_interval: 1000,
                    retry_interval: 5000,
                },
                parser: ParserConfig {
                    parser_type: crate::types::ParserType::JSON,
                    rules: vec![],
                    data_format: crate::types::DataFormat::JSON,
                    field_mapping: HashMap::new(),
                },
                storage: StorageConfig {
                    storage_type: crate::types::StorageType::Memory,
                    target: "default".to_string(),
                    table: None,
                    batch_size: Some(100),
                    compression: None,
                },
                notification: None,
            },
            retry_policy: RetryPolicy {
                max_retries: request.max_retries.unwrap_or(3),
                base_delay: 1000,
                max_delay: 60000,
                backoff_strategy: crate::types::BackoffStrategy::ExponentialWithJitter,
                retry_conditions: vec![
                    crate::types::RetryCondition::HttpError(vec![500, 502, 503, 504]),
                    crate::types::RetryCondition::NetworkError,
                    crate::types::RetryCondition::TimeoutError,
                ],
            },
            timeout: request.timeout,
            dependencies: vec![],
            tags: request.tags.unwrap_or_default(),
            status: TaskStatus::Pending,
            created_at: now,
            updated_at: now,
        };

        // 添加到任务存储
        {
            let mut tasks = self.tasks.write().await;
            tasks.insert(task_id.clone(), task.clone());
        }

        // 发送事件
        let event = CollectorEvent::TaskSubmitted {
            task_id: task_id.clone(),
            task,
        };
        let _ = self.event_tx.send(event);

        info!("Task submitted: {}", task_id);

        Ok(TaskResponse {
            task_id,
            status: "submitted".to_string(),
            message: Some("Task submitted successfully".to_string()),
            created_at: now,
        })
    }

    /// 从 YAML/JSON 任务模板（文件或目录）批量登记任务（architecture §24 任务描述）
    ///
    /// 返回 `(已登记, 已跳过)`——`enabled: false` 的模板校验通过但不登记；
    /// 每个登记的任务与 HTTP API 提交路径同事件面（广播 TaskSubmitted）。
    pub async fn submit_task_templates<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<(usize, usize), String> {
        let templates =
            crate::task_templates::load_path(path.as_ref()).map_err(|e| e.to_string())?;
        let mut submitted = 0;
        let mut skipped = 0;
        for template in templates {
            if !template.enabled {
                skipped += 1;
                continue;
            }
            let source = self
                .parse_task_source(&template.source_type, &template.url)
                .await?;
            let task = template.into_task_definition(source)?;
            let task_id = task.id.clone();
            {
                let mut tasks = self.tasks.write().await;
                tasks.insert(task_id.clone(), task.clone());
            }
            let event = CollectorEvent::TaskSubmitted {
                task_id: task_id.clone(),
                task,
            };
            let _ = self.event_tx.send(event);
            info!("Task submitted from template: {}", task_id);
            submitted += 1;
        }
        Ok((submitted, skipped))
    }

    /// 执行任务
    pub async fn execute_task(&self, task_id: &str) -> Result<String, String> {
        // 获取任务
        let task = {
            let tasks = self.tasks.read().await;
            tasks.get(task_id).cloned()
        };

        let task = match task {
            Some(task) => task,
            None => return Err("Task not found".to_string()),
        };

        // 更新任务状态为运行中
        {
            let mut running = self.running_tasks.write().await;
            running.insert(task_id.to_string(), TaskStatus::Running);
        }
        {
            let mut tasks = self.tasks.write().await;
            if let Some(existing) = tasks.get_mut(task_id) {
                existing.status = TaskStatus::Running;
                existing.updated_at = Utc::now();
            }
        }

        let status_event = CollectorEvent::TaskStatusUpdated {
            task_id: task_id.to_string(),
            status: TaskStatus::Running,
        };
        let _ = self.event_tx.send(status_event);

        let working_directory = PathBuf::from(format!("workspaces/{}", task_id));
        let working_directory_abs = self.workspace_root.join(&working_directory);
        if let Err(e) = tokio::fs::create_dir_all(&working_directory_abs).await {
            return Err(format!("Failed to create working directory: {}", e));
        }

        // 选择执行语言
        let language = self.select_language_for_task(&task);

        // 创建爬虫配置：A 股默认使用本地 Python 爬虫脚本；其余任务走 inline_code（零依赖实现）
        let crawler_config = match &task.source {
            TaskSource::AShare { symbols, .. } => CrawlerConfig {
                language: CrawlerLanguage::Python,
                script_path: Some(PathBuf::from("crawlers/python/eastmoney_quote.py")),
                inline_code: None,
                working_directory: Some(working_directory),
                environment: HashMap::new(),
                timeout: task.timeout,
                arguments: vec!["--symbols".to_string(), symbols.join(",")],
            },
            _ => CrawlerConfig {
                language: language.clone(),
                script_path: None,
                inline_code: Some(self.generate_script_code(&task, &language)),
                working_directory: Some(working_directory),
                environment: HashMap::new(),
                timeout: task.timeout,
                arguments: vec![],
            },
        };

        // 执行任务
        match self.crawler.execute_crawler(&task, &crawler_config).await {
            Ok(result) => {
                // 原始响应归档（旁路，architecture-review §2.1 MinIO/S3 原始归档）：
                // 脚本 best-effort 落在工作目录的 raw_response.txt/raw_meta.json
                // 上传对象存储——成功/失败都只走指标与日志，绝不影响任务状态
                // （采集可用性优先于归档完整性）；失败任务的 raw 同样归档，
                // 那正是排查数据源问题的证据
                if let Some(archiver) = &self.raw_archive {
                    match archiver.archive_task(task_id, &working_directory_abs).await {
                        Ok(0) => {}
                        Ok(n) => {
                            metrics::counter!("alpha_collector_raw_archive_total",
                                "result" => "uploaded")
                            .increment(n as u64);
                            info!(task_id, count = n, "raw response archived");
                        }
                        Err(e) => {
                            metrics::counter!("alpha_collector_raw_archive_total",
                                "result" => "failed")
                            .increment(1);
                            tracing::warn!(
                                task_id,
                                error = %e,
                                "raw archive failed (旁路，不影响任务状态)"
                            );
                        }
                    }
                }

                {
                    let mut running = self.running_tasks.write().await;
                    running.insert(task_id.to_string(), result.status.clone());
                }
                {
                    let mut tasks = self.tasks.write().await;
                    if let Some(existing) = tasks.get_mut(task_id) {
                        existing.status = result.status.clone();
                        existing.updated_at = Utc::now();
                    }
                }

                if result.status == TaskStatus::Completed {
                    let _ = self.event_tx.send(CollectorEvent::TaskCompleted {
                        task_id: task_id.to_string(),
                        result,
                    });
                    info!("Task {} completed successfully", task_id);
                    Ok("Task completed successfully".to_string())
                } else {
                    let error = result
                        .error
                        .clone()
                        .unwrap_or_else(|| "crawler execution failed".to_string());
                    let _ = self.event_tx.send(CollectorEvent::TaskFailed {
                        task_id: task_id.to_string(),
                        error,
                    });
                    Err("Task failed".to_string())
                }
            }
            Err(e) => {
                // 更新状态为失败
                {
                    let mut running = self.running_tasks.write().await;
                    running.insert(task_id.to_string(), TaskStatus::Failed);
                }
                {
                    let mut tasks = self.tasks.write().await;
                    if let Some(existing) = tasks.get_mut(task_id) {
                        existing.status = TaskStatus::Failed;
                        existing.updated_at = Utc::now();
                    }
                }

                let failed_event = CollectorEvent::TaskFailed {
                    task_id: task_id.to_string(),
                    error: e.to_string(),
                };
                let _ = self.event_tx.send(failed_event);

                error!("Task {} failed: {}", task_id, e);
                Err(e.to_string())
            }
        }
    }

    /// 启动 cron 调度（architecture §24 刷新频率执行面）：每秒扫描 `schedule`
    /// 非空的任务，到期且非运行中即派发执行。需要 `Arc<Self>` 供后台任务
    /// 持 'static 引用；表达式解析在 CronDispatcher 内懒缓存。
    pub async fn start_cron_scheduler(self: Arc<Self>) {
        let dispatcher = std::sync::Arc::new(crate::cron_scheduler::CronDispatcher::new(
            Arc::clone(&self.tasks),
            Arc::clone(&self.running_tasks),
            self,
        ));
        tokio::spawn(async move {
            dispatcher
                .run_forever(std::time::Duration::from_secs(1))
                .await;
        });
        info!("Cron scheduler started (1s tick)");
    }

    pub async fn publish_realtime_quotes(
        &self,
        symbols: &[String],
    ) -> Result<PublishQuotesResponse, String> {
        if symbols.is_empty() {
            return Err("symbols cannot be empty".to_string());
        }

        let quotes = self.fetch_quotes(symbols).await?;
        let redis_url = std::env::var("ALPHA_REDIS_URL")
            .or_else(|_| std::env::var("REDIS_URL"))
            .unwrap_or_else(|_| DEFAULT_REDIS_URL.to_string());
        let queue = RedisStreamQueue::connect(&redis_url).map_err(|e| e.to_string())?;

        let mut published = 0usize;
        for quote in quotes {
            // Envelope v2（architecture-review §3.1/§3.3）：源侧行情时刻上提为
            // event_time；market 标注 cn（A 股采集面）；source_event_id 待数据源
            // 提供对账锚点后接线（当前源无此字段，留缺省）。sequence 为
            // per-source 单调序号（数据质量 §5 P2 断档检测的生产面）。
            let envelope = StreamEnvelope::new(
                QUOTES_STREAM,
                "quote",
                quote.source.clone(),
                Some(quote.symbol.clone()),
                serde_json::to_value(&quote).map_err(|e| e.to_string())?,
            )
            .with_event_time(quote.timestamp)
            .with_market("cn")
            .with_sequence(self.next_source_sequence(&quote.source));
            queue
                .publish(QUOTES_STREAM, &envelope)
                .await
                .map_err(|e| e.to_string())?;
            published += 1;
        }

        Ok(PublishQuotesResponse {
            stream: QUOTES_STREAM.to_string(),
            requested: symbols.len(),
            published,
        })
    }

    async fn fetch_quotes(
        &self,
        symbols: &[String],
    ) -> Result<Vec<crate::sources::RealtimeQuote>, String> {
        let source = EastmoneySource::new(SourceCrawlerConfig::default());
        source
            .get_realtime_quotes(symbols)
            .await
            .map_err(map_crawler_error)
    }

    /// 解析任务来源
    async fn parse_task_source(&self, source_type: &str, url: &str) -> Result<TaskSource, String> {
        match source_type.to_lowercase().as_str() {
            "ashare" | "a-share" => Ok(TaskSource::AShare {
                source: AShareDataSource::EastMoney,
                symbols: vec!["000001".to_string()], // 默认示例
            }),
            "hkshare" | "hk-share" => Ok(TaskSource::HKShare {
                source: HKShareDataSource::HKEX,
                symbols: vec!["00700".to_string()], // 默认示例
            }),
            "usshare" | "us-share" => Ok(TaskSource::USShare {
                source: USShareDataSource::Yahoo,
                symbols: vec!["AAPL".to_string()], // 默认示例
            }),
            "news" => Ok(TaskSource::News {
                sources: vec![NewsDataSource::Sina],
                keywords: vec!["finance".to_string()],
                languages: vec!["zh".to_string()],
            }),
            "custom" => Ok(TaskSource::Custom {
                source_type: source_type.to_string(),
                endpoint: url.to_string(),
                params: HashMap::new(),
            }),
            _ => Err(format!("Unsupported source type: {}", source_type)),
        }
    }

    /// 解析优先级
    fn parse_priority(&self, priority: &Option<String>) -> TaskPriority {
        match priority.as_ref().map(|s| s.as_str()) {
            Some("critical") => TaskPriority::Critical,
            Some("high") => TaskPriority::High,
            Some("low") => TaskPriority::Low,
            Some("background") => TaskPriority::Background,
            _ => TaskPriority::Medium,
        }
    }

    /// 为任务选择最佳语言
    fn select_language_for_task(&self, task: &TaskDefinition) -> CrawlerLanguage {
        match &task.source {
            TaskSource::AShare { .. } => {
                // A股数据采集优先使用Python
                CrawlerLanguage::Python
            }
            TaskSource::News { .. } => {
                // 新闻采集可以使用Python或Node.js
                CrawlerLanguage::NodeJs
            }
            TaskSource::Custom { source_type, .. } => {
                // 根据自定义类型选择语言
                match source_type.as_str() {
                    "api" | "rest" => CrawlerLanguage::Go,
                    "json" | "parsing" => CrawlerLanguage::Python,
                    "javascript" | "js" => CrawlerLanguage::NodeJs,
                    _ => CrawlerLanguage::Python, // 默认
                }
            }
            _ => CrawlerLanguage::Python, // 默认使用Python
        }
    }

    /// 生成脚本代码
    fn generate_script_code(&self, task: &TaskDefinition, language: &CrawlerLanguage) -> String {
        match language {
            CrawlerLanguage::Python => {
                let headers_repr = format!("{:?}", task.config.request.headers);
                format!(
                    r#"
	import json
	import urllib.request
	import urllib.error
	from datetime import datetime, timezone

	def main():
	    url = "{}"
	    headers = {}

	    try:
	        req = urllib.request.Request(url, headers=headers, method="GET")
	        with urllib.request.urlopen(req, timeout=30) as resp:
	            raw_bytes = resp.read()
	            raw = raw_bytes.decode("utf-8", errors="replace")
	            # 原始响应落盘（best-effort，RawArchiver 取证底座）：写不进不改主链路
	            try:
	                with open("raw_response.txt", "wb") as f:
	                    f.write(raw_bytes)
	                with open("raw_meta.json", "w", encoding="utf-8") as f:
	                    json.dump({{"url": resp.geturl(), "status": resp.status,
	                               "byte_len": len(raw_bytes),
	                               "fetched_at": datetime.now(tz=timezone.utc).isoformat()}},
	                              f, ensure_ascii=False)
	            except Exception:
	                pass
	            try:
	                data = json.loads(raw)
	                print(json.dumps(data, ensure_ascii=False, indent=2))
	                return data
	            except Exception:
	                print(raw)
	                return {{"text": raw}}
	    except Exception as e:
	        print(f"Error: {{e}}")
	        return None

	if __name__ == "__main__":
	    main()
	"#,
                    task.config.request.url, headers_repr
                )
            }
            CrawlerLanguage::NodeJs => {
                format!(
                    r#"
const https = require('https');
const fs = require('fs');
const url = '{}';
https.get(url, (res) => {{
    let data = '';
    res.on('data', (chunk) => {{
        data += chunk;
    }});
    res.on('end', () => {{
        // 原始响应落盘（best-effort，RawArchiver 取证底座）：写不进不改主链路
        try {{
            fs.writeFileSync('raw_response.txt', data);
            fs.writeFileSync('raw_meta.json', JSON.stringify({{
                url: url, byte_len: data.length, fetched_at: new Date().toISOString()
            }}));
        }} catch (e) {{}}
        try {{
            const jsonData = JSON.parse(data);
            console.log(JSON.stringify(jsonData, null, 2));
        }} catch (e) {{
            console.error('Error:', e.message);
        }}
    }});
}}).on('error', (err) => {{
    console.error('Error:', err.message);
}});
"#,
                    task.config.request.url
                )
            }
            CrawlerLanguage::Go => {
                format!(
                    r#"
package main

import (
    "encoding/json"
    "fmt"
    "io/ioutil"
    "net/http"
    "time"
)

func main() {{
    url := "{}"

    resp, err := http.Get(url)
    if err != nil {{
        fmt.Printf("Error: %v\n", err)
        return
    }}
    defer resp.Body.Close()

    body, err := ioutil.ReadAll(resp.Body)
    if err != nil {{
        fmt.Printf("Error reading response: %v\n", err)
        return
    }}

    var result interface{{}}
    if err := json.Unmarshal(body, &result); err != nil {{
        fmt.Printf("Error parsing JSON: %v\n", err)
        return
    }}

    output, _ := json.MarshalIndent(result, "", "  ")
    fmt.Println(string(output))
}}
"#,
                    task.config.request.url
                )
            }
            CrawlerLanguage::Shell => {
                format!(
                    r#"
#!/bin/bash

URL="{}"
HEADERS='{}'

echo "Fetching data from: $URL"

# 使用curl获取数据
if command -v curl >/dev/null 2>&1; then
    if [ -n "$HEADERS" ]; then
        curl -s -H "$HEADERS" "$URL" 2>/dev/null || echo "Error: Failed to fetch data"
    else
        curl -s "$URL" 2>/dev/null || echo "Error: Failed to fetch data"
    fi
else
    echo "Error: curl command not found"
fi
"#,
                    task.config.request.url,
                    task.config
                        .request
                        .headers
                        .iter()
                        .map(|(k, v)| format!("-H '{}: {}'", k, v))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            }
            CrawlerLanguage::Rust => {
                format!(
                    r#"
[package]
name = "crawler-{}"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = {{ version = "1", features = ["full"] }}
reqwest = {{ version = "0.11", features = ["json"] }}
serde_json = "1.0"

[[bin]]
name = "main"
path = "main.rs"
"#,
                    task.id
                )
            }
        }
    }

    /// 获取任务状态
    pub async fn get_task_status(&self, task_id: &str) -> Option<TaskStatus> {
        let running = self.running_tasks.read().await;
        running.get(task_id).cloned()
    }

    /// 取消任务（幂等）：清 `schedule`（cron 不再派发）+ status=Cancelled。
    /// 运行中任务的子进程无法中断（crawler 无 kill 句柄）——本次执行会
    /// 跑完，之后不再调度；对不存在的任务报错
    pub async fn cancel_task(&self, task_id: &str) -> Result<String, String> {
        {
            let tasks = self.tasks.read().await;
            if !tasks.contains_key(task_id) {
                return Err("Task not found".to_string());
            }
        }
        {
            let mut tasks = self.tasks.write().await;
            if let Some(existing) = tasks.get_mut(task_id) {
                existing.schedule = None;
                existing.status = TaskStatus::Cancelled;
                existing.updated_at = Utc::now();
            }
        }
        {
            let mut running = self.running_tasks.write().await;
            running.insert(task_id.to_string(), TaskStatus::Cancelled);
        }
        let _ = self.event_tx.send(CollectorEvent::TaskStatusUpdated {
            task_id: task_id.to_string(),
            status: TaskStatus::Cancelled,
        });
        Ok(format!("Task {task_id} cancelled"))
    }

    /// 删除任务：从任务表与状态表移除（状态表残留会让 /tasks/:id 对已删
    /// 任务仍返回状态、/stats 计数虚高）。运行中拒绝——同 cancel 的边界，
    /// crawler 无 kill 句柄，等执行完再删；不存在按幂等成功处理
    pub async fn delete_task(&self, task_id: &str) -> Result<(), String> {
        let running_now = {
            let running = self.running_tasks.read().await;
            running.get(task_id) == Some(&TaskStatus::Running)
        };
        if running_now {
            return Err("Task is running; cancel it and delete after completion".to_string());
        }
        self.tasks.write().await.remove(task_id);
        self.running_tasks.write().await.remove(task_id);
        Ok(())
    }

    /// 获取任务统计
    pub async fn get_task_stats(&self) -> TaskStats {
        let running = self.running_tasks.read().await;
        let tasks = self.tasks.read().await;

        let (completed, failed) =
            running
                .values()
                .fold((0, 0), |(comp, fail), status| match status {
                    TaskStatus::Completed => (comp + 1, fail),
                    TaskStatus::Failed => (comp, fail + 1),
                    _ => (comp, fail),
                });

        TaskStats {
            total: tasks.len(),
            running: running.len(),
            completed,
            failed,
        }
    }

    /// 获取事件接收器
    pub fn subscribe_events(&self) -> broadcast::Receiver<CollectorEvent> {
        self.event_tx.subscribe()
    }
}

/// 进程级唯一 Prometheus 句柄（install_recorder 每进程一次——首个调用者
/// install 全局接管 metrics 宏，后续复用渲染；CollectorMetrics 的宏指标
/// 由此进入 /metrics）
fn global_metrics_handle() -> &'static PrometheusHandle {
    static HANDLE: std::sync::OnceLock<PrometheusHandle> = std::sync::OnceLock::new();
    HANDLE.get_or_init(|| {
        PrometheusBuilder::new()
            .install_recorder()
            .unwrap_or_else(|_| PrometheusBuilder::new().build_recorder().handle())
    })
}

/// 构建路由
pub fn build_router(collector: Arc<SimpleCollector>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/metrics", get(metrics_endpoint))
        .route("/tasks", post(submit_task))
        .route("/tasks/:id", get(get_task_status).delete(delete_task))
        .route("/tasks", get(list_tasks))
        .route("/tasks/:id/cancel", post(cancel_task))
        .route("/tasks/:id/execute", post(execute_task))
        .route("/streams/quotes/publish", post(publish_quotes))
        .route("/stats", get(get_stats))
        .route("/events", get(sse_events))
        .with_state(collector)
        .layer(axum::middleware::from_fn(request_log_middleware))
}

async fn request_log_middleware(request: axum::http::Request<Body>, next: Next) -> Response {
    let method = request.method().to_string();
    let uri = request.uri().to_string();
    debug!("{} {}", method, uri);
    next.run(request).await
}

/// Prometheus 抓取端点（L459）：全局 recorder 快照渲染
async fn metrics_endpoint() -> String {
    global_metrics_handle().render()
}

/// 健康检查端点
async fn health_check(State(collector): State<Arc<SimpleCollector>>) -> impl IntoResponse {
    let stats = collector.get_task_stats().await;

    let response = HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: collector.started_at.elapsed().as_secs(),
        task_stats: stats,
    };

    (StatusCode::OK, Json(response))
}

/// 提交任务端点
async fn submit_task(
    State(collector): State<Arc<SimpleCollector>>,
    Json(request): Json<NewTaskRequest>,
) -> impl IntoResponse {
    match collector.submit_task(request).await {
        Ok(response) => {
            let value = serde_json::to_value(response)
                .unwrap_or_else(|_| serde_json::json!({"error": "failed to serialize response"}));
            (StatusCode::CREATED, Json(value))
        }
        Err(error) => {
            let response = serde_json::json!({
                "error": error
            });
            (StatusCode::BAD_REQUEST, Json(response))
        }
    }
}

/// 获取任务状态端点
async fn get_task_status(
    State(collector): State<Arc<SimpleCollector>>,
    axum::extract::Path(task_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    match collector.get_task_status(&task_id).await {
        Some(status) => {
            let response = serde_json::json!({
                "task_id": task_id,
                "status": format!("{:?}", status)
            });
            (StatusCode::OK, Json(response))
        }
        None => {
            let response = serde_json::json!({
                "error": "Task not found"
            });
            (StatusCode::NOT_FOUND, Json(response))
        }
    }
}

/// 列出所有任务端点
async fn list_tasks(State(collector): State<Arc<SimpleCollector>>) -> impl IntoResponse {
    let tasks = collector.tasks.read().await;
    let task_list: Vec<_> = tasks
        .values()
        .map(|task| {
            serde_json::json!({
                "id": task.id,
                "name": task.name,
                "source": format!("{:?}", task.source),
                "priority": format!("{:?}", task.priority),
                "created_at": task.created_at,
                "status": format!("{:?}", task.status),
            })
        })
        .collect();

    (StatusCode::OK, Json(task_list))
}

/// 执行任务端点
async fn execute_task(
    State(collector): State<Arc<SimpleCollector>>,
    axum::extract::Path(task_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    match collector.execute_task(&task_id).await {
        Ok(message) => {
            let response = serde_json::json!({
                "task_id": task_id,
                "message": message
            });
            (StatusCode::OK, Json(response))
        }
        Err(error) => {
            let response = serde_json::json!({
                "task_id": task_id,
                "error": error
            });
            (StatusCode::INTERNAL_SERVER_ERROR, Json(response))
        }
    }
}

/// 取消任务端点（重试语义归既有 POST /tasks/:id/execute 承担——
/// 对 Failed/Cancelled 任务重跑即 retry，不再造重复端点）
async fn cancel_task(
    State(collector): State<Arc<SimpleCollector>>,
    axum::extract::Path(task_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    match collector.cancel_task(&task_id).await {
        Ok(message) => (
            StatusCode::OK,
            Json(serde_json::json!({ "task_id": task_id, "message": message })),
        ),
        Err(error) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": error })),
        ),
    }
}

/// 删除任务端点：204 幂等删除；409 运行中；404 不会出现（幂等语义）
async fn delete_task(
    State(collector): State<Arc<SimpleCollector>>,
    axum::extract::Path(task_id): axum::extract::Path<String>,
) -> StatusCode {
    match collector.delete_task(&task_id).await {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::CONFLICT,
    }
}

async fn publish_quotes(
    State(collector): State<Arc<SimpleCollector>>,
    Json(request): Json<PublishQuotesRequest>,
) -> impl IntoResponse {
    match collector.publish_realtime_quotes(&request.symbols).await {
        Ok(response) => (
            StatusCode::OK,
            Json(serde_json::to_value(response).unwrap_or_default()),
        ),
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": error })),
        ),
    }
}

/// 获取统计信息端点
async fn get_stats(State(collector): State<Arc<SimpleCollector>>) -> impl IntoResponse {
    let stats = collector.get_task_stats().await;
    (StatusCode::OK, Json(stats))
}

/// SSE事件流端点
async fn sse_events(State(collector): State<Arc<SimpleCollector>>) -> impl IntoResponse {
    let rx = collector.subscribe_events();

    let stream = BroadcastStream::new(rx).filter_map(|msg| match msg {
        Ok(event) => match SseEvent::default().json_data(&event) {
            Ok(evt) => Some(Ok::<SseEvent, Infallible>(evt)),
            Err(err) => {
                tracing::warn!("failed to serialize event: {}", err);
                None
            }
        },
        Err(BroadcastStreamRecvError::Lagged(skipped)) => {
            tracing::warn!("events subscriber lagged by {}", skipped);
            None
        }
    });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

fn map_crawler_error(err: CrawlerError) -> String {
    match err {
        CrawlerError::RequestError(msg) => format!("request error: {}", msg),
        CrawlerError::ParseError(msg) => format!("parse error: {}", msg),
        CrawlerError::SourceError(msg) => format!("source error: {}", msg),
        CrawlerError::RateLimited => "rate limited".to_string(),
        CrawlerError::Timeout => "crawler timeout".to_string(),
        CrawlerError::InvalidData(msg) => format!("invalid data: {}", msg),
    }
}

/// cron 调度执行面：直接复用真实执行链路（execute_task），cron 派发与
/// HTTP POST /tasks/:id/execute 走同一条执行路径
#[async_trait::async_trait]
impl crate::cron_scheduler::TaskRunner for SimpleCollector {
    async fn run(&self, task_id: &str) -> Result<String, String> {
        self.execute_task(task_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_simple_collector_creation() {
        let collector = SimpleCollector::new("/tmp/test_collector");
        assert_eq!(collector.get_task_stats().await.total, 0);
    }

    /// 测试构造：Custom 源 + 可选 schedule（cron 派发条件的取消靶子）
    fn seeded_task(id: &str, schedule: Option<&str>) -> TaskDefinition {
        let mut task = TaskDefinition::new(
            id,
            TaskSource::Custom {
                source_type: "custom".to_string(),
                endpoint: "https://x.io".to_string(),
                params: HashMap::new(),
            },
            id,
        );
        task.schedule = schedule.map(|s| s.to_string());
        task
    }

    /// 取消：清 schedule + status=Cancelled（cron 不再派发），幂等；不存在报错
    #[tokio::test]
    async fn cancel_clears_schedule_marks_cancelled_and_is_idempotent() {
        let collector = SimpleCollector::new("/tmp/test_collector_cancel");
        collector
            .tasks
            .write()
            .await
            .insert("t1".into(), seeded_task("t1", Some("0 30 9 * * 1-5")));

        let message = collector.cancel_task("t1").await.unwrap();
        assert!(message.contains("t1"));
        {
            let tasks = collector.tasks.read().await;
            let task = tasks.get("t1").unwrap();
            assert_eq!(task.status, TaskStatus::Cancelled);
            assert!(task.schedule.is_none(), "schedule 须清空——cron 派发条件");
        }
        assert_eq!(
            collector.get_task_status("t1").await,
            Some(TaskStatus::Cancelled)
        );

        // 幂等：再取消一次仍成功
        assert!(collector.cancel_task("t1").await.is_ok());
        // 不存在的任务报错
        assert_eq!(
            collector.cancel_task("ghost").await.unwrap_err(),
            "Task not found"
        );
    }

    /// 删除：任务表+状态表一起摘（/tasks/:id 对已删任务不再有状态、
    /// /stats 不虚高）；运行中 409；不存在幂等成功
    #[tokio::test]
    async fn delete_removes_task_and_state_rejects_running() {
        let collector = SimpleCollector::new("/tmp/test_collector_delete");
        collector
            .tasks
            .write()
            .await
            .insert("t1".into(), seeded_task("t1", None));
        collector
            .running_tasks
            .write()
            .await
            .insert("t1".into(), TaskStatus::Running);

        // 运行中拒绝
        let err = collector.delete_task("t1").await.unwrap_err();
        assert!(err.contains("running"), "{err}");
        assert!(collector.tasks.read().await.contains_key("t1"));

        // 状态改掉后可删：两表同清
        collector
            .running_tasks
            .write()
            .await
            .insert("t1".into(), TaskStatus::Completed);
        collector.delete_task("t1").await.unwrap();
        assert!(!collector.tasks.read().await.contains_key("t1"));
        assert_eq!(collector.get_task_status("t1").await, None);
        assert_eq!(collector.get_task_stats().await.total, 0);

        // 幂等：删不存在的仍成功
        collector.delete_task("t1").await.unwrap();
    }

    #[test]
    fn test_task_source_parsing() {
        let _collector = SimpleCollector::new("/tmp");

        // This would need to be made async in a real test
        // let result = collector.parse_task_source("ashare", "https://example.com").await;
        // assert!(result.is_ok());
    }

    #[test]
    fn test_sequence_counter_monotonic_per_source() {
        // 数据质量 §5 P2：per-source 单调序号（Envelope v2 sequence 生产面）
        let collector = SimpleCollector::new("/tmp");
        assert_eq!(collector.next_source_sequence("eastmoney"), 1);
        assert_eq!(collector.next_source_sequence("eastmoney"), 2);
        assert_eq!(collector.next_source_sequence("eastmoney"), 3);
    }

    #[test]
    fn test_sequence_counters_independent_across_sources() {
        // 不同 source 的基线互不串扰（与消费端 SequenceGapMonitor 的
        // per-(stream, source) 语义对齐）
        let collector = SimpleCollector::new("/tmp");
        assert_eq!(collector.next_source_sequence("eastmoney"), 1);
        assert_eq!(collector.next_source_sequence("sina"), 1);
        assert_eq!(collector.next_source_sequence("eastmoney"), 2);
        assert_eq!(collector.next_source_sequence("sina"), 2);
    }

    #[test]
    fn test_priority_parsing() {
        let collector = SimpleCollector::new("/tmp");

        assert_eq!(
            collector.parse_priority(&Some("critical".to_string())),
            TaskPriority::Critical
        );
        assert_eq!(
            collector.parse_priority(&Some("high".to_string())),
            TaskPriority::High
        );
        assert_eq!(
            collector.parse_priority(&Some("low".to_string())),
            TaskPriority::Low
        );
        assert_eq!(
            collector.parse_priority(&Some("invalid".to_string())),
            TaskPriority::Medium
        );
        assert_eq!(collector.parse_priority(&None), TaskPriority::Medium);
    }

    #[test]
    fn test_language_selection() {
        let collector = SimpleCollector::new("/tmp");

        let a_share_task = TaskDefinition::new(
            "test-1",
            TaskSource::AShare {
                source: AShareDataSource::Sina,
                symbols: vec!["000001".to_string()],
            },
            "A-Share Task",
        );

        let language = collector.select_language_for_task(&a_share_task);
        assert_eq!(language, CrawlerLanguage::Python);
    }

    #[tokio::test]
    async fn publish_quotes_rejects_empty_symbols() {
        let collector = SimpleCollector::new("/tmp/test_collector");
        let err = collector.publish_realtime_quotes(&[]).await.unwrap_err();
        assert!(err.contains("symbols cannot be empty"));
    }

    #[test]
    fn test_script_code_generation() {
        let collector = SimpleCollector::new("/tmp");

        let task = TaskDefinition::new(
            "test-script",
            TaskSource::Custom {
                source_type: "test".to_string(),
                endpoint: "https://api.example.com".to_string(),
                params: HashMap::new(),
            },
            "Test Script",
        );

        let python_code = collector.generate_script_code(&task, &CrawlerLanguage::Python);
        assert!(python_code.contains("import urllib.request"));
        assert!(python_code.contains(&task.config.request.url));

        let nodejs_code = collector.generate_script_code(&task, &CrawlerLanguage::NodeJs);
        assert!(nodejs_code.contains("require('https')"));
        assert!(nodejs_code.contains(&task.config.request.url));

        let shell_code = collector.generate_script_code(&task, &CrawlerLanguage::Shell);
        assert!(shell_code.contains("curl"));
        assert!(shell_code.contains(&task.config.request.url));
    }
}
