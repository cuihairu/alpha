//! 业务级智能告警规则引擎（纯函数，TODO L503）
//!
//! 与运维面告警（Prometheus 规则 + Alertmanager，见 docs/alerting-and-diagnosis.md）
//! 互补：本模块是**用户自定义行情告警**的评估语义——规则定义（价格阈值/
//! 涨跌幅/均线交叉）、评估（对行情快照逐规则判定）、冷却抑制（触发后
//! cooldown 窗口内不重复报）、个性化分组（规则带 owner，事件按 owner
//! 归批供推送通道投递）。
//!
//! 设计约定：
//! - 评估显式传 `now`（快照 ts_ms），不含时钟读取与随机性——同输入序必
//!   同输出序，测试与回放可复现；
//! - 规则即数据（serde 判别式，条件 flatten 进规则体）：CRUD/订阅面可
//!   原样过网线，服务端存取与客户端展示共用同一形状；
//! - 投递通道（WS 推送/APNs/FCM/webhook）不在本层，服务侧拿
//!   `NotificationBatch` 自行接通道；
//! - `AlertEngine` 持有冷却与均线交叉的推进状态（单线程按时间序推进，
//!   语义上仍是「同输入序必同输出序」），冷却是**按规则**的——同 owner
//!   不同规则互不抑制。
//!
//! 口径细节见各条件文档。

use crate::indicators::TechnicalIndicators;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 行情快照单点（服务侧从 RealTimeQuote/历史序列投影；评估不改写）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotePoint {
    pub symbol: String,
    pub price: f64,
    /// 毫秒 epoch 时间戳（评估时钟：冷却窗口与事件时间都以此为准）
    pub ts_ms: i64,
    /// 截至该点的滚动收盘窗口（旧→新；均线交叉与窗口涨跌幅的数据源）
    #[serde(default)]
    pub closes: Vec<f64>,
}

/// 用户告警规则（判别式；`owner` 即订阅方标识，推送按 owner 分组）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertRule {
    pub id: String,
    pub owner: String,
    #[serde(flatten)]
    pub condition: AlertCondition,
    /// 触发后的抑制窗口（毫秒）：窗口内同规则不重复触发
    pub cooldown_ms: i64,
}

/// 告警条件（serde tag 无 rename，线上为 variant 原名 PascalCase，
/// 与 protocols::websocket 同风格）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AlertCondition {
    /// 价格触及上限：`price >= threshold`（含等于的「触及」口径）
    PriceAbove { symbol: String, threshold: f64 },
    /// 价格跌破下限：`price <= threshold`
    PriceBelow { symbol: String, threshold: f64 },
    /// 窗口涨跌幅绝对值达阈：|(price − closes[0]) / closes[0]| ≥ pct%，
    /// 以窗口首收盘为基价（窗口口径由快照提供方决定，如「近 N 收盘」）；
    /// 基价 0 或窗口不足 2 收盘时静默不判定。
    PctChangeAbove { symbol: String, pct: f64 },
    /// 均线交叉：短均线金叉（自下而上穿越长线）或死叉（自上而下）。
    /// 判定 = 相对关系**翻转**（前值严格一侧 → 当前严格另一侧），贴线
    /// （差值为 0）不算交叉、同向延续不重复报。前值来源优先引擎推进
    /// 状态（上一拍快照），冷启（首拍）用本快照内前一根兜底。收盘窗口
    /// 不足 `long` 期或 `short >= long` 时静默不判定（非错误）。
    SmaCross {
        symbol: String,
        short: usize,
        long: usize,
        direction: CrossDirection,
    },
}

/// 均线交叉方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrossDirection {
    /// 金叉：短线上穿长线
    GoldenCross,
    /// 死叉：短线下穿长线
    DeathCross,
}

/// 一次触发（dedup_key = 规则 id，冷却抑制的判定键）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertEvent {
    pub rule_id: String,
    pub owner: String,
    pub symbol: String,
    /// 人读消息（服务侧可直接投递；模板在本层定死，翻译层留后续项）
    pub message: String,
    pub ts_ms: i64,
}

