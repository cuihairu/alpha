//! 用户行为分析（L480）：事件计数 / 日活 / 会话切分 / 漏斗 / 留存。
//!
//! 与 `analytics.rs`（行情技术面）互补——本模块只消费行为事件流，不碰价格。
//! 全部纯函数、时间显式入参（事件自带 `ts_ms`，不读时钟）；无 I/O、无采样。
//! 上报通道（flush 到哪）是各端壳层职责（web 见 `lib/analytics.ts`），
//! 本模块只回答「给定事件流，指标是多少」。
//!
//! 事件属性（props）不建模：计数/漏斗/留存都不需要属性维度，
//! 要下钻时调用方按事件名分流（事件名即最细维度）。

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// 一天毫秒数（UTC 天边界，日活/留存分桶用）
pub const DAY_MS: i64 = 86_400_000;

/// 行为事件（上报契约：各端壳层按此形状采集，JSON 可序列化）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BehaviorEvent {
    /// 匿名用户标识（各端本地生成，不含个人身份）
    pub user: String,
    /// 事件名（调用方按名分流，如 `workspace.create`）
    pub name: String,
    /// 事件毫秒时间戳（UTC epoch）
    pub ts_ms: i64,
}

impl BehaviorEvent {
    pub fn new(user: &str, name: &str, ts_ms: i64) -> Self {
        Self {
            user: user.to_string(),
            name: name.to_string(),
            ts_ms,
        }
    }
}

/// 按事件名计数（总事件量口径，含重复触发）
pub fn count_by_name(events: &[BehaviorEvent]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for e in events {
        *counts.entry(e.name.clone()).or_insert(0) += 1;
    }
    counts
}

/// 日活（UTC 天 → 去重用户数；同一用户一天多事件只算一次）
pub fn daily_active(events: &[BehaviorEvent]) -> HashMap<i64, usize> {
    let mut days: HashMap<i64, HashSet<&str>> = HashMap::new();
    for e in events {
        days.entry(e.ts_ms.div_euclid(DAY_MS))
            .or_default()
            .insert(e.user.as_str());
    }
    days.into_iter()
        .map(|(d, users)| (d, users.len()))
        .collect()
}

/// 会话（同用户按时间排序，相邻间隔超 `gap_ms` 切新会话）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub user: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub events: usize,
}

pub fn sessionize(events: &[BehaviorEvent], gap_ms: i64) -> Vec<Session> {
    let mut by_user: HashMap<&str, Vec<i64>> = HashMap::new();
    for e in events {
        by_user.entry(e.user.as_str()).or_default().push(e.ts_ms);
    }
    let mut sessions = Vec::new();
    // 用户名字典序输出（确定性，与 grid_search tie-break 同纪律）
    let mut users: Vec<&str> = by_user.keys().copied().collect();
    users.sort();
    for user in users {
        let mut ts = by_user[user].clone();
        ts.sort();
        let mut start = ts[0];
        let mut prev = ts[0];
        let mut count = 1usize;
        for &t in &ts[1..] {
            if t - prev > gap_ms {
                sessions.push(Session {
                    user: user.to_string(),
                    start_ms: start,
                    end_ms: prev,
                    events: count,
                });
                start = t;
                count = 0;
            }
            prev = t;
            count += 1;
        }
        sessions.push(Session {
            user: user.to_string(),
            start_ms: start,
            end_ms: prev,
            events: count,
        });
    }
    sessions
}

/// 漏斗：每步返回完成该步（且按序完成之前各步）的去重用户数。
/// 空步骤表返回空；同用户重复完成只算一次。
pub fn funnel(events: &[BehaviorEvent], steps: &[&str]) -> Vec<usize> {
    if steps.is_empty() {
        return Vec::new();
    }
    let mut by_user: HashMap<&str, Vec<(&i64, &str)>> = HashMap::new();
    for e in events {
        by_user
            .entry(e.user.as_str())
            .or_default()
            .push((&e.ts_ms, e.name.as_str()));
    }
    let mut counts = vec![0usize; steps.len()];
    for streams in by_user.values() {
        let mut ordered = streams.clone();
        ordered.sort_by_key(|(ts, _)| *ts);
        let mut step = 0;
        for (_, name) in ordered {
            if step < steps.len() && name == steps[step] {
                step += 1;
            }
        }
        for c in counts.iter_mut().take(step) {
            *c += 1;
        }
    }
    counts
}

