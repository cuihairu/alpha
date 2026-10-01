//! 推送通知决策（L337，docs/mobile-push-sync.md）
//!
//! 分层纪律：**核心库决定推什么（规则评估 + 穿越去重），壳层决定怎么送达**
//! （Android NotificationManager / iOS UNUserNotificationCenter）。送达面经
//! [`NotificationChannel`] 可插拔——默认 [`LocalQueueChannel`]（入队，壳层经
//! FFI `take_pending_json` 取走）；远程通道（FCM/APNs）与任何云推送**不引入**，
//! 留作后续 trait 实现位。与 alpha-core `platform::UserNotification` 不互相
//! 依赖（那是四端语义面，这是移动端送达面，见文档 §2）。

use alpha_core::models::MarketData;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// 通知类型（首期仅价格告警；`SignalChange` 智能告警归 L418，见文档 §4）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationKind {
    /// 价格穿越告警
    PriceAlert,
}

/// 送达壳层的通知载荷（字段契约由单测点名，docs/mobile-push-sync.md §4）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotificationSpec {
    /// 稳定去重键（规则键派生，同规则同穿越只发一次）
    pub id: String,
    /// 类型
    pub kind: NotificationKind,
    /// 标的代码
    pub symbol: String,
    /// 标题（壳层展示）
    pub title: String,
    /// 正文（壳层展示）
    pub body: String,
    /// 触发时刻
    pub created_at: DateTime<Utc>,
}

/// 价格告警规则（整体替换式配置，持久化归 L339 离线存储）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertRule {
    /// 标的（须在观察列表——经 FFI 设置时校验）
    pub symbol: String,
    /// 目标价
    pub target_price: f64,
    /// `true` = 现价 ≥ 目标价（上穿）；`false` = 现价 ≤ 目标价（下穿）
    pub above: bool,
}

impl AlertRule {
    /// 稳定规则键：标的 + 方向 + 目标价位模式（`to_bits` 避开浮点格式漂移）
    pub fn rule_id(&self) -> String {
        format!(
            "{}|{}|{:016x}",
            self.symbol,
            if self.above { "above" } else { "below" },
            self.target_price.to_bits()
        )
    }

    /// 条件是否成立（含等于——边界触发与桌面 alerts 口径一致）
    pub fn triggered(&self, price: f64) -> bool {
        if self.above {
            price >= self.target_price
        } else {
            price <= self.target_price
        }
    }
}

/// 送达通道（可插拔抽象：本地通知为默认，远程/自建留实现位）
pub trait NotificationChannel: Send + Sync {
    /// 送一条；失败返回错误文案，由调用方决定降级（骨架不带重试，§10⑤）
    fn send(&self, spec: &NotificationSpec) -> Result<(), String>;
}

impl<T: NotificationChannel + ?Sized> NotificationChannel for Arc<T> {
    fn send(&self, spec: &NotificationSpec) -> Result<(), String> {
        (**self).send(spec)
    }
}

/// 默认通道：入队待壳层 `take_pending_json` 取走（平台送达在壳层完成）
#[derive(Debug, Default)]
pub struct LocalQueueChannel {
    queue: Mutex<Vec<NotificationSpec>>,
}

impl NotificationChannel for LocalQueueChannel {
    fn send(&self, spec: &NotificationSpec) -> Result<(), String> {
        self.queue.lock().expect("通知队列锁").push(spec.clone());
        Ok(())
    }
}

impl LocalQueueChannel {
    /// 当前待送达条数
    pub fn pending(&self) -> usize {
        self.queue.lock().expect("通知队列锁").len()
    }

    /// 取走全部（取走即空，壳层负责平台送达）
    pub fn take_all(&self) -> Vec<NotificationSpec> {
        std::mem::take(&mut *self.queue.lock().expect("通知队列锁"))
    }
}

/// 通知决策器：规则集 + 已发集 + 送达通道
///
/// 去重语义（文档 §5）：**穿越一次发一次**——条件成立且未发 → 发并记入已发集；
/// 仍成立不重发；回落 → 移出已发集（再次穿越可再发），防通知轰炸。
#[derive(Debug)]
pub struct Notifier<C = LocalQueueChannel> {
    rules: Vec<AlertRule>,
    fired: HashSet<String>,
    channel: C,
}

impl<C: NotificationChannel> Notifier<C> {
    /// 以给定通道构建
    pub fn new(channel: C) -> Self {
        Self {
            rules: Vec::new(),
            fired: HashSet::new(),
            channel,
        }
    }

    /// 整体替换规则（已发集重置——新规则集重新武装）
    pub fn set_rules(&mut self, rules: Vec<AlertRule>) {
        self.rules = rules;
        self.fired.clear();
    }

