use crate::{AppState, RealTimeData};
use alpha_core::alerts::{AlertRule, QuotePoint};

pub(crate) const ALERT_CLOSES_CAP: usize = 64;

/// 告警规则装配（L503）：`ALPHA_ALERT_RULES` 为 AlertRule JSON 数组，
/// 与 `alpha_core::alerts` 序列化形状一致（规则即数据）。
/// 缺失/空/非法 = 无规则（评估面零行为，仅窗口滚动）。
pub(crate) fn load_alert_rules() -> Vec<AlertRule> {
    let raw = std::env::var("ALPHA_ALERT_RULES").unwrap_or_default();
    load_alert_rules_from(&raw)
}

pub(crate) fn load_alert_rules_from(raw: &str) -> Vec<AlertRule> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    match serde_json::from_str::<Vec<AlertRule>>(raw) {
        Ok(rules) => {
            tracing::info!("Loaded {} alert rule(s)", rules.len());
            rules
        }
        Err(err) => {
            tracing::warn!("ALPHA_ALERT_RULES invalid, alerts disabled: {}", err);
            Vec::new()
        }
    }
}

/// 业务告警评估（L503）：行情扇出同拍推进——滚动收盘窗口 → QuotePoint →
/// 引擎评估，命中事件走 alerts 广播（`Data{channel:"alerts"}`，owner 贯穿
/// 载荷；无订阅者/慢订阅者即丢弃，告警不回压行情面）。规则空时引擎恒
/// 静默，仅窗口滚动（O(1)）。
pub(crate) fn evaluate_alerts(app_state: &AppState, data: &RealTimeData) {
    let closes = {
        let mut windows = app_state.alert_closes.lock().unwrap();
        let closes = windows.entry(data.symbol.clone()).or_default();
        closes.push(data.price);
        if closes.len() > ALERT_CLOSES_CAP {
            closes.remove(0);
        }
        closes.clone()
    };
    let point = QuotePoint {
        symbol: data.symbol.clone(),
        price: data.price,
        ts_ms: data.timestamp.timestamp_millis(),
        closes,
    };
    let events = app_state.alert_engine.lock().unwrap().evaluate(&point);
    for event in events {
        let _ = app_state.alert_sender.send(event);
    }
}