/// 次日（N 日）留存：首日活跃用户中第 N 天仍活跃的比例。
/// 无事件返回 None（分母不存在，不编 0）。
pub fn day_n_retention(events: &[BehaviorEvent], n: i64) -> Option<f64> {
    if events.is_empty() || n < 0 {
        return None;
    }
    let day0 = events.iter().map(|e| e.ts_ms.div_euclid(DAY_MS)).min()?;
    let cohort: HashSet<&str> = events
        .iter()
        .filter(|e| e.ts_ms.div_euclid(DAY_MS) == day0)
        .map(|e| e.user.as_str())
        .collect();
    if cohort.is_empty() {
        return None;
    }
    let retained = events
        .iter()
        .filter(|e| e.ts_ms.div_euclid(DAY_MS) == day0 + n)
        .map(|e| e.user.as_str())
        .filter(|u| cohort.contains(u))
        .collect::<HashSet<&str>>()
        .len();
    Some(retained as f64 / cohort.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(user: &str, name: &str, ts_ms: i64) -> BehaviorEvent {
        BehaviorEvent::new(user, name, ts_ms)
    }

    #[test]
    fn count_by_name_totals_repeats() {
        let events = vec![ev("u1", "a", 1), ev("u1", "a", 2), ev("u2", "b", 3)];
        let counts = count_by_name(&events);
        assert_eq!(counts["a"], 2);
        assert_eq!(counts["b"], 1);
        assert!(count_by_name(&[]).is_empty());
    }

    #[test]
    fn daily_active_dedupes_users_per_utc_day() {
        let events = vec![
            ev("u1", "a", 1000),
            ev("u1", "b", 2000), // 同天同用户只算一次
            ev("u2", "a", 3000),
            ev("u1", "a", DAY_MS + 500), // 次日
        ];
        let dau = daily_active(&events);
        assert_eq!(dau[&0], 2);
        assert_eq!(dau[&1], 1);
    }

    #[test]
    fn sessionize_splits_on_gap_and_orders_users() {
        let events = vec![
            ev("u2", "a", 10_000),
            ev("u1", "a", 0),
            ev("u1", "b", 1_000),
            ev("u1", "c", 100_000), // gap 99s > 30s 切分
        ];
        let sessions = sessionize(&events, 30_000);
        assert_eq!(sessions.len(), 3);
        // 用户名字典序：u1 两段在前
        assert_eq!(
            sessions[0],
            Session {
                user: "u1".into(),
                start_ms: 0,
                end_ms: 1_000,
                events: 2
            }
        );
        assert_eq!(
            sessions[1],
            Session {
                user: "u1".into(),
                start_ms: 100_000,
                end_ms: 100_000,
                events: 1
            }
        );
        assert_eq!(sessions[2].user, "u2");
        // 边界：间隔恰等于 gap 不切分
        let edge = sessionize(&[ev("u", "a", 0), ev("u", "b", 30_000)], 30_000);
        assert_eq!(edge.len(), 1);
    }

    #[test]
    fn funnel_requires_order_and_dedupes() {
        let events = vec![
            // u1 顺序完成三步（含重复）
            ev("u1", "view", 1),
            ev("u1", "view", 2),
            ev("u1", "add", 3),
            ev("u1", "buy", 4),
            // u2 乱序：buy 在 add 之前，只算到 view
            ev("u2", "view", 1),
            ev("u2", "buy", 2),
            // u3 只看
            ev("u3", "view", 1),
        ];
        assert_eq!(funnel(&events, &["view", "add", "buy"]), vec![3, 1, 1]);
        assert!(funnel(&events, &[]).is_empty());
    }

    #[test]
    fn day_n_retention_cohort_math() {
        // day0: u1,u2,u3；day1: u1,u3；day2: u1
        let events = vec![
            ev("u1", "a", 100),
            ev("u2", "a", 200),
            ev("u3", "a", 300),
            ev("u1", "a", DAY_MS + 100),
            ev("u3", "a", DAY_MS + 200),
            ev("u1", "a", 2 * DAY_MS + 100),
        ];
        assert_eq!(day_n_retention(&events, 1), Some(2.0 / 3.0));
        assert_eq!(day_n_retention(&events, 2), Some(1.0 / 3.0));
        assert_eq!(day_n_retention(&[], 1), None);
        assert_eq!(day_n_retention(&events, -1), None);
    }

    #[test]
    fn event_serde_roundtrip_for_sink_contract() {
        let e = ev("anon-7", "workspace.create", 1727856000000);
        let back: BehaviorEvent =
            serde_json::from_value(serde_json::to_value(&e).unwrap()).unwrap();
        assert_eq!(back, e);
    }
}