    /// 当前规则集
    pub fn rules(&self) -> &[AlertRule] {
        &self.rules
    }

    /// 送达通道（默认队列经此取用）
    pub fn channel(&self) -> &C {
        &self.channel
    }

    /// 评估一轮（时间与行情由调用方注入，便于单测确定性断言）
    ///
    /// 无对应行情的规则跳过（FFI 设置时已拦观察列表外，此处为防御）；
    /// 通道送失败的规则不记已发（下一轮可重试）。
    pub fn evaluate_at(
        &mut self,
        quotes: &[MarketData],
        now: DateTime<Utc>,
    ) -> Vec<NotificationSpec> {
        let mut fired_now = Vec::new();
        for rule in self.rules.clone() {
            let Some(price) = quotes
                .iter()
                .find(|quote| quote.symbol == rule.symbol)
                .map(|quote| quote.price)
            else {
                continue;
            };
            let id = rule.rule_id();
            if !rule.triggered(price) {
                self.fired.remove(&id);
                continue;
            }
            if self.fired.contains(&id) {
                continue;
            }
            let spec = NotificationSpec {
                id: id.clone(),
                kind: NotificationKind::PriceAlert,
                symbol: rule.symbol.clone(),
                title: format!("价格提醒 {}", rule.symbol),
                body: if rule.above {
                    format!(
                        "{} 现价 {:.2} 已突破目标价 {:.2}",
                        rule.symbol, price, rule.target_price
                    )
                } else {
                    format!(
                        "{} 现价 {:.2} 已跌破目标价 {:.2}",
                        rule.symbol, price, rule.target_price
                    )
                },
                created_at: now,
            };
            match self.channel.send(&spec) {
                Ok(()) => {
                    self.fired.insert(id);
                    fired_now.push(spec);
                }
                Err(_) => { /* 送达失败不记已发，下一轮重试（文档 §10⑤） */ }
            }
        }
        fired_now
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn quote(symbol: &str, price: f64) -> MarketData {
        MarketData::new(symbol.to_string(), price, 1_000)
    }

    fn rule(symbol: &str, target: f64, above: bool) -> AlertRule {
        AlertRule {
            symbol: symbol.to_string(),
            target_price: target,
            above,
        }
    }

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_760_000_000, 0).expect("固定时刻")
    }

    /// 上穿：条件成立才发；`set_rules` 后首轮即评估
    #[test]
    fn above_rule_fires_when_crossed() {
        let mut notifier = Notifier::new(LocalQueueChannel::default());
        notifier.set_rules(vec![rule("600519", 90.0, true)]);
        let fired = notifier.evaluate_at(&[quote("600519", 91.0)], now());
        assert_eq!(fired.len(), 1, "上穿应触发");
        assert_eq!(fired[0].symbol, "600519");
        assert_eq!(fired[0].kind, NotificationKind::PriceAlert);
        assert!(
            fired[0].body.contains("突破"),
            "上穿文案: {}",
            fired[0].body
        );
    }

    /// 未达条件不发
    #[test]
    fn rule_stays_silent_below_condition() {
        let mut notifier = Notifier::new(LocalQueueChannel::default());
        notifier.set_rules(vec![rule("600519", 90.0, true)]);
        let fired = notifier.evaluate_at(&[quote("600519", 89.0)], now());
        assert!(fired.is_empty(), "未达条件不应触发");
    }

    /// 去重：仍处触发区间不重发（穿越一次发一次）
    #[test]
    fn fires_once_until_condition_resets() {
        let mut notifier = Notifier::new(LocalQueueChannel::default());
        notifier.set_rules(vec![rule("600519", 90.0, true)]);
        let quotes = [quote("600519", 91.0)];
        assert_eq!(notifier.evaluate_at(&quotes, now()).len(), 1);
        assert!(
            notifier.evaluate_at(&quotes, now()).is_empty(),
            "仍成立不重发"
        );
        assert!(
            notifier.evaluate_at(&quotes, now()).is_empty(),
            "多轮仍不重发"
        );
    }

    /// 回落重装：跌回条件下方再穿越 → 再发
    #[test]
    fn re_arms_after_condition_clears() {
        let mut notifier = Notifier::new(LocalQueueChannel::default());
        notifier.set_rules(vec![rule("600519", 90.0, true)]);
        let above = [quote("600519", 91.0)];
        let below = [quote("600519", 89.0)];
        assert_eq!(notifier.evaluate_at(&above, now()).len(), 1, "首次上穿");
        assert!(notifier.evaluate_at(&below, now()).is_empty(), "回落评估");
        assert_eq!(notifier.evaluate_at(&above, now()).len(), 1, "再次上穿重发");
    }

    /// 下穿对称：现价跌破目标价触发，文案「跌破」
    #[test]
    fn below_rule_fires_on_drop() {
        let mut notifier = Notifier::new(LocalQueueChannel::default());
        notifier.set_rules(vec![rule("000001", 10.0, false)]);
        let fired = notifier.evaluate_at(&[quote("000001", 9.5)], now());
        assert_eq!(fired.len(), 1);
        assert!(
            fired[0].body.contains("跌破"),
            "下穿文案: {}",
            fired[0].body
        );
    }

    /// 默认通道：入队可数、取走即空
    #[test]
    fn local_queue_channel_enqueues_and_drains() {
        let mut notifier = Notifier::new(LocalQueueChannel::default());
        notifier.set_rules(vec![rule("600519", 90.0, true)]);
        notifier.evaluate_at(&[quote("600519", 91.0)], now());
        assert_eq!(notifier.channel().pending(), 1, "评估即入队");
        let taken = notifier.channel().take_all();
        assert_eq!(taken.len(), 1);
        assert_eq!(notifier.channel().pending(), 0, "取走即空");
        assert!(notifier.channel().take_all().is_empty(), "重复取走为空");
    }

    /// 可插拔：换 RecordingChannel 不改决策逻辑（trait 抽象成立）
    #[test]
    fn pluggable_channel_receives_specs() {
        #[derive(Default)]
        struct RecordingChannel {
            received: Mutex<Vec<NotificationSpec>>,
        }
        impl NotificationChannel for RecordingChannel {
            fn send(&self, spec: &NotificationSpec) -> Result<(), String> {
                self.received.lock().expect("记录锁").push(spec.clone());
                Ok(())
            }
        }

        let recorder = Arc::new(RecordingChannel::default());
        let mut notifier = Notifier::new(recorder.clone());
        notifier.set_rules(vec![rule("600519", 90.0, true)]);
        notifier.evaluate_at(&[quote("600519", 91.0)], now());
        assert_eq!(recorder.received.lock().expect("记录锁").len(), 1);
        assert_eq!(
            notifier.channel().received.lock().expect("记录锁").len(),
            1,
            "决策器持有的就是注入的通道实例"
        );
    }

    /// 规则键稳定且区分方向与价位
    #[test]
    fn rule_id_is_stable_and_distinguishing() {
        let base = rule("600519", 90.0, true);
        assert_eq!(base.rule_id(), rule("600519", 90.0, true).rule_id());
        assert_ne!(
            base.rule_id(),
            rule("600519", 90.0, false).rule_id(),
            "方向"
        );
        assert_ne!(
            base.rule_id(),
            rule("600519", 90.01, true).rule_id(),
            "价位"
        );
        assert_ne!(base.rule_id(), rule("000001", 90.0, true).rule_id(), "标的");
    }

    /// 载荷 serde 字段契约（§4：六字段逐个点名）
    #[test]
    fn notification_spec_keeps_payload_contract() {
        let spec = NotificationSpec {
            id: "k".into(),
            kind: NotificationKind::PriceAlert,
            symbol: "600519".into(),
            title: "价格提醒 600519".into(),
            body: "正文".into(),
            created_at: now(),
        };
        let value: serde_json::Value = serde_json::to_value(&spec).expect("序列化");
        for key in ["id", "kind", "symbol", "title", "body", "created_at"] {
            assert!(value.get(key).is_some(), "缺字段 {key}: {value}");
        }
        assert_eq!(value["kind"], "PriceAlert", "枚举序列化为变体名字符串");
        let back: NotificationSpec = serde_json::from_value(value).expect("反序列化");
        assert_eq!(back, spec, "往返一致");
    }

    /// 无对应行情的规则跳过（防御，不 panic）
    #[test]
    fn rules_without_quotes_are_skipped() {
        let mut notifier = Notifier::new(LocalQueueChannel::default());
        notifier.set_rules(vec![rule("600519", 90.0, true)]);
        let fired = notifier.evaluate_at(&[quote("000001", 91.0)], now());
        assert!(fired.is_empty());
        assert_eq!(notifier.channel().pending(), 0);
    }

    /// 替换规则集重置已发集（新规则重新武装）
    #[test]
    fn set_rules_rearms_fired_state() {
        let mut notifier = Notifier::new(LocalQueueChannel::default());
        let quotes = [quote("600519", 91.0)];
        notifier.set_rules(vec![rule("600519", 90.0, true)]);
        assert_eq!(notifier.evaluate_at(&quotes, now()).len(), 1);
        notifier.set_rules(vec![rule("600519", 90.0, true)]);
        assert_eq!(
            notifier.evaluate_at(&quotes, now()).len(),
            1,
            "规则重设后重新武装"
        );
    }
}
