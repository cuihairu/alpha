//! 采集执行链：任务执行、脚本生成、Redis 发布与数据源健康

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use alpha_storage::{RedisStreamQueue, StreamEnvelope};
use chrono::Utc;
use tokio::{
    sync::{broadcast, RwLock},
    time::interval,
};
use tracing::{error, info};
use uuid::Uuid;

use super::routes::{
    map_crawler_error, NewTaskRequest, PublishQuotesResponse, TaskResponse, TaskStats,
};
use super::{CollectorEvent, SimpleCollector, DEFAULT_REDIS_URL, QUOTES_STREAM};
use crate::multilang_simple::{CrawlerConfig, CrawlerLanguage, MultilangCrawler};
use crate::raw_archive::RawArchiver;
use crate::source_health::{health_state, SourceHealthEntry, SourceHealthTracker};
use crate::sources::{CrawlerConfig as SourceCrawlerConfig, DataSource, EastmoneySource};
use crate::types::{
    AShareDataSource, HKShareDataSource, NewsDataSource, ParserConfig, RequestConfig, RetryPolicy,
    StorageConfig, TaskConfig, TaskDefinition, TaskPriority, TaskSource, TaskStatus,
    USShareDataSource,
};

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
            health: SourceHealthTracker::new(),
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
    pub(crate) fn next_source_sequence(&self, source: &str) -> u64 {
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
                    // 数据源健康（L504）：解析产出成功 = 源健康，清零连败
                    self.health.record_success(task_id);
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
                    self.health.record_failure(task_id, &error);
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
                // 数据源健康（L504）：执行器级失败同样计入连败
                self.health.record_failure(task_id, &e.to_string());

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
    pub(crate) fn parse_priority(&self, priority: &Option<String>) -> TaskPriority {
        match priority.as_ref().map(|s| s.as_str()) {
            Some("critical") => TaskPriority::Critical,
            Some("high") => TaskPriority::High,
            Some("low") => TaskPriority::Low,
            Some("background") => TaskPriority::Background,
            _ => TaskPriority::Medium,
        }
    }

    /// 为任务选择最佳语言
    pub(crate) fn select_language_for_task(&self, task: &TaskDefinition) -> CrawlerLanguage {
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

    /// 生成脚本里的字符串字面量：JSON 字面量在 Python/JS/Go 均为合法字符串字面量，
    /// 引号/反斜杠/控制字符一律转义。URL 经此收口，杜绝爬虫源码注入（安全扫描 S-1）。
    fn script_json_literal(s: &str) -> String {
        serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
    }

    /// shell 单引号字面量：`'…'` 内不能含单引号，用 `'\''` 转义（S-1 收口）。
    fn script_shell_literal(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\\''"))
    }

    /// 生成脚本代码
    pub(crate) fn generate_script_code(
        &self,
        task: &TaskDefinition,
        language: &CrawlerLanguage,
    ) -> String {
        match language {
            CrawlerLanguage::Python => {
                format!(
                    r#"
	import json
	import urllib.request
	import urllib.error
	from datetime import datetime, timezone

	def main():
	    url = {}
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
                    Self::script_json_literal(&task.config.request.url),
                    serde_json::to_string(&task.config.request.headers)
                        .unwrap_or_else(|_| "{}".to_string())
                )
            }
            CrawlerLanguage::NodeJs => {
                format!(
                    r#"
const https = require('https');
const fs = require('fs');
const url = {};
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
                    Self::script_json_literal(&task.config.request.url)
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
    url := {}

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
                    Self::script_json_literal(&task.config.request.url)
                )
            }
            CrawlerLanguage::Shell => {
                let header_args = task
                    .config
                    .request
                    .headers
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "-H {}",
                            Self::script_shell_literal(&format!("{}: {}", k, v))
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                format!(
                    r#"
#!/bin/bash

URL={}

echo "Fetching data from: $URL"

# 使用curl获取数据
if command -v curl >/dev/null 2>&1; then
    curl -s {} "$URL" 2>/dev/null || echo "Error: Failed to fetch data"
else
    echo "Error: curl command not found"
fi
"#,
                    Self::script_shell_literal(&task.config.request.url),
                    header_args
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
        // 健康台账同摘（残留计数会让 /sources/health 虚高）
        self.health.remove(task_id);
        Ok(())
    }

    /// 数据源健康快照（L504）：任务表 ∪ 执行台账——从未执行的任务以
    /// unknown 呈现，健康面是「所有已登记任务的健康视图」而非只有跑过的
    pub async fn get_sources_health(&self) -> Vec<serde_json::Value> {
        let tasks = self.tasks.read().await;
        let ledger = self.health.snapshot();
        let mut task_ids: Vec<&String> = tasks.keys().chain(ledger.keys()).collect();
        task_ids.sort();
        task_ids.dedup();

        task_ids
            .into_iter()
            .map(|task_id| {
                let entry = ledger.get(task_id);
                let state = health_state(entry);
                let SourceHealthEntry {
                    consecutive_failures,
                    last_success_at,
                    last_failure_at,
                    last_error,
                } = entry.cloned().unwrap_or_default();
                serde_json::json!({
                    "task_id": task_id,
                    "state": state,
                    "consecutive_failures": consecutive_failures,
                    "last_success_at": last_success_at,
                    "last_failure_at": last_failure_at,
                    "last_error": last_error,
                })
            })
            .collect()
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