/// 按 owner 归批的推送载荷（个性化投递通道的最小契约）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotificationBatch {
    pub owner: String,
    pub events: Vec<AlertEvent>,
}

/// 单规则判定产出：`message` = 条件命中的可投递消息；`spread` = 均线
/// 交叉规则的当拍差值符号（每拍必登记进引擎状态，与是否触发、是否被
/// 冷却抑制无关——「上一拍」随时间推进而非随触发推进）。
type RuleOutcome = (Option<String>, Option<i8>);

/// 告警引擎：规则集 + 各规则的冷却与交叉推进状态
#[derive(Debug, Default)]
pub struct AlertEngine {
    rules: Vec<AlertRule>,
    /// rule_id → 最近触发 ts_ms（冷却抑制）
    last_fired: HashMap<String, i64>,
    /// rule_id → 上一拍 short−long 差值符号（1 短在上 / -1 短在下 /
    /// 0 贴线或不可判定）
    prev_spread: HashMap<String, i8>,
}

impl AlertEngine {
    pub fn new(rules: Vec<AlertRule>) -> Self {
        Self {
            rules,
            last_fired: HashMap::new(),
            prev_spread: HashMap::new(),
        }
    }

    pub fn rules(&self) -> &[AlertRule] {
        &self.rules
    }

    /// 对一条行情快照评估全部规则；返回本轮触发事件（已过冷却抑制）。
    /// 同一快照内多规则命中全部产出（互不抑制）；symbol 不匹配的规则
    /// 静默跳过；交叉状态每拍推进。
    pub fn evaluate(&mut self, point: &QuotePoint) -> Vec<AlertEvent> {
        let mut events = Vec::new();
        for rule in &self.rules {
            let (message, spread) = self.evaluate_rule(rule, point);
            if let Some(sign) = spread {
                self.prev_spread.insert(rule.id.clone(), sign);
            }
            let Some(message) = message else {
                continue;
            };
            // 冷却抑制：窗口内同规则不重复触发（按 ts_ms 差值，负值同样抑制
            // ——时间戳必须按拍单调，乱序输入按「仍在窗口内」保守处理）
            if let Some(last) = self.last_fired.get(&rule.id) {
                if point.ts_ms - *last < rule.cooldown_ms {
                    continue;
                }
            }
            self.last_fired.insert(rule.id.clone(), point.ts_ms);
            events.push(AlertEvent {
                rule_id: rule.id.clone(),
                owner: rule.owner.clone(),
                symbol: point.symbol.clone(),
                message,
                ts_ms: point.ts_ms,
            });
        }
        events
    }

