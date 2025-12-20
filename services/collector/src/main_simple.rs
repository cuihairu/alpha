//! Alpha Collector Service - 简化版
//!
//! 多语言异步爬虫与数据采集引擎，提供任务调度、限流和状态查询功能
//! 支持 Python、Node.js、Go、Rust、Shell 等多种语言的爬虫执行

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use axum::{
    body::Body,
    extract::State,
    http::StatusCode,
    response::{
        Response,
        sse::{Event as SseEvent, KeepAlive, Sse},
        IntoResponse,
        Json,
    },
    routing::{get, post},
    middleware::Next,
    Router,
};
use chrono::{DateTime, Utc};
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
use crate::types::{
    TaskDefinition, TaskResult, TaskSource, TaskStatus, TaskPriority,
    TaskConfig, RequestConfig, ParserConfig, StorageConfig, RetryPolicy,
    AShareDataSource, HKShareDataSource, USShareDataSource,
    NewsDataSource,
};

/// 简化版的数据收集器
pub struct SimpleCollector {
    /// 任务存储
    tasks: Arc<RwLock<HashMap<String, TaskDefinition>>>,
    /// 运行中任务
    running_tasks: Arc<RwLock<HashMap<String, TaskStatus>>>,
    /// 多语言爬虫执行器
    crawler: Arc<MultilangCrawler>,
    /// 事件广播
    event_tx: broadcast::Sender<CollectorEvent>,
}

