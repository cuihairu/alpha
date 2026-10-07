//! 数据源健康面（L504）：从采集任务执行结果推导三态健康。
//!
//! 数据可靠性是本仓采集层的差异化叙事（architecture-review §2.2），
//! health_check 是 SourceDefinition 收敛方向的最后一位字段——落法为
//! 「由执行结果推导」而非模板声明：数据源健康与否，本质是它的任务
//! 能否持续产出解析成功的结果，声明式字段反而制造双源真相。
//!
//! 语义（确定性、可告警）：
//! - 从未执行 = `unknown`（gauge 0）
//! - 连续失败 0 次 = `healthy`（gauge 1）
//! - 连续失败 1..=[`DEGRADED_THRESHOLD`] 次 = `degraded`（gauge 2）
//! - 连续失败 > [`DEGRADED_THRESHOLD`] 次 = `down`（gauge 3）
//!
//! 指标：`alpha_collector_source_health{task=...}` 单值 gauge，告警规则
//! 直接阈值比对（>=2 预警、>=3 告警）。架构 §6「观测爬虫成功率」的
//! 数据源维度落点。

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Serialize;

/// 降级阈值：连续失败达到该次数为 `degraded`，超过为 `down`
pub const DEGRADED_THRESHOLD: u32 = 3;

/// 单数据源健康条目（task_id 维度，由执行路径维护）
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SourceHealthEntry {
    /// 连续失败计数（成功即清零）
    pub consecutive_failures: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_failure_at: Option<DateTime<Utc>>,
    /// 最近一次失败原因（截断到 200 字符——异常响应体不撑爆快照）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// 三态健康（+unknown），serde 小写输出
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceHealthState {
    Unknown,
    Healthy,
    Degraded,
    Down,
}

impl SourceHealthState {
    /// Prometheus 单值编码（gauge），alert 规则按阈值比对
    pub fn metric_value(self) -> f64 {
        match self {
            SourceHealthState::Unknown => 0.0,
            SourceHealthState::Healthy => 1.0,
            SourceHealthState::Degraded => 2.0,
            SourceHealthState::Down => 3.0,
        }
    }
}

/// 由条目推导健康态（None = 台账无记录 = 从未执行）
pub fn health_state(entry: Option<&SourceHealthEntry>) -> SourceHealthState {
    match entry {
        None => SourceHealthState::Unknown,
        Some(e) if e.consecutive_failures == 0 => SourceHealthState::Healthy,
        Some(e) if e.consecutive_failures <= DEGRADED_THRESHOLD => SourceHealthState::Degraded,
        Some(_) => SourceHealthState::Down,
    }
}

/// 健康台账：执行路径 `record_success`/`record_failure`，快照经
/// `/sources/health` 暴露。同步 `Mutex`——记录路径无 await 点，
/// 锁内只有 clone/写条目，不跨 await 持锁。
#[derive(Debug, Default)]
pub struct SourceHealthTracker {
    entries: std::sync::Mutex<HashMap<String, SourceHealthEntry>>,
}

impl SourceHealthTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// 执行成功：清零连续失败、记最近成功时间
    pub fn record_success(&self, task_id: &str) {
        let mut map = self.entries.lock().expect("source health lock poisoned");
        let entry = map.entry(task_id.to_string()).or_default();
        entry.consecutive_failures = 0;
        entry.last_success_at = Some(Utc::now());
        entry.last_error = None;
        emit_gauge(task_id, health_state(Some(entry)));
    }

    /// 执行失败：累计连续失败、记最近失败时间与原因
    pub fn record_failure(&self, task_id: &str, error: &str) {
        let mut map = self.entries.lock().expect("source health lock poisoned");
        let entry = map.entry(task_id.to_string()).or_default();
        entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
        entry.last_failure_at = Some(Utc::now());
        entry.last_error = Some(error.chars().take(200).collect());
        emit_gauge(task_id, health_state(Some(entry)));
    }

    /// 任务删除：摘除台账条目并把 gauge 归零（unknown）——残留计数会让
    /// 快照虚高；gauge 标签序列在导出器侧无法移除，归零是可做的最诚实值
    pub fn remove(&self, task_id: &str) {
        let mut map = self.entries.lock().expect("source health lock poisoned");
        map.remove(task_id);
        emit_gauge(task_id, SourceHealthState::Unknown);
    }

    /// 台账快照（键 = task_id；从未执行的任务不在台账内，
    /// 由调用方并集任务表得出 unknown）
    pub fn snapshot(&self) -> HashMap<String, SourceHealthEntry> {
        self.entries
            .lock()
            .expect("source health lock poisoned")
            .clone()
    }
}

fn emit_gauge(task_id: &str, state: SourceHealthState) {
    metrics::gauge!("alpha_collector_source_health", "task" => task_id.to_string())
        .set(state.metric_value());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 成功清零、失败累计、阈值三态推导
    #[test]
    fn state_derivation_follows_consecutive_failures() {
        assert_eq!(health_state(None), SourceHealthState::Unknown);

        let mut entry = SourceHealthEntry::default();
        assert_eq!(health_state(Some(&entry)), SourceHealthState::Healthy);

        for i in 1..=DEGRADED_THRESHOLD {
            entry.consecutive_failures = i;
            assert_eq!(
                health_state(Some(&entry)),
                SourceHealthState::Degraded,
                "连续失败 {i} 次应为 degraded"
            );
        }
        entry.consecutive_failures = DEGRADED_THRESHOLD + 1;
        assert_eq!(health_state(Some(&entry)), SourceHealthState::Down);
    }

    /// 台账记录：成功清零失败计数与错误文本；失败累计并截断错误
    #[test]
    fn tracker_records_success_and_failure_lifecycles() {
        let tracker = SourceHealthTracker::new();

        tracker.record_failure("t1", "boom");
        tracker.record_failure("t1", "boom again");
        {
            let snap = tracker.snapshot();
            let e = snap.get("t1").unwrap();
            assert_eq!(e.consecutive_failures, 2);
            assert_eq!(e.last_error.as_deref(), Some("boom again"));
            assert!(e.last_failure_at.is_some());
            assert_eq!(health_state(Some(e)), SourceHealthState::Degraded);
        }

        tracker.record_success("t1");
        {
            let snap = tracker.snapshot();
            let e = snap.get("t1").unwrap();
            assert_eq!(e.consecutive_failures, 0, "成功清零连续失败");
            assert!(e.last_error.is_none(), "成功清除错误文本");
            assert_eq!(health_state(Some(e)), SourceHealthState::Healthy);
        }
    }

    /// 错误文本截断 200 字符 + remove 摘除条目
    #[test]
    fn tracker_truncates_error_and_removes_entries() {
        let tracker = SourceHealthTracker::new();
        let long_error = "x".repeat(500);
        tracker.record_failure("t2", &long_error);
        {
            let snap = tracker.snapshot();
            let e = snap.get("t2").unwrap();
            assert_eq!(e.last_error.as_ref().unwrap().chars().count(), 200);
        }

        tracker.remove("t2");
        assert!(!tracker.snapshot().contains_key("t2"), "删除后台账无残留");
    }

    /// 互相独立的 task_id 不串账
    #[test]
    fn tracker_keeps_tasks_independent() {
        let tracker = SourceHealthTracker::new();
        tracker.record_failure("a", "err");
        tracker.record_success("b");
        let snap = tracker.snapshot();
        assert_eq!(snap.get("a").unwrap().consecutive_failures, 1);
        assert_eq!(snap.get("b").unwrap().consecutive_failures, 0);
    }
}