    /// 单规则判定：`Some(消息)` = 条件命中。不动引擎状态（登记归
    /// evaluate 统一推进，保证「每拍登记」不因触发/抑制路径分叉）。
    fn evaluate_rule(&self, rule: &AlertRule, point: &QuotePoint) -> RuleOutcome {
        match &rule.condition {
            AlertCondition::PriceAbove { symbol, threshold } => {
                let hit = point.symbol == *symbol && point.price >= *threshold;
                (
                    hit.then(|| {
                        format!("{} 价格 {} 触及上限阈值 {}", symbol, point.price, threshold)
                    }),
                    None,
                )
            }
            AlertCondition::PriceBelow { symbol, threshold } => {
                let hit = point.symbol == *symbol && point.price <= *threshold;
                (
                    hit.then(|| {
                        format!("{} 价格 {} 跌破下限阈值 {}", symbol, point.price, threshold)
                    }),
                    None,
                )
            }
            AlertCondition::PctChangeAbove { symbol, pct } => {
                if point.symbol != *symbol || point.closes.len() < 2 {
                    return (None, None);
                }
                let first = point.closes[0];
                if first == 0.0 {
                    return (None, None); // 基价 0 无涨跌幅语义
                }
                let change_pct = (point.price - first) / first.abs() * 100.0;
                let hit = change_pct.abs() >= *pct;
                (
                    hit.then(|| {
                        format!("{} 窗口涨跌幅 {:+.2}% 达阈值 ±{}%", symbol, change_pct, pct)
                    }),
                    None,
                )
            }
            AlertCondition::SmaCross {
                symbol,
                short,
                long,
                direction,
            } => {
                if point.symbol != *symbol || short >= long || point.closes.len() < *long {
                    return (None, None); // 窗口不足或参数非法：静默不判定
                }
                let indicators = TechnicalIndicators::default();
                // 根位 idx 处的两均线差值符号（对前缀 [..idx+1] 重算取尾）；
                // idx < long-1 时 SMA(long) 尾元素是 0 填充假值，返回 None。
                let spread_at = |idx: usize| -> Option<i8> {
                    if idx < long - 1 {
                        return None;
                    }
                    let prefix = &point.closes[..idx + 1];
                    let s = indicators.calculate_sma(prefix, *short).last().copied()?;
                    let l = indicators.calculate_sma(prefix, *long).last().copied()?;
                    Some(sign_of(s - l))
                };
                let now_sign = spread_at(point.closes.len() - 1);
                let Some(now_sign) = now_sign else {
                    return (None, None);
                };
                // 前值：引擎已登记（上一拍）则用之；否则快照内前一根兜底
                // （冷启不丢金叉）。登记值 0（贴线）不顶替快照内前一根——
                // 贴线本身不构成翻转参照。
                let baseline = match self.prev_spread.get(&rule.id) {
                    Some(prev) if *prev != 0 => *prev,
                    // 快照内前一根兜底（窗口不足两根可判定位时无从兜底 → 0）
                    _ => point
                        .closes
                        .len()
                        .checked_sub(2)
                        .and_then(spread_at)
                        .unwrap_or(0),
                };
                let crossed = match direction {
                    CrossDirection::GoldenCross => baseline < 0 && now_sign > 0,
                    CrossDirection::DeathCross => baseline > 0 && now_sign < 0,
                };
                let message = crossed.then(|| {
                    let name = match direction {
                        CrossDirection::GoldenCross => "金叉",
                        CrossDirection::DeathCross => "死叉",
                    };
                    format!("{} SMA({}/{}) {}", symbol, short, long, name)
                });
                (message, Some(now_sign))
            }
        }
    }
}

/// 符号归约（交叉判定用；NaN 视作不可判定 0）
fn sign_of(x: f64) -> i8 {
    if x.is_nan() || x == 0.0 {
        0
    } else if x > 0.0 {
        1
    } else {
        -1
    }
}

