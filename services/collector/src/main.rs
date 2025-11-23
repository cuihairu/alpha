//! Alpha Collector Service
//!
//! 异步爬虫与数据采集引擎，提供任务调度、限流和状态查询功能

use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use async_ftp::FtpStream;
use axum::{
    extract::{Path, State},
    http::{header::CONTENT_TYPE, HeaderValue, StatusCode},
    response::{
        sse::{Event as SseEvent, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use config::{Config, Environment, File};
use metrics::{counter, gauge, histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use once_cell::sync::OnceCell;
use rand::{seq::SliceRandom, thread_rng, Rng};
use reqwest::{Client, Method, Proxy};
use serde::{Deserialize, Serialize};
use tokio::{
    fs,
    net::{TcpListener, TcpStream},
    sync::{broadcast, mpsc, RwLock, Semaphore},
};
use tokio_stream::{
    wrappers::{errors::BroadcastStreamRecvError, BroadcastStream},
    StreamExt,
};
use std::convert::Infallible;
use tower::ServiceBuilder;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::Instrument;
use url::Url;
use uuid::Uuid;

static METRICS_HANDLE: OnceCell<PrometheusHandle> = OnceCell::new();

fn init_metrics_recorder() -> Result<PrometheusHandle, anyhow::Error> {
    METRICS_HANDLE
        .get_or_try_init(|| {
            PrometheusBuilder::new()
                .install_recorder()
                .map_err(|err| anyhow::anyhow!("failed to install metrics recorder: {}", err))
        })
        .cloned()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = CollectorConfig::load()?;
    let metrics_handle = init_metrics_recorder()?;

    tracing_subscriber::fmt()
        .with_max_level(config.telemetry.level_filter())
        .init();

    let (task_tx, task_rx) = mpsc::channel(config.scheduler.queue_capacity);
    let event_bus = Arc::new(EventBus::new(config.events.capacity));
    let state = Arc::new(CollectorState::new(
        config.clone(),
        task_tx.clone(),
        metrics_handle.clone(),
        event_bus.clone(),
    ));

    let manager = TaskManager::new(config.clone(), task_rx, state.clone(), task_tx.clone());
    manager.spawn_workers();

    Monitor::new(state.clone(), config.monitor.clone()).spawn();
    DependencyMonitor::spawn(state.clone(), config.dependencies.clone());

    let router = build_router(state.clone());
    let addr: SocketAddr = config.server.addr.parse()?;
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("Collector service listening on {}", addr);

    axum::serve(listener, router).await?;

    Ok(())
}

fn build_router(state: Arc<CollectorState>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/tasks", post(create_task).get(list_tasks))
        .route("/tasks/:id", get(get_task))
        .route("/metrics", get(get_metrics))
        .route("/metrics/prometheus", get(get_prometheus_metrics))
        .route("/events", get(stream_events))
        .with_state(state)
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(
                    CorsLayer::new()
                        .allow_origin(Any)
                        .allow_methods(Any)
                        .allow_headers(Any),
                ),
        )
}

async fn health_check(State(state): State<Arc<CollectorState>>) -> Json<serde_json::Value> {
    let dependencies = state.dependency_statuses().await;
    let dependencies_ready = state.dependencies_ready().await;
    let status = if dependencies_ready {
        "healthy"
    } else {
        "degraded"
    };
    Json(serde_json::json!({
        "status": status,
        "version": env!("CARGO_PKG_VERSION"),
        "pending_tasks": state.pending_task_count().await,
        "dependencies_enabled": state.dependency_checks_enabled(),
        "dependencies_ready": dependencies_ready,
        "dependencies": dependencies,
        "timestamp": Utc::now(),
    }))
}

async fn create_task(
    State(state): State<Arc<CollectorState>>,
    Json(payload): Json<NewTaskRequest>,
) -> Result<Json<TaskResponse>, ApiError> {
    let id = state.enqueue_task(payload).await?;

    Ok(Json(TaskResponse {
        id,
        status: TaskStatus::Pending,
    }))
}

async fn list_tasks(State(state): State<Arc<CollectorState>>) -> Json<Vec<TaskDetail>> {
    let tasks = state.list_tasks().await;
    Json(tasks)
}

async fn get_task(
    Path(id): Path<Uuid>,
    State(state): State<Arc<CollectorState>>,
) -> Result<Json<TaskDetail>, ApiError> {
    state
        .get_task(id)
        .await
        .map(Json)
        .ok_or_else(|| ApiError::not_found("task not found"))
}

async fn get_metrics(State(state): State<Arc<CollectorState>>) -> Json<serde_json::Value> {
    let metrics = state.metrics().await;
    Json(metrics)
}

async fn get_prometheus_metrics(
    State(state): State<Arc<CollectorState>>,
) -> impl IntoResponse {
    let body = state.render_prometheus_metrics();
    (
        StatusCode::OK,
        [(CONTENT_TYPE, HeaderValue::from_static("text/plain; version=0.0.4"))],
        body,
    )
}

async fn stream_events(State(state): State<Arc<CollectorState>>) -> impl IntoResponse {
    let rx = state.subscribe_events();
    let keep_alive = KeepAlive::new()
        .interval(state.event_keep_alive_interval())
        .text("keep-alive");
    let stream = BroadcastStream::new(rx).filter_map(|result| match result {
        Ok(event) => match SseEvent::default().json_data(&event) {
            Ok(evt) => Some(Ok::<SseEvent, Infallible>(evt)),
            Err(err) => {
                tracing::error!("failed to serialize event: {}", err);
                None
            }
        },
        Err(BroadcastStreamRecvError::Lagged(skipped)) => {
            tracing::warn!("events subscriber lagged by {}", skipped);
            None
        },
    });
    Sse::new(stream).keep_alive(keep_alive)
}

#[derive(Debug, Clone, Deserialize)]
struct NewTaskRequest {
    url: String,
    method: Option<String>,
    headers: Option<HashMap<String, String>>,
    body: Option<String>,
    priority: Option<u8>,
    max_retries: Option<u8>,
    tags: Option<Vec<String>>,
    source: Option<TaskSource>,
}

#[derive(Debug, Clone, Serialize)]
struct TaskResponse {
    id: Uuid,
    status: TaskStatus,
}

#[derive(Debug, Clone, Serialize)]
struct TaskDetail {
    id: Uuid,
    url: String,
    priority: u8,
    status: TaskStatus,
    attempts: u8,
    max_retries: u8,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    last_error: Option<String>,
    response: Option<TaskResult>,
    tags: Vec<String>,
    source: TaskSource,
    quality_flags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct TaskResult {
    status: u16,
    latency_ms: u128,
    bytes: usize,
    sample: String,
    content_type: Option<String>,
    source: TaskSource,
    quality_flags: Vec<String>,
}

#[derive(Debug, Clone)]
struct FetchOutcome {
    status: u16,
    body: String,
    content_type: Option<String>,
}

#[derive(Clone)]
struct DataQualityPipeline {
    config: QualityConfig,
}

#[derive(Debug, Clone)]
struct QualityReport {
    rejected: bool,
    issues: Vec<String>,
}

impl QualityReport {
    fn passed(&self) -> bool {
        !self.rejected
    }
}

impl DataQualityPipeline {
    fn new(config: QualityConfig) -> Self {
        Self { config }
    }

    fn clean(&self, body: &str) -> String {
        let mut cleaned: String = body
            .chars()
            .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
            .collect();
        if self.config.normalize_whitespace {
            cleaned = cleaned
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
        }
        cleaned.trim().to_string()
    }

    fn analyze(&self, _url: &str, body: &str) -> QualityReport {
        if !self.config.enabled {
            return QualityReport {
                rejected: false,
                issues: Vec::new(),
            };
        }
        let mut issues = Vec::new();
        let mut rejected = false;
        let bytes = body.as_bytes().len();
        if bytes < self.config.min_bytes {
            issues.push(format!(
                "payload too small: {}B < {}B",
                bytes, self.config.min_bytes
            ));
            rejected = true;
        }
        if self.config.max_bytes > 0 && bytes > self.config.max_bytes {
            issues.push(format!(
                "payload too large: {}B > {}B",
                bytes, self.config.max_bytes
            ));
            rejected = true;
        }
        if !self.config.blocked_keywords.is_empty() {
            for keyword in &self.config.blocked_keywords {
                if body.contains(keyword) {
                    issues.push(format!("blocked keyword detected: {}", keyword));
                    rejected = true;
                }
            }
        }
        if !self.config.required_keywords.is_empty() {
            let missing: Vec<_> = self
                .config
                .required_keywords
                .iter()
                .filter(|kw| !body.contains(*kw))
                .cloned()
                .collect();
            if !missing.is_empty() {
                issues.push(format!("missing required markers: {}", missing.join(", ")));
                if self.config.reject_on_missing_required {
                    rejected = true;
                }
            }
        }
        QualityReport { rejected, issues }
    }
}

struct ProxyPool {
    proxies: Arc<tokio::sync::Mutex<Vec<ProxyState>>>,
    config: ProxyConfig,
}

struct ProxyState {
    _url: String,
    client: Client,
    fail_count: u8,
    banned_until: Option<DateTime<Utc>>,
}

struct ProxyLease {
    pool: Arc<ProxyPool>,
    index: usize,
    client: Client,
}

struct RateLimiter {
    per_host_min_interval: Duration,
    global_min_interval: Duration,
    jitter_ms: u64,
    last_seen: tokio::sync::Mutex<HashMap<String, Instant>>,
    last_global: tokio::sync::Mutex<Option<Instant>>,
}

struct Monitor {
    state: Arc<CollectorState>,
    config: MonitorConfig,
}

struct DependencyMonitor {
    state: Arc<CollectorState>,
    config: DependencyConfig,
    http_client: Client,
}

impl ProxyPool {
    fn new(config: ProxyConfig, timeout_secs: u64) -> Arc<Self> {
        let proxies = config
            .endpoints
            .iter()
            .filter_map(|endpoint| {
                Proxy::all(endpoint)
                    .ok()
                    .and_then(|proxy| {
                        Client::builder()
                            .proxy(proxy)
                            .timeout(Duration::from_secs(timeout_secs))
                            .build()
                            .ok()
                    })
                    .map(|client| ProxyState {
                        _url: endpoint.clone(),
                        client,
                        fail_count: 0,
                        banned_until: None,
                    })
            })
            .collect();

        Arc::new(Self {
            proxies: Arc::new(tokio::sync::Mutex::new(proxies)),
            config,
        })
    }

    async fn checkout(self: &Arc<Self>) -> Option<ProxyLease> {
        let mut guard = self.proxies.lock().await;
        let now = Utc::now();
        let mut indices: Vec<usize> = (0..guard.len()).collect();
        indices.shuffle(&mut thread_rng());
        for idx in indices {
            let state = &mut guard[idx];
            if state.banned_until.map(|until| until > now).unwrap_or(false) {
                continue;
            }
            return Some(ProxyLease {
                pool: self.clone(),
                index: idx,
                client: state.client.clone(),
            });
        }
        None
    }

    async fn record_success(&self, index: usize) {
        let mut guard = self.proxies.lock().await;
        if let Some(state) = guard.get_mut(index) {
            state.fail_count = 0;
            state.banned_until = None;
        }
    }

    async fn record_failure(&self, index: usize) {
        let mut guard = self.proxies.lock().await;
        if let Some(state) = guard.get_mut(index) {
            state.fail_count += 1;
            if state.fail_count >= self.config.failure_threshold {
                state.banned_until = Some(
                    Utc::now() + chrono::Duration::seconds(self.config.ban_duration_secs as i64),
                );
                state.fail_count = 0;
            }
        }
    }
}

impl ProxyLease {
    async fn report_success(self) {
        self.pool.record_success(self.index).await;
    }

    async fn report_failure(self) {
        self.pool.record_failure(self.index).await;
    }
}

impl RateLimiter {
    fn new(config: RateLimitConfig) -> Self {
        Self {
            per_host_min_interval: Duration::from_millis(config.per_host_min_interval_ms.max(1)),
            global_min_interval: Duration::from_millis(config.global_min_interval_ms),
            jitter_ms: config.jitter_ms,
            last_seen: tokio::sync::Mutex::new(HashMap::new()),
            last_global: tokio::sync::Mutex::new(None),
        }
    }

    async fn acquire(&self, host: Option<&str>) {
        if let Some(host) = host {
            self.wait_host(host).await;
        }
        self.wait_global().await;
        if self.jitter_ms > 0 {
            let jitter = thread_rng().gen_range(0..=self.jitter_ms);
            if jitter > 0 {
                tokio::time::sleep(Duration::from_millis(jitter)).await;
            }
        }
    }

    async fn wait_host(&self, host: &str) {
        loop {
            let mut guard = self.last_seen.lock().await;
            if let Some(last) = guard.get(host) {
                let elapsed = last.elapsed();
                if elapsed < self.per_host_min_interval {
                    let wait = self.per_host_min_interval - elapsed;
                    drop(guard);
                    tokio::time::sleep(wait).await;
                    continue;
                }
            }
            guard.insert(host.to_string(), Instant::now());
            break;
        }
    }

    async fn wait_global(&self) {
        if self.global_min_interval.is_zero() {
            return;
        }
        loop {
            let mut guard = self.last_global.lock().await;
            if let Some(last) = *guard {
                let elapsed = last.elapsed();
                if elapsed < self.global_min_interval {
                    let wait = self.global_min_interval - elapsed;
                    drop(guard);
                    tokio::time::sleep(wait).await;
                    continue;
                }
            }
            *guard = Some(Instant::now());
            break;
        }
    }
}

impl Monitor {
    fn new(state: Arc<CollectorState>, config: MonitorConfig) -> Self {
        Self { state, config }
    }

    fn spawn(self) {
        let state = self.state.clone();
        let interval = Duration::from_secs(self.config.interval_secs.max(1));
        let timeout = chrono::Duration::seconds(self.config.task_timeout_secs as i64);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                let requeued = state.requeue_timed_out_tasks(timeout).await;
                if requeued > 0 {
                    tracing::warn!(
                        "Monitor requeued {} timed out task(s)",
                        requeued
                    );
                }
            }
        });
    }
}

impl DependencyMonitor {
    fn spawn(state: Arc<CollectorState>, config: DependencyConfig) {
        if !config.enabled || config.checks.is_empty() {
            return;
        }
        let timeout = Duration::from_secs(config.default_timeout_secs.max(1));
        let http_client = match Client::builder().timeout(timeout).build() {
            Ok(client) => client,
            Err(err) => {
                tracing::error!("Failed to build dependency monitor client: {}", err);
                return;
            }
        };
        let monitor = Self {
            state,
            config,
            http_client,
        };
        tokio::spawn(async move {
            monitor.run().await;
        });
    }

    async fn run(self) {
        let interval = Duration::from_secs(self.config.interval_secs.max(5));
        loop {
            for check in self.config.checks.iter().cloned() {
                self.execute_check(check).await;
            }
            tokio::time::sleep(interval).await;
        }
    }

    async fn execute_check(&self, check: DependencyCheckConfig) {
        let timeout = Duration::from_secs(
            check
                .timeout_secs
                .unwrap_or(self.config.default_timeout_secs)
                .max(1),
        );
        let start = Instant::now();
        let result = match tokio::time::timeout(timeout, self.perform_check(&check)).await {
            Ok(Ok(())) => DependencyResult {
                status: CheckStatus::Healthy,
                latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0),
                message: Some("ok".to_string()),
                checked_at: Utc::now(),
            },
            Ok(Err(err)) => DependencyResult {
                status: CheckStatus::Unhealthy,
                latency_ms: Some(start.elapsed().as_secs_f64() * 1000.0),
                message: Some(err.to_string()),
                checked_at: Utc::now(),
            },
            Err(_) => DependencyResult {
                status: CheckStatus::Unhealthy,
                latency_ms: None,
                message: Some("check timed out".to_string()),
                checked_at: Utc::now(),
            },
        };
        self.state
            .update_dependency_status(&check.name, result)
            .await;
    }

    async fn perform_check(&self, check: &DependencyCheckConfig) -> Result<(), anyhow::Error> {
        match check.kind {
            DependencyKind::Http => {
                let resp = self.http_client.get(&check.target).send().await?;
                if resp.status().is_success() {
                    Ok(())
                } else {
                    anyhow::bail!("status {}", resp.status());
                }
            }
            DependencyKind::Tcp => {
                TcpStream::connect(&check.target).await?;
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Retrying,
    QualityRejected,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TaskSource {
    WebPage,
    Api,
    Ftp,
    FilePush,
}

impl Default for TaskSource {
    fn default() -> Self {
        TaskSource::WebPage
    }
}

impl TaskSource {
    fn from_url(url: &str) -> Self {
        if let Ok(parsed) = Url::parse(url) {
            match parsed.scheme() {
                "ftp" => TaskSource::Ftp,
                "file" => TaskSource::FilePush,
                _ => TaskSource::WebPage,
            }
        } else {
            TaskSource::WebPage
        }
    }

    fn is_http(&self) -> bool {
        matches!(self, TaskSource::WebPage | TaskSource::Api)
    }

    fn as_label(&self) -> &'static str {
        match self {
            TaskSource::WebPage => "web_page",
            TaskSource::Api => "api",
            TaskSource::Ftp => "ftp",
            TaskSource::FilePush => "file_push",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DependencyKind {
    Http,
    Tcp,
}

impl DependencyKind {
    fn as_label(&self) -> &'static str {
        match self {
            DependencyKind::Http => "http",
            DependencyKind::Tcp => "tcp",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CheckStatus {
    Unknown,
    Healthy,
    Unhealthy,
}

impl CheckStatus {
    fn is_healthy(&self) -> bool {
        matches!(self, CheckStatus::Healthy)
    }
}

#[derive(Debug, Clone, Serialize)]
struct DependencySnapshot {
    name: String,
    kind: DependencyKind,
    target: String,
    status: CheckStatus,
    last_checked: Option<DateTime<Utc>>,
    latency_ms: Option<f64>,
    message: Option<String>,
}

struct DependencyState {
    config: DependencyCheckConfig,
    status: CheckStatus,
    last_checked: Option<DateTime<Utc>>,
    latency_ms: Option<f64>,
    message: Option<String>,
}

impl DependencyState {
    fn new(config: DependencyCheckConfig) -> Self {
        Self {
            config,
            status: CheckStatus::Unknown,
            last_checked: None,
            latency_ms: None,
            message: None,
        }
    }

    fn snapshot(&self) -> DependencySnapshot {
        DependencySnapshot {
            name: self.config.name.clone(),
            kind: self.config.kind.clone(),
            target: self.config.target.clone(),
            status: self.status.clone(),
            last_checked: self.last_checked,
            latency_ms: self.latency_ms,
            message: self.message.clone(),
        }
    }

    fn apply(&mut self, result: DependencyResult) {
        self.status = result.status;
        self.last_checked = Some(result.checked_at);
        self.latency_ms = result.latency_ms;
        self.message = result.message;
    }
}

struct DependencyResult {
    status: CheckStatus,
    latency_ms: Option<f64>,
    message: Option<String>,
    checked_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum TaskEvent {
    TaskQueued {
        id: Uuid,
        url: String,
        priority: u8,
        source: TaskSource,
        tags: Vec<String>,
    },
    TaskStarted {
        id: Uuid,
        attempt: u8,
        source: TaskSource,
    },
    TaskCompleted {
        id: Uuid,
        status_code: u16,
        latency_ms: u128,
        bytes: usize,
        source: TaskSource,
    },
    TaskFailed {
        id: Uuid,
        attempt: u8,
        source: TaskSource,
        error: String,
    },
    TaskQualityRejected {
        id: Uuid,
        source: TaskSource,
        reason: String,
    },
    DependencyStatus {
        name: String,
        status: CheckStatus,
        latency_ms: Option<f64>,
        message: Option<String>,
    },
}

#[derive(Clone)]
struct EventBus {
    tx: broadcast::Sender<TaskEvent>,
}

impl EventBus {
    fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity.max(32));
        Self { tx }
    }

    fn publish(&self, event: TaskEvent) {
        if let Err(err) = self.tx.send(event) {
            tracing::debug!("dropping event subscriber error: {}", err);
        }
    }

    fn subscribe(&self) -> broadcast::Receiver<TaskEvent> {
        self.tx.subscribe()
    }
}

#[derive(Clone)]
struct CollectorState {
    config: CollectorConfig,
    tasks: Arc<RwLock<HashMap<Uuid, TaskRecord>>>,
    task_tx: mpsc::Sender<CrawlTask>,
    metrics_handle: PrometheusHandle,
    dependencies: Arc<RwLock<HashMap<String, DependencyState>>>,
    event_bus: Arc<EventBus>,
}

impl CollectorState {
    fn new(
        config: CollectorConfig,
        task_tx: mpsc::Sender<CrawlTask>,
        metrics_handle: PrometheusHandle,
        event_bus: Arc<EventBus>,
    ) -> Self {
        let dependencies = config
            .dependencies
            .checks
            .iter()
            .cloned()
            .map(|check| (check.name.clone(), DependencyState::new(check)))
            .collect();
        Self {
            config,
            tasks: Arc::new(RwLock::new(HashMap::new())),
            task_tx,
            metrics_handle,
            dependencies: Arc::new(RwLock::new(dependencies)),
            event_bus,
        }
    }

    async fn enqueue_task(&self, req: NewTaskRequest) -> Result<Uuid, ApiError> {
        let id = Uuid::new_v4();
        let method = req
            .method
            .as_deref()
            .unwrap_or("GET")
            .parse::<Method>()
            .map_err(|e| ApiError::bad_request(format!("invalid method: {}", e)))?;

        let task = CrawlTask {
            id,
            url: req.url.clone(),
            method,
            headers: req.headers.unwrap_or_default(),
            body: req.body,
            priority: req.priority.unwrap_or(5),
            attempts: 0,
            max_retries: req.max_retries.unwrap_or(self.config.scheduler.max_retries),
            created_at: Utc::now(),
            tags: req.tags.unwrap_or_default(),
            source: req
                .source
                .unwrap_or_else(|| TaskSource::from_url(&req.url)),
        };

        {
            let mut guard = self.tasks.write().await;
            guard.insert(
                id,
                TaskRecord {
                    task: task.clone(),
                    status: TaskStatus::Pending,
                    last_error: None,
                    started_at: None,
                    finished_at: None,
                    last_update: Utc::now(),
                    response: None,
                    quality_flags: Vec::new(),
                },
            );
            counter!(
                "collector_tasks_enqueued_total",
                1,
                "source" => task.source.as_label()
            );
            gauge!("collector_tasks_tracked", guard.len() as f64);
        }
        self.publish_event(TaskEvent::TaskQueued {
            id,
            url: task.url.clone(),
            priority: task.priority,
            source: task.source,
            tags: task.tags.clone(),
        });

        self.task_tx
            .send(task)
            .await
            .map_err(|_| ApiError::internal("failed to queue task"))?;

        Ok(id)
    }

    async fn update_status<F>(&self, id: &Uuid, update_fn: F)
    where
        F: FnOnce(&mut TaskRecord),
    {
        if let Some(record) = self.tasks.write().await.get_mut(id) {
            update_fn(record);
            record.last_update = Utc::now();
        }
    }

    async fn get_task(&self, id: Uuid) -> Option<TaskDetail> {
        self.tasks.read().await.get(&id).map(|record| TaskDetail {
            id: record.task.id,
            url: record.task.url.clone(),
            priority: record.task.priority,
            status: record.status.clone(),
            attempts: record.task.attempts,
            max_retries: record.task.max_retries,
            created_at: record.task.created_at,
            started_at: record.started_at,
            finished_at: record.finished_at,
            last_error: record.last_error.clone(),
            response: record.response.clone(),
            tags: record.task.tags.clone(),
             source: record.task.source,
             quality_flags: record.quality_flags.clone(),
        })
    }

    async fn list_tasks(&self) -> Vec<TaskDetail> {
        self.tasks
            .read()
            .await
            .values()
            .map(|record| TaskDetail {
                id: record.task.id,
                url: record.task.url.clone(),
                priority: record.task.priority,
                status: record.status.clone(),
                attempts: record.task.attempts,
                max_retries: record.task.max_retries,
                created_at: record.task.created_at,
                started_at: record.started_at,
                finished_at: record.finished_at,
                last_error: record.last_error.clone(),
                response: record.response.clone(),
                tags: record.task.tags.clone(),
                source: record.task.source,
                quality_flags: record.quality_flags.clone(),
            })
            .collect()
    }

    async fn pending_task_count(&self) -> usize {
        self.tasks
            .read()
            .await
            .values()
            .filter(|r| matches!(r.status, TaskStatus::Pending | TaskStatus::Retrying))
            .count()
    }

    async fn dependency_statuses(&self) -> Vec<DependencySnapshot> {
        self.dependencies
            .read()
            .await
            .values()
            .map(|state| state.snapshot())
            .collect()
    }

    fn subscribe_events(&self) -> broadcast::Receiver<TaskEvent> {
        self.event_bus.subscribe()
    }

    fn publish_event(&self, event: TaskEvent) {
        self.event_bus.publish(event);
    }

    fn event_keep_alive_interval(&self) -> Duration {
        Duration::from_secs(self.config.events.sse_keep_alive_secs.max(5))
    }

    async fn update_dependency_status(&self, name: &str, result: DependencyResult) {
        let mut guard = self.dependencies.write().await;
        if let Some(state) = guard.get_mut(name) {
            state.apply(result);
            let status_value = if state.status.is_healthy() { 1.0 } else { 0.0 };
            gauge!(
                "collector_dependency_status",
                status_value,
                "dependency" => state.config.name.clone(),
                "kind" => state.config.kind.as_label()
            );
            self.publish_event(TaskEvent::DependencyStatus {
                name: state.config.name.clone(),
                status: state.status.clone(),
                latency_ms: state.latency_ms,
                message: state.message.clone(),
            });
        }
    }

    async fn dependencies_ready(&self) -> bool {
        if !self.config.dependencies.enabled || self.config.dependencies.checks.is_empty() {
            return true;
        }
        self.dependencies
            .read()
            .await
            .values()
            .all(|state| state.status.is_healthy())
    }

    fn dependency_checks_enabled(&self) -> bool {
        self.config.dependencies.enabled && !self.config.dependencies.checks.is_empty()
    }

    async fn metrics(&self) -> serde_json::Value {
        let guard = self.tasks.read().await;
        let total = guard.len();
        let pending = guard
            .values()
            .filter(|r| matches!(r.status, TaskStatus::Pending | TaskStatus::Retrying))
            .count();
        let running = guard
            .values()
            .filter(|r| matches!(r.status, TaskStatus::Running))
            .count();
        let completed = guard
            .values()
            .filter(|r| matches!(r.status, TaskStatus::Completed))
            .count();
        let failed = guard
            .values()
            .filter(|r| matches!(r.status, TaskStatus::Failed))
            .count();
        let quality_rejected = guard
            .values()
            .filter(|r| matches!(r.status, TaskStatus::QualityRejected))
            .count();

        gauge!("collector_tasks_total", total as f64);
        gauge!("collector_tasks_pending", pending as f64);
        gauge!("collector_tasks_running", running as f64);
        gauge!("collector_tasks_completed", completed as f64);
        gauge!("collector_tasks_failed", failed as f64);
        gauge!("collector_tasks_quality_rejected", quality_rejected as f64);

        serde_json::json!({
            "total": total,
            "pending": pending,
            "running": running,
            "completed": completed,
            "failed": failed,
            "quality_rejected": quality_rejected,
            "timeout_cutoff_secs": self.config.monitor.task_timeout_secs,
        })
    }

    fn render_prometheus_metrics(&self) -> String {
        self.metrics_handle.render()
    }

    async fn requeue_timed_out_tasks(&self, timeout: chrono::Duration) -> usize {
        let now = Utc::now();
        let mut to_requeue = Vec::new();

        {
            let mut guard = self.tasks.write().await;
            for record in guard.values_mut() {
                if !matches!(record.status, TaskStatus::Running) {
                    continue;
                }
                let Some(started_at) = record.started_at else {
                    continue;
                };
                if now - started_at < timeout {
                    continue;
                }
                counter!(
                    "collector_tasks_timeouts_total",
                    1,
                    "source" => record.task.source.as_label()
                );
                if record.task.attempts >= record.task.max_retries {
                    record.status = TaskStatus::Failed;
                    record.finished_at.get_or_insert(now);
                    record.last_error
                        .get_or_insert_with(|| "Task timed out".to_string());
                    counter!(
                        "collector_tasks_failed_total",
                        1,
                        "source" => record.task.source.as_label(),
                        "reason" => "timeout"
                    );
                    continue;
                }

                record.task.attempts += 1;
                record.status = TaskStatus::Retrying;
                record.last_error = Some("Task timed out".to_string());
                record.started_at = None;
                record.finished_at = None;
                record.response = None;
                record.quality_flags.clear();
                counter!(
                    "collector_tasks_retried_total",
                    1,
                    "source" => record.task.source.as_label()
                );
                to_requeue.push(record.task.clone());
            }
        }

        for task in to_requeue.iter().cloned() {
            if let Err(err) = self.task_tx.send(task).await {
                tracing::error!("Failed to requeue timed-out task: {}", err);
            }
        }

        to_requeue.len()
    }
}

#[derive(Clone)]
struct TaskRecord {
    task: CrawlTask,
    status: TaskStatus,
    last_error: Option<String>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    last_update: DateTime<Utc>,
    response: Option<TaskResult>,
    quality_flags: Vec<String>,
}

#[derive(Clone)]
struct CrawlTask {
    id: Uuid,
    url: String,
    method: Method,
    headers: HashMap<String, String>,
    body: Option<String>,
    priority: u8,
    attempts: u8,
    max_retries: u8,
    created_at: DateTime<Utc>,
    tags: Vec<String>,
    source: TaskSource,
}

struct TaskManager {
    config: CollectorConfig,
    default_client: Client,
    task_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<CrawlTask>>>,
    state: Arc<CollectorState>,
    limiter: Arc<Semaphore>,
    task_tx: mpsc::Sender<CrawlTask>,
    proxy_pool: Option<Arc<ProxyPool>>,
    rate_limiter: Arc<RateLimiter>,
    quality: Arc<DataQualityPipeline>,
}

impl TaskManager {
    fn new(
        config: CollectorConfig,
        task_rx: mpsc::Receiver<CrawlTask>,
        state: Arc<CollectorState>,
        task_tx: mpsc::Sender<CrawlTask>,
    ) -> Arc<Self> {
        let default_client = Client::builder()
            .timeout(Duration::from_secs(config.network.timeout_secs))
            .user_agent(config.network.user_agent())
            .build()
            .expect("failed to build reqwest client");

        let proxy_pool = config.network.proxy.as_ref().and_then(|cfg| {
            if cfg.enabled && !cfg.endpoints.is_empty() {
                Some(ProxyPool::new(cfg.clone(), config.network.timeout_secs))
            } else {
                None
            }
        });
        let quality = Arc::new(DataQualityPipeline::new(config.quality.clone()));

        Arc::new(Self {
            limiter: Arc::new(Semaphore::new(config.scheduler.max_concurrency)),
            rate_limiter: Arc::new(RateLimiter::new(config.rate_limit.clone())),
            config,
            default_client,
            task_rx: Arc::new(tokio::sync::Mutex::new(task_rx)),
            state,
            task_tx,
            proxy_pool,
            quality,
        })
    }

    fn spawn_workers(self: &Arc<Self>) {
        for worker_id in 0..self.config.scheduler.worker_count {
            let manager = Arc::clone(self);
            tokio::spawn(async move {
                manager.worker_loop(worker_id).await;
            });
        }
    }

    async fn worker_loop(self: Arc<Self>, worker_id: usize) {
        let span = tracing::info_span!("collector_worker", worker_id);
        let _enter = span.enter();
        tracing::info!("worker {} started", worker_id);

        loop {
            let task = {
                let mut guard = self.task_rx.lock().await;
                guard.recv().await
            };

            let Some(task) = task else {
                tracing::warn!("task channel closed for worker {}", worker_id);
                break;
            };

            let permit = match self.limiter.clone().acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => break,
            };

            let state = self.state.clone();
            let client = self.default_client.clone();
            let config = self.config.clone();
            let tx = self.task_tx.clone();
            let proxy_pool = self.proxy_pool.clone();
            let rate_limiter = self.rate_limiter.clone();
            let quality = self.quality.clone();

            let span = tracing::info_span!("task_worker", task_id = %task.id);
            async move {
                let _permit = permit;
                process_task(
                    state,
                    client,
                    config,
                    tx,
                    task,
                    proxy_pool,
                    rate_limiter,
                    quality,
                )
                .await;
            }
            .instrument(span)
            .await;
        }
    }
}

async fn process_task(
    state: Arc<CollectorState>,
    default_client: Client,
    config: CollectorConfig,
    task_tx: mpsc::Sender<CrawlTask>,
    mut task: CrawlTask,
    proxy_pool: Option<Arc<ProxyPool>>,
    rate_limiter: Arc<RateLimiter>,
    quality: Arc<DataQualityPipeline>,
) {
    let source_label = task.source.as_label();
    state
        .update_status(&task.id, |record| {
            record.status = TaskStatus::Running;
        })
        .await;
    state
        .update_status(&task.id, |record| {
            record.started_at.get_or_insert(Utc::now());
        })
        .await;
    counter!("collector_tasks_started_total", 1, "source" => source_label);
    state.publish_event(TaskEvent::TaskStarted {
        id: task.id,
        attempt: task.attempts + 1,
        source: task.source,
    });

    let mut proxy_handle = if task.source.is_http() {
        if let Some(pool) = proxy_pool {
            pool.checkout().await
        } else {
            None
        }
    } else {
        None
    };
    let client = proxy_handle
        .as_ref()
        .map(|lease| lease.client.clone())
        .unwrap_or_else(|| default_client.clone());

    let host = Url::parse(&task.url)
        .ok()
        .and_then(|url| url.host_str().map(|h| h.to_string()));
    if task.source.is_http() {
        rate_limiter.acquire(host.as_deref()).await;
    }

    let user_agent = config.network.random_user_agent();
    let header_profile = config.network.random_header_profile(task.source);

    let start = Instant::now();
    let result = match task.source {
        TaskSource::WebPage | TaskSource::Api => {
            execute_http_request(&client, &task, &user_agent, header_profile.as_ref()).await
        }
        TaskSource::Ftp => execute_ftp_request(&task).await,
        TaskSource::FilePush => execute_file_request(&task).await,
    };
    let elapsed = start.elapsed();

    match result {
        Ok(outcome) => {
            let cleaned_body = quality.clean(&outcome.body);
            let cleaned_bytes = cleaned_body.as_bytes().len();
            let quality_report = quality.analyze(&task.url, &cleaned_body);
            if !quality_report.passed() {
                let reason = if quality_report.issues.is_empty() {
                    "quality violation detected".to_string()
                } else {
                    format!("quality violation: {}", quality_report.issues.join("; "))
                };
                state
                    .update_status(&task.id, |record| {
                        record.status = TaskStatus::QualityRejected;
                        record.finished_at = Some(Utc::now());
                        record.last_error = Some(reason.clone());
                        record.response = None;
                        record.quality_flags = quality_report.issues.clone();
                    })
                    .await;
                if let Some(handle) = proxy_handle.take() {
                    handle.report_failure().await;
                }
                counter!(
                    "collector_tasks_quality_rejected_total",
                    1,
                    "source" => source_label
                );
                state.publish_event(TaskEvent::TaskQualityRejected {
                    id: task.id,
                    source: task.source,
                    reason,
                });
                return;
            }

            let sample = cleaned_body
                .chars()
                .take(config.network.body_preview)
                .collect::<String>();
            let latency_ms = elapsed.as_secs_f64() * 1000.0;
            state
                .update_status(&task.id, |record| {
                    record.status = TaskStatus::Completed;
                    record.finished_at = Some(Utc::now());
                    record.response = Some(TaskResult {
                        status: outcome.status,
                        latency_ms: elapsed.as_millis(),
                        bytes: cleaned_bytes,
                        sample,
                        content_type: outcome.content_type.clone(),
                        source: task.source,
                        quality_flags: quality_report.issues.clone(),
                    });
                    record.quality_flags = quality_report.issues.clone();
                })
                .await;
            counter!(
                "collector_tasks_completed_total",
                1,
                "source" => source_label
            );
            histogram!(
                "collector_task_latency_ms",
                latency_ms,
                "source" => source_label
            );
            histogram!(
                "collector_task_payload_bytes",
                cleaned_bytes as f64,
                "source" => source_label
            );
            if let Some(handle) = proxy_handle.take() {
                handle.report_success().await;
            }
            state.publish_event(TaskEvent::TaskCompleted {
                id: task.id,
                status_code: outcome.status,
                latency_ms: elapsed.as_millis(),
                bytes: cleaned_bytes,
                source: task.source,
            });
        }
        Err(err) => {
            let error_message = err.to_string();
            task.attempts += 1;
            let should_retry = task.attempts <= task.max_retries;
            let failure_reason = if should_retry { "transient" } else { "exhausted" };

            state
                .update_status(&task.id, |record| {
                    record.last_error = Some(error_message.clone());
                    record.status = if should_retry {
                        TaskStatus::Retrying
                    } else {
                        TaskStatus::Failed
                    };
                    if !should_retry {
                        record.finished_at = Some(Utc::now());
                    }
                    record.quality_flags.clear();
                })
                .await;
            counter!(
                "collector_tasks_failed_total",
                1,
                "source" => source_label,
                "reason" => failure_reason
            );
            state.publish_event(TaskEvent::TaskFailed {
                id: task.id,
                attempt: task.attempts,
                source: task.source,
                error: error_message,
            });

            if should_retry {
                if let Some(handle) = proxy_handle.take() {
                    handle.report_failure().await;
                }
                let backoff =
                    Duration::from_millis(config.scheduler.retry_backoff_ms * task.attempts as u64);
                tokio::time::sleep(backoff).await;
                counter!(
                    "collector_tasks_retried_total",
                    1,
                    "source" => source_label
                );
                if let Err(e) = task_tx.send(task).await {
                    tracing::error!("Failed to requeue task: {}", e);
                }
            } else if let Some(handle) = proxy_handle.take() {
                handle.report_failure().await;
            }
        }
    }
}

async fn execute_http_request(
    client: &Client,
    task: &CrawlTask,
    user_agent: &str,
    profile: Option<&HeaderProfile>,
) -> Result<FetchOutcome, anyhow::Error> {
    let mut req = client.request(task.method.clone(), &task.url);

    for (key, value) in &task.headers {
        req = req.header(key, value);
    }

    req = req.header(reqwest::header::USER_AGENT, user_agent);

    if let Some(profile) = profile {
        for (key, value) in &profile.headers {
            if task.headers.contains_key(key) {
                continue;
            }
            req = req.header(key, value);
        }
    }

    if let Some(body) = &task.body {
        req = req.body(body.clone());
    }

    let resp = req.send().await?;
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());

    if !resp.status().is_success() {
        anyhow::bail!("status {}", resp.status());
    }

    let text = resp.text().await?;
    Ok(FetchOutcome {
        status,
        body: text,
        content_type,
    })
}

async fn execute_ftp_request(task: &CrawlTask) -> Result<FetchOutcome, anyhow::Error> {
    let url = Url::parse(&task.url)?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("missing ftp host"))?;
    let port = url.port().unwrap_or(21);
    let mut ftp_stream = FtpStream::connect((host, port)).await?;
    let username = if url.username().is_empty() {
        "anonymous"
    } else {
        url.username()
    };
    let password = url.password().unwrap_or("anonymous@");
    ftp_stream.login(username, password).await?;
    let path = if url.path().is_empty() { "/" } else { url.path() };
    let cursor = ftp_stream.simple_retr(path).await?;
    let data = cursor.into_inner();
    ftp_stream.quit().await.ok();
    let body = String::from_utf8_lossy(&data).to_string();
    Ok(FetchOutcome {
        status: 226,
        body,
        content_type: None,
    })
}

async fn execute_file_request(task: &CrawlTask) -> Result<FetchOutcome, anyhow::Error> {
    if let Some(body) = &task.body {
        return Ok(FetchOutcome {
            status: 200,
            body: body.clone(),
            content_type: None,
        });
    }

    let path = if task.url.starts_with("file://") {
        Url::parse(&task.url)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .unwrap_or_else(|| PathBuf::from(task.url.trim_start_matches("file://")))
    } else {
        PathBuf::from(&task.url)
    };
    let bytes = fs::read(path).await?;
    let body = String::from_utf8_lossy(&bytes).to_string();
    Ok(FetchOutcome {
        status: 200,
        body,
        content_type: None,
    })
}

#[derive(Debug, Clone, Deserialize)]
struct CollectorConfig {
    server: ServerConfig,
    scheduler: SchedulerConfig,
    network: NetworkConfig,
    monitor: MonitorConfig,
    rate_limit: RateLimitConfig,
    telemetry: TelemetryConfig,
    quality: QualityConfig,
    dependencies: DependencyConfig,
    events: EventsConfig,
}

#[derive(Debug, Clone, Deserialize)]
struct ServerConfig {
    addr: String,
}

#[derive(Debug, Clone, Deserialize)]
struct SchedulerConfig {
    worker_count: usize,
    max_concurrency: usize,
    queue_capacity: usize,
    max_retries: u8,
    retry_backoff_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct NetworkConfig {
    timeout_secs: u64,
    user_agents: Vec<String>,
    body_preview: usize,
    proxy: Option<ProxyConfig>,
    #[serde(default)]
    header_profiles: Vec<HeaderProfile>,
}

#[derive(Debug, Clone, Deserialize)]
struct HeaderProfile {
    name: String,
    headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProxyConfig {
    enabled: bool,
    endpoints: Vec<String>,
    failure_threshold: u8,
    ban_duration_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct RateLimitConfig {
    per_host_min_interval_ms: u64,
    global_min_interval_ms: u64,
    jitter_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct MonitorConfig {
    interval_secs: u64,
    task_timeout_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct TelemetryConfig {
    level: String,
}

#[derive(Debug, Clone, Deserialize)]
struct QualityConfig {
    enabled: bool,
    min_bytes: usize,
    max_bytes: usize,
    reject_on_missing_required: bool,
    blocked_keywords: Vec<String>,
    required_keywords: Vec<String>,
    normalize_whitespace: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct DependencyConfig {
    enabled: bool,
    interval_secs: u64,
    default_timeout_secs: u64,
    #[serde(default)]
    checks: Vec<DependencyCheckConfig>,
}

#[derive(Debug, Clone, Deserialize)]
struct DependencyCheckConfig {
    name: String,
    kind: DependencyKind,
    target: String,
    timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct EventsConfig {
    capacity: usize,
    sse_keep_alive_secs: u64,
}

impl TelemetryConfig {
    fn level_filter(&self) -> tracing::Level {
        match self.level.to_lowercase().as_str() {
            "debug" => tracing::Level::DEBUG,
            "warn" => tracing::Level::WARN,
            "error" => tracing::Level::ERROR,
            "trace" => tracing::Level::TRACE,
            _ => tracing::Level::INFO,
        }
    }
}

fn default_header_profiles() -> Vec<HeaderProfile> {
    let mut desktop_headers = HashMap::new();
    desktop_headers.insert(
        "accept-language".to_string(),
        "en-US,en;q=0.9".to_string(),
    );
    desktop_headers.insert(
        "accept".to_string(),
        "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8".to_string(),
    );
    desktop_headers.insert("cache-control".to_string(), "no-cache".to_string());

    let mut api_headers = HashMap::new();
    api_headers.insert("accept".to_string(), "application/json".to_string());
    api_headers.insert("accept-language".to_string(), "en-US;q=0.8".to_string());

    vec![
        HeaderProfile {
            name: "browser_desktop".to_string(),
            headers: desktop_headers,
        },
        HeaderProfile {
            name: "api_json".to_string(),
            headers: api_headers,
        },
    ]
}

impl NetworkConfig {
    fn user_agent(&self) -> String {
        self.user_agents
            .first()
            .cloned()
            .unwrap_or_else(|| "alpha-collector/0.1".to_string())
    }

    fn random_user_agent(&self) -> String {
        if let Some(choice) = self.user_agents.choose(&mut thread_rng()) {
            choice.clone()
        } else {
            self.user_agent()
        }
    }

    fn random_header_profile(&self, source: TaskSource) -> Option<HeaderProfile> {
        if self.header_profiles.is_empty() {
            return None;
        }
        if source == TaskSource::Api {
            if let Some(profile) = self
                .header_profiles
                .iter()
                .find(|profile| profile.name.to_lowercase().contains("api"))
            {
                return Some(profile.clone());
            }
        }
        self.header_profiles.choose(&mut thread_rng()).cloned()
    }
}

impl CollectorConfig {
    fn load() -> Result<Self, anyhow::Error> {
        let builder = Config::builder()
            .set_default("server.addr", "0.0.0.0:8090")?
            .set_default("scheduler.worker_count", 4)?
            .set_default("scheduler.max_concurrency", 8)?
            .set_default("scheduler.queue_capacity", 1024)?
            .set_default("scheduler.max_retries", 3)?
            .set_default("scheduler.retry_backoff_ms", 500)?
            .set_default("network.timeout_secs", 10)?
            .set_default(
                "network.user_agents",
                vec!["AlphaCollector/0.1 (https://alpha.finance)"],
            )?
            .set_default("network.body_preview", 512)?
            .set_default("network.proxy.enabled", false)?
            .set_default::<_, Vec<String>>("network.proxy.endpoints", vec![])? 
            .set_default("network.proxy.failure_threshold", 3)?
            .set_default("network.proxy.ban_duration_secs", 60)?
            .set_default("rate_limit.per_host_min_interval_ms", 250)?
            .set_default("rate_limit.global_min_interval_ms", 250)?
            .set_default("rate_limit.jitter_ms", 250)?
            .set_default("monitor.interval_secs", 30)?
            .set_default("monitor.task_timeout_secs", 60)?
            .set_default("quality.enabled", true)?
            .set_default("quality.min_bytes", 32)?
            .set_default("quality.max_bytes", 1024 * 1024)?
            .set_default("quality.reject_on_missing_required", false)?
            .set_default::<_, Vec<String>>("quality.blocked_keywords", vec![])? 
            .set_default::<_, Vec<String>>("quality.required_keywords", vec![])? 
            .set_default("quality.normalize_whitespace", true)?
            .set_default("telemetry.level", "info")?
            .set_default("dependencies.enabled", false)?
            .set_default("dependencies.interval_secs", 30)?
            .set_default("dependencies.default_timeout_secs", 5)?
            .set_default("events.capacity", 1024)?
            .set_default("events.sse_keep_alive_secs", 15)?
            .add_source(File::with_name("collector").required(false))
            .add_source(Environment::with_prefix("COLLECTOR").separator("__"));

        let mut config: CollectorConfig = builder.build()?.try_deserialize()?;
        if config.network.header_profiles.is_empty() {
            config.network.header_profiles = default_header_profiles();
        }
        Ok(config)
    }
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self::new(axum::http::StatusCode::BAD_REQUEST, message)
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self::new(axum::http::StatusCode::NOT_FOUND, message)
    }

    fn internal(message: impl Into<String>) -> Self {
        Self::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, message)
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        ApiError::internal(err.to_string())
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let payload = Json(serde_json::json!({
            "success": false,
            "error": self.message,
        }));
        (self.status, payload).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::runtime::Runtime;

    #[test]
    fn test_config_defaults() {
        let config = CollectorConfig::load().expect("config loads");
        assert_eq!(config.scheduler.max_retries, 3);
        assert!(config.network.body_preview > 0);
    }

    #[test]
    fn test_state_enqueues_task() {
        Runtime::new().unwrap().block_on(async {
            let (tx, mut rx) = mpsc::channel(1);
            let metrics_handle = init_metrics_recorder().expect("metrics recorder");
            let event_bus = Arc::new(EventBus::new(32));
            let state = Arc::new(CollectorState::new(
                CollectorConfig::load().unwrap(),
                tx,
                metrics_handle,
                event_bus,
            ));
            let request = NewTaskRequest {
                url: "https://example.com".to_string(),
                method: None,
                headers: None,
                body: None,
                priority: None,
                max_retries: None,
                tags: None,
                source: None,
            };
            let id = state.enqueue_task(request).await.unwrap();
            assert!(state.get_task(id).await.is_some());
            assert!(rx.recv().await.is_some());
        });
    }

    #[test]
    fn test_requeue_timed_out_task() {
        Runtime::new().unwrap().block_on(async {
            let (tx, mut rx) = mpsc::channel(1);
            let config = CollectorConfig::load().unwrap();
            let metrics_handle = init_metrics_recorder().expect("metrics recorder");
            let event_bus = Arc::new(EventBus::new(32));
            let state = Arc::new(CollectorState::new(
                config.clone(),
                tx,
                metrics_handle,
                event_bus,
            ));
            let request = NewTaskRequest {
                url: "https://example.com".to_string(),
                method: None,
                headers: None,
                body: None,
                priority: None,
                max_retries: Some(2),
                tags: None,
                source: None,
            };
            let id = state.enqueue_task(request).await.unwrap();
            {
                let mut guard = state.tasks.write().await;
                let record = guard.get_mut(&id).unwrap();
                record.status = TaskStatus::Running;
                record.started_at = Some(Utc::now() - chrono::Duration::seconds(600));
            }

            let requeued = state
                .requeue_timed_out_tasks(chrono::Duration::seconds(60))
                .await;
            assert_eq!(requeued, 1);
            assert!(rx.recv().await.is_some());
        });
    }
}