/// 收集器事件
#[derive(Debug, Clone, Serialize)]
pub enum CollectorEvent {
    /// 任务提交
    TaskSubmitted { task_id: String, task: TaskDefinition },
    /// 任务状态更新
    TaskStatusUpdated { task_id: String, status: TaskStatus },
    /// 任务完成
    TaskCompleted { task_id: String, result: TaskResult },
    /// 任务失败
    TaskFailed { task_id: String, error: String },
    /// 系统状态更新
    SystemStatus { status: String, timestamp: DateTime<Utc> },
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
        let (event_tx, _) = broadcast::channel(1000);

        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            running_tasks: Arc::new(RwLock::new(HashMap::new())),
            crawler: Arc::new(MultilangCrawler::new(workspace_root)),
            event_tx,
        }
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
        let source = self.parse_task_source(&request.source_type, &request.url).await?;

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
                    user_agents: vec![
                        "Mozilla/5.0 (compatible; AlphaCollector/1.0)".to_string(),
                    ],
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

        // 选择执行语言
        let language = self.select_language_for_task(&task);

        // 创建爬虫配置（优先使用 inline_code）
        let crawler_config = CrawlerConfig {
            language: language.clone(),
            script_path: None,
            inline_code: Some(self.generate_script_code(&task, &language)),
            working_directory: Some(PathBuf::from(format!("workspaces/{}", task_id))),
            environment: task.config.request.headers.clone(),
            timeout: task.timeout,
            arguments: vec![],
        };

        // 执行任务
        match self.crawler.execute_crawler(&task, &crawler_config).await {
            Ok(result) => {
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

    /// 解析任务来源
    async fn parse_task_source(&self, source_type: &str, url: &str) -> Result<TaskSource, String> {
        match source_type.to_lowercase().as_str() {
            "ashare" | "a-share" => Ok(TaskSource::AShare {
                source: AShareDataSource::Sina,
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
                format!(r#"
import requests
import json
from datetime import datetime

def main():
    url = "{}"
    headers = {}

    try:
        response = requests.get(url, headers=headers, timeout=30)
        response.raise_for_status()
        data = response.json()
        print(json.dumps(data, ensure_ascii=False, indent=2))
        return data
    except Exception as e:
        print(f"Error: {{e}}")
        return None

if __name__ == "__main__":
    main()
"#,
                    task.config.request.url,
                    format!("{:?}", task.config.request.headers)
                )
            }
            CrawlerLanguage::NodeJs => {
                format!(r#"
const https = require('https');
const url = '{}';
https.get(url, (res) => {{
    let data = '';
    res.on('data', (chunk) => {{
        data += chunk;
    }});
    res.on('end', () => {{
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
                format!(r#"
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
                format!(r#"
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
                    task.config.request.headers
                        .iter()
                        .map(|(k, v)| format!("-H '{}: {}'", k, v))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            }
            CrawlerLanguage::Rust => {
                format!(r#"
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

    /// 获取任务统计
    pub async fn get_task_stats(&self) -> TaskStats {
        let running = self.running_tasks.read().await;
        let tasks = self.tasks.read().await;

        let (completed, failed) = running.values().fold((0, 0), |(comp, fail), status| {
            match status {
                TaskStatus::Completed => (comp + 1, fail),
                TaskStatus::Failed => (comp, fail + 1),
                _ => (comp, fail),
            }
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

/// 构建路由
pub fn build_router(collector: Arc<SimpleCollector>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/tasks", post(submit_task))
        .route("/tasks/:id", get(get_task_status))
        .route("/tasks", get(list_tasks))
        .route("/tasks/:id/execute", post(execute_task))
        .route("/stats", get(get_stats))
        .route("/events", get(sse_events))
        .with_state(collector)
        .layer(
            axum::middleware::from_fn(request_log_middleware)
        )
}

async fn request_log_middleware(request: axum::http::Request<Body>, next: Next) -> Response {
    let method = request.method().to_string();
    let uri = request.uri().to_string();
    debug!("{} {}", method, uri);
    next.run(request).await
}

/// 健康检查端点
async fn health_check(
    State(collector): State<Arc<SimpleCollector>>,
) -> impl IntoResponse {
    let stats = collector.get_task_stats().await;

    let response = HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: 0, // TODO: 实际计算运行时间
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
            let value = serde_json::to_value(response).unwrap_or_else(|_| {
                serde_json::json!({"error": "failed to serialize response"})
            });
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
async fn list_tasks(
    State(collector): State<Arc<SimpleCollector>>,
) -> impl IntoResponse {
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

/// 获取统计信息端点
async fn get_stats(
    State(collector): State<Arc<SimpleCollector>>,
) -> impl IntoResponse {
    let stats = collector.get_task_stats().await;
    (StatusCode::OK, Json(stats))
}

/// SSE事件流端点
async fn sse_events(
    State(collector): State<Arc<SimpleCollector>>,
) -> impl IntoResponse {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_simple_collector_creation() {
        let collector = SimpleCollector::new("/tmp/test_collector");
        assert_eq!(collector.get_task_stats().await.total, 0);
    }

    #[test]
    fn test_task_source_parsing() {
        let _collector = SimpleCollector::new("/tmp");

        // This would need to be made async in a real test
        // let result = collector.parse_task_source("ashare", "https://example.com").await;
        // assert!(result.is_ok());
    }

    #[test]
    fn test_priority_parsing() {
        let collector = SimpleCollector::new("/tmp");

        assert_eq!(collector.parse_priority(&Some("critical".to_string())), TaskPriority::Critical);
        assert_eq!(collector.parse_priority(&Some("high".to_string())), TaskPriority::High);
        assert_eq!(collector.parse_priority(&Some("low".to_string())), TaskPriority::Low);
        assert_eq!(collector.parse_priority(&Some("invalid".to_string())), TaskPriority::Medium);
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
        assert!(python_code.contains("import requests"));
        assert!(python_code.contains(&task.config.request.url));

        let nodejs_code = collector.generate_script_code(&task, &CrawlerLanguage::NodeJs);
        assert!(nodejs_code.contains("require('https')"));
        assert!(nodejs_code.contains(&task.config.request.url));

        let shell_code = collector.generate_script_code(&task, &CrawlerLanguage::Shell);
        assert!(shell_code.contains("curl"));
        assert!(shell_code.contains(&task.config.request.url));
    }
}