/// 事件按 owner 归批（批内保持触发顺序；批间顺序不构成契约）
pub fn group_by_owner(events: &[AlertEvent]) -> Vec<NotificationBatch> {
    let mut order: Vec<String> = Vec::new();
    let mut grouped: HashMap<String, Vec<AlertEvent>> = HashMap::new();
    for event in events {
        if !grouped.contains_key(&event.owner) {
            order.push(event.owner.clone());
        }
        grouped
            .entry(event.owner.clone())
            .or_default()
            .push(event.clone());
    }
    order
        .into_iter()
        .map(|owner| NotificationBatch {
            events: grouped.remove(&owner).unwrap_or_default(),
            owner,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: &str, owner: &str, condition: AlertCondition) -> AlertRule {
        AlertRule {
            id: id.to_string(),
            owner: owner.to_string(),
            condition,
            cooldown_ms: 60_000,
        }
    }

    fn point(symbol: &str, price: f64, closes: Vec<f64>) -> QuotePoint {
        QuotePoint {
            symbol: symbol.to_string(),
            price,
            ts_ms: 1_000,
            closes,
        }
    }

    #[test]
    fn price_thresholds_fire_on_breach_side_only() {
        let mut engine = AlertEngine::new(vec![
            rule(
                "up",
                "u1",
                AlertCondition::PriceAbove {
                    symbol: "600519".into(),
                    threshold: 1700.0,
                },
            ),
            rule(
                "down",
                "u1",
                AlertCondition::PriceBelow {
                    symbol: "600519".into(),
                    threshold: 1600.0,
                },
            ),
        ]);
        // 区间内：双双静默
        assert!(engine.evaluate(&point("600519", 1650.0, vec![])).is_empty());
        // 上限命中（>= 含等于的「触及」）
        let events = engine.evaluate(&point("600519", 1700.0, vec![]));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].rule_id, "up");
        // 下限命中（不同规则，冷却互不影响）
        let events = engine.evaluate(&point("600519", 1599.9, vec![]));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].rule_id, "down");
        // 其他 symbol 不触发任何规则
        assert!(engine.evaluate(&point("000001", 1.0, vec![])).is_empty());
    }

    #[test]
    fn cooldown_suppresses_repeat_within_window_only() {
        let mut engine = AlertEngine::new(vec![rule(
            "r",
            "u1",
            AlertCondition::PriceAbove {
                symbol: "X".into(),
                threshold: 10.0,
            },
        )]);
        assert_eq!(engine.evaluate(&point("X", 11.0, vec![])).len(), 1);
        // 窗口内重复越限：抑制
        assert!(engine.evaluate(&point("X", 12.0, vec![])).is_empty());
        // 窗口边界（ts − last == cooldown）：放行
        let mut late = point("X", 13.0, vec![]);
        late.ts_ms = 1_000 + 60_000;
        assert_eq!(engine.evaluate(&late).len(), 1);
        // 冷却重置后再进窗：再抑制
        assert!(engine.evaluate(&point("X", 14.0, vec![])).is_empty());
    }

    #[test]
    fn pct_change_uses_window_first_close_as_base() {
        let mk = || {
            AlertEngine::new(vec![rule(
                "pct",
                "u1",
                AlertCondition::PctChangeAbove {
                    symbol: "X".into(),
                    pct: 5.0,
                },
            )])
        };
        // 窗口基价 100，现价 105.4 → +5.4% 达阈（独立引擎防冷却空转断言）
        assert_eq!(
            mk().evaluate(&point("X", 105.4, vec![100.0, 102.0, 104.0]))
                .len(),
            1
        );
        // +4.99%：不触发
        assert!(mk()
            .evaluate(&point("X", 104.99, vec![100.0, 102.0, 104.0]))
            .is_empty());
        // 负向 −5.2%：绝对值口径触发
        assert_eq!(mk().evaluate(&point("X", 94.8, vec![100.0, 97.0])).len(), 1);
        // 窗口不足 2 收盘：静默；基价 0：静默
        assert!(mk().evaluate(&point("X", 200.0, vec![100.0])).is_empty());
        assert!(mk().evaluate(&point("X", 200.0, vec![0.0, 1.0])).is_empty());
    }

    /// 金叉判定：下跌段（短在下）首拍只登记，反转上穿触发；同向延续
    /// 不重复报。死叉镜像；窗口不足静默。
    #[test]
    fn sma_cross_detects_flip_not_persistence() {
        let mut engine = AlertEngine::new(vec![rule(
            "gx",
            "u1",
            AlertCondition::SmaCross {
                symbol: "X".into(),
                short: 2,
                long: 3,
                direction: CrossDirection::GoldenCross,
            },
        )]);
        // 下跌段：短均线在长线下方，首拍只登记（快照内前一根兜底也为负）
        let falling = vec![30.0, 28.0, 26.0, 24.0];
        assert!(engine
            .evaluate(&point("X", 24.0, falling.clone()))
            .is_empty());
        // 反转：SMA2(27) 上穿 SMA3(26)：金叉触发
        let mut turning = falling.clone();
        turning.extend_from_slice(&[26.0, 28.0]);
        let events = engine.evaluate(&point("X", 28.0, turning));
        assert_eq!(events.len(), 1);
        assert!(events[0].message.contains("金叉"));
        // 同向延续：不再触发
        let mut rising = falling;
        rising.extend_from_slice(&[26.0, 28.0, 30.0, 32.0]);
        assert!(engine.evaluate(&point("X", 32.0, rising)).is_empty());

        // 死叉镜像：上升段后短线下穿
        let mut death_engine = AlertEngine::new(vec![rule(
            "dx",
            "u1",
            AlertCondition::SmaCross {
                symbol: "X".into(),
                short: 2,
                long: 3,
                direction: CrossDirection::DeathCross,
            },
        )]);
        let rising = vec![20.0, 22.0, 24.0, 26.0];
        assert!(death_engine
            .evaluate(&point("X", 26.0, rising.clone()))
            .is_empty());
        let mut turning_down = rising;
        turning_down.extend_from_slice(&[24.0, 22.0]);
        let events = death_engine.evaluate(&point("X", 22.0, turning_down));
        assert_eq!(events.len(), 1);
        assert!(events[0].message.contains("死叉"));

        // 窗口不足 long 期：静默（非错误）
        let mut short_window = AlertEngine::new(vec![rule(
            "sw",
            "u1",
            AlertCondition::SmaCross {
                symbol: "X".into(),
                short: 2,
                long: 3,
                direction: CrossDirection::GoldenCross,
            },
        )]);
        assert!(short_window
            .evaluate(&point("X", 10.0, vec![9.0, 10.0]))
            .is_empty());
    }

    #[test]
    fn events_group_by_owner_preserving_order() {
        let mk_event = |rule_id: &str, owner: &str, ts_ms: i64| AlertEvent {
            rule_id: rule_id.into(),
            owner: owner.into(),
            symbol: "X".into(),
            message: "m".into(),
            ts_ms,
        };
        let events = vec![
            mk_event("a", "bob", 1),
            mk_event("b", "alice", 2),
            mk_event("c", "bob", 3),
        ];
        let batches = group_by_owner(&events);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].owner, "bob");
        assert_eq!(batches[0].events.len(), 2);
        assert_eq!(batches[1].owner, "alice");
        assert_eq!(batches[1].events.len(), 1);
        assert!(group_by_owner(&[]).is_empty());
    }

    /// 规则 serde 形状：条件 flatten 进规则体 + PascalCase tag（服务端
    /// 存取/客户端展示共用形状的契约锚）。
    #[test]
    fn rule_serializes_with_flat_condition() {
        let value = serde_json::to_value(rule(
            "r1",
            "u1",
            AlertCondition::PriceAbove {
                symbol: "600519".into(),
                threshold: 1700.0,
            },
        ))
        .unwrap();
        assert_eq!(value["id"], "r1");
        assert_eq!(value["owner"], "u1");
        assert_eq!(value["type"], "PriceAbove");
        assert_eq!(value["symbol"], "600519");
        assert_eq!(value["threshold"], 1700.0);
        assert_eq!(value["cooldown_ms"], 60_000);
        let back: AlertRule = serde_json::from_value(value).unwrap();
        assert_eq!(
            back.condition,
            AlertCondition::PriceAbove {
                symbol: "600519".into(),
                threshold: 1700.0,
            }
        );
    }

    /// 同快照多规则命中全部产出（互不抑制），随后重放被各自冷却抑制
    /// ——冷却是按规则的。
    #[test]
    fn same_snapshot_multi_rule_all_fire_then_cooldown() {
        let mut engine = AlertEngine::new(vec![
            rule(
                "a",
                "u1",
                AlertCondition::PriceAbove {
                    symbol: "X".into(),
                    threshold: 10.0,
                },
            ),
            rule(
                "b",
                "u1",
                AlertCondition::PctChangeAbove {
                    symbol: "X".into(),
                    pct: 5.0,
                },
            ),
        ]);
        let events = engine.evaluate(&point("X", 11.0, vec![10.0, 10.5]));
        assert_eq!(events.len(), 2, "both rules fire on the same snapshot");
        assert!(engine
            .evaluate(&point("X", 12.0, vec![10.0, 10.5]))
            .is_empty());
    }
}
