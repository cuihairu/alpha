use super::*;
use crate::multilang_simple::CrawlerLanguage;
use crate::types::{AShareDataSource, TaskPriority, TaskSource};
use std::collections::HashMap;

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

/// 数据源健康面（L504）：台账记录 → 快照并集任务表（从未执行补
/// unknown）；删除任务同摘台账
#[tokio::test]
async fn sources_health_merges_tasks_and_ledger() {
    let collector = SimpleCollector::new("/tmp/test_collector_health");
    collector
        .tasks
        .write()
        .await
        .insert("t1".into(), seeded_task("t1", None));
    collector
        .tasks
        .write()
        .await
        .insert("t2".into(), seeded_task("t2", None));

    // 从未执行：unknown
    let snap = collector.get_sources_health().await;
    let t2 = snap.iter().find(|e| e["task_id"] == "t2").unwrap();
    assert_eq!(t2["state"], "unknown");
    assert_eq!(t2["consecutive_failures"], 0);

    // 失败 → degraded（含错误文本）；成功 → healthy
    collector.health.record_failure("t1", "源超时");
    collector.health.record_success("t2");
    let snap = collector.get_sources_health().await;
    let t1 = snap.iter().find(|e| e["task_id"] == "t1").unwrap();
    assert_eq!(t1["state"], "degraded");
    assert_eq!(t1["last_error"], "源超时");
    let t2 = snap.iter().find(|e| e["task_id"] == "t2").unwrap();
    assert_eq!(t2["state"], "healthy");

    // 删除任务摘台账
    collector.delete_task("t1").await.unwrap();
    let snap = collector.get_sources_health().await;
    assert!(snap.iter().all(|e| e["task_id"] != "t1"));
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

/// S-1 回归：URL/header 里的引号+语句逃逸必须收进字符串字面量，不得原样
/// 出现在生成脚本里（collector 生成面曾把 url 裸插进四种语言模板）。
#[test]
fn test_script_code_generation_neutralizes_injection() {
    let collector = SimpleCollector::new("/tmp");
    let mut task = TaskDefinition::new(
        "inject-script",
        TaskSource::Custom {
            source_type: "test".to_string(),
            endpoint: "https://api.example.com".to_string(),
            params: HashMap::new(),
        },
        "Inject Script",
    );
    let malicious_url = "\";import os;os.system('id');#";
    let mut headers = HashMap::new();
    headers.insert(
        "X-Test".to_string(),
        "\"};import os;os.system('id');#".to_string(),
    );
    task.config.request.url = malicious_url.to_string();
    task.config.request.headers = headers;

    // 关键断言：url 行必须是「一个完整字符串字面量」，即载荷全部被收进
    // 字面量内部（serde_json 转义所有引号/反斜杠/控制字符），无语句断句逃逸。
    let json_url = serde_json::to_string(malicious_url).unwrap();
    let json_headers = serde_json::to_string(&task.config.request.headers).unwrap();
    let shell_url = format!("'{}'", malicious_url.replace('\'', "'\\''"));

    let python_code = collector.generate_script_code(&task, &CrawlerLanguage::Python);
    assert!(python_code.contains(&format!("url = {}", json_url)));
    assert!(python_code.contains(&format!("headers = {}", json_headers)));

    let nodejs_code = collector.generate_script_code(&task, &CrawlerLanguage::NodeJs);
    assert!(nodejs_code.contains(&format!("const url = {};", json_url)));

    let go_code = collector.generate_script_code(&task, &CrawlerLanguage::Go);
    assert!(go_code.contains(&format!("url := {}", json_url)));

    let shell_code = collector.generate_script_code(&task, &CrawlerLanguage::Shell);
    assert!(shell_code.contains(&format!("URL={}", shell_url)));
}
