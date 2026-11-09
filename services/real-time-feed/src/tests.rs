use super::*;
use alpha_core::alerts::AlertRule;
use alpha_protocols::websocket::{channels, SyncOp, WsMessage};
use alpha_storage::StreamEnvelope;

use crate::alerts::{evaluate_alerts, load_alert_rules_from, ALERT_CLOSES_CAP};
use crate::stream::envelope_to_realtime;
use crate::sync::{
    next_versioned_frame, resync_full_snapshot, resync_reply, RESYNC_UNKNOWN_CHANNEL_CODE,
};

#[tokio::test]
async fn test_connection_manager() {
    let manager = ConnectionManager::new();
    let (tx, _rx) = broadcast::channel(10);

    manager.add_connection("test".to_string(), tx);
    assert_eq!(manager.get_connection_count(), 1);

    manager.remove_connection("test");
    assert_eq!(manager.get_connection_count(), 0);
}

#[tokio::test]
async fn test_real_time_data_serialization() {
    let data = RealTimeData {
        symbol: "AAPL".to_string(),
        price: 150.0,
        volume: 1000,
        change: 1.5,
        change_percent: 1.0,
        timestamp: chrono::Utc::now(),
    };

    let json = serde_json::to_string(&data).unwrap();
    let deserialized: RealTimeData = serde_json::from_str(&json).unwrap();

    assert_eq!(data.symbol, deserialized.symbol);
    assert_eq!(data.price, deserialized.price);
}

#[test]
fn test_parse_log_level_defaults_to_info_and_matches_data_engine() {
    assert_eq!(parse_log_level("info"), tracing::Level::INFO);
    assert_eq!(parse_log_level("DEBUG"), tracing::Level::DEBUG);
    assert_eq!(parse_log_level("Warn"), tracing::Level::WARN);
    assert_eq!(parse_log_level("error"), tracing::Level::ERROR);
    assert_eq!(parse_log_level("trace"), tracing::Level::TRACE);
    // 未设/未知取值兜底 INFO：保证 WARN 级兜底日志可见
    assert_eq!(parse_log_level(""), tracing::Level::INFO);
    assert_eq!(parse_log_level("bogus"), tracing::Level::INFO);
}

#[test]
fn test_envelope_to_realtime() {
    let envelope = StreamEnvelope::new(
        QUOTES_STREAM,
        "quote",
        "collector",
        Some("sz000001".to_string()),
        serde_json::json!({
            "symbol": "sz000001",
            "price": 12.34,
            "volume": 5000,
            "change": 0.12,
            "change_percent": 0.98
        }),
    );

    let data = envelope_to_realtime(&envelope).unwrap();
    assert_eq!(data.symbol, "sz000001");
    assert_eq!(data.price, 12.34);
}

#[test]
fn test_envelope_to_realtime_without_change_fields() {
    // data-engine 的 normalized payload 即 MarketData 序列化，不含 change/change_percent，
    // 不应被误判为无效消息进入 DLQ。
    let envelope = StreamEnvelope::new(
        NORMALIZED_QUOTES_STREAM,
        "normalized_quote",
        "data-engine",
        Some("sz000001".to_string()),
        serde_json::json!({
            "symbol": "sz000001",
            "timestamp": "2026-09-27T08:00:00Z",
            "price": 12.34,
            "volume": 5000,
            "bid": 12.3,
            "ask": 12.4,
            "open": 12.1,
            "high": 12.5,
            "low": 12.0
        }),
    );

    let data = envelope_to_realtime(&envelope).unwrap();
    assert_eq!(data.symbol, "sz000001");
    assert_eq!(data.price, 12.34);
    assert_eq!(data.volume, 5000);
    assert_eq!(data.change, 0.0);
    assert_eq!(data.change_percent, 0.0);
}

#[test]
fn test_envelope_to_realtime_rejects_missing_price() {
    let envelope = StreamEnvelope::new(
        QUOTES_STREAM,
        "quote",
        "collector",
        None,
        serde_json::json!({ "symbol": "sz000001", "volume": 5000 }),
    );

    assert!(envelope_to_realtime(&envelope).is_none());
}

/// 合成一条实时数据（price/volume 可变，其余字段固定；timestamp 固定保证
/// Delta 断言精确——增量只含受控变化字段）
fn sample_data(price: f64, volume: u64) -> RealTimeData {
    RealTimeData {
        symbol: "sz000001".to_string(),
        price,
        volume,
        change: 0.5,
        change_percent: 0.98,
        timestamp: chrono::DateTime::parse_from_rfc3339("2026-09-29T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc),
    }
}

/// 带告警面的测试态（其余字段与生产装配同构；timestamp 固定保证
/// 冷却窗口断言确定）
fn alert_app_state(rules: Vec<AlertRule>) -> (Arc<AppState>, broadcast::Receiver<AlertEvent>) {
    let (alert_sender, alert_receiver) = broadcast::channel(16);
    let state = Arc::new(AppState {
        connection_manager: ConnectionManager::new(),
        data_sender: broadcast::channel(16).0,
        alert_engine: Arc::new(Mutex::new(AlertEngine::new(rules))),
        alert_closes: Arc::new(Mutex::new(HashMap::new())),
        alert_sender,
        sync_state: Arc::new(Mutex::new(HashMap::new())),
    });
    (state, alert_receiver)
}

/// 规则装配：缺失/空白/非法一律回退空集（评估面零行为），合法 JSON
/// 按条件 flatten 形状解析
#[test]
fn test_alert_rules_loading() {
    assert!(load_alert_rules_from("").is_empty());
    assert!(load_alert_rules_from("   ").is_empty());
    assert!(load_alert_rules_from("not json").is_empty());
    let rules = load_alert_rules_from(
        r#"[{"id":"r1","owner":"u1","type":"PriceAbove","symbol":"X","threshold":10.0,"cooldown_ms":60000}]"#,
    );
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].id, "r1");
}

/// 告警评估：命中走 alerts 广播（owner 贯穿载荷），冷却窗口内重复
/// 越限抑制，滚动收盘窗口封顶
#[test]
fn test_alert_evaluation_broadcasts_and_suppresses() {
    use alpha_core::alerts::AlertCondition;
    let (state, mut rx) = alert_app_state(vec![AlertRule {
        id: "r1".into(),
        owner: "u1".into(),
        condition: AlertCondition::PriceAbove {
            symbol: "sz000001".into(),
            threshold: 10.0,
        },
        cooldown_ms: 60_000,
    }]);

    evaluate_alerts(&state, &sample_data(11.0, 100));
    let event = rx.try_recv().unwrap();
    assert_eq!(event.rule_id, "r1");
    assert_eq!(event.owner, "u1");
    assert_eq!(event.symbol, "sz000001");

    // timestamp 固定（同一拍）：重复越限被冷却抑制
    evaluate_alerts(&state, &sample_data(12.0, 100));
    assert!(rx.try_recv().is_err());

    // 滚动收盘窗口封顶（后续价格在阈值下，不产生新事件）
    for i in 0..(ALERT_CLOSES_CAP as u64 + 5) {
        let mut d = sample_data(9.0 + i as f64 * 0.001, 1);
        d.symbol = "sz000001".into();
        evaluate_alerts(&state, &d);
    }
    let windows = state.alert_closes.lock().unwrap();
    assert_eq!(windows["sz000001"].len(), ALERT_CLOSES_CAP);
}

/// 无规则（默认装配）：评估面零行为——窗口照常滚动但无事件产出
#[test]
fn test_alert_evaluation_no_rules_is_silent() {
    let (state, mut rx) = alert_app_state(vec![]);
    evaluate_alerts(&state, &sample_data(11.0, 100));
    assert!(rx.try_recv().is_err());
    assert_eq!(state.alert_closes.lock().unwrap()["sz000001"].len(), 1);
}

/// 首帧 Full（seq=1，含全字段），第二帧 Delta（seq=2，只含变化字段）
#[test]
fn test_versioned_frame_starts_full_then_delta() {
    let hub = Mutex::new(HashMap::new());

    let first = next_versioned_frame(&hub, channels::REAL_TIME_QUOTES, &sample_data(10.0, 100));
    match first {
        WsMessage::Sync(s) => {
            assert_eq!(s.seq, 1);
            assert_eq!(s.op, SyncOp::Full);
            assert_eq!(s.data["symbol"], "sz000001");
            assert_eq!(s.data["price"], 10.0);
            assert_eq!(s.data["volume"], 100);
        }
        other => panic!("首帧应为 Sync::Full，实际 {other:?}"),
    }

    // 价格变化：Delta 只携带变化字段；未变化字段（symbol/change 等）不重复传输
    let second = next_versioned_frame(&hub, channels::REAL_TIME_QUOTES, &sample_data(10.5, 100));
    match second {
        WsMessage::Sync(s) => {
            assert_eq!(s.seq, 2);
            assert_eq!(s.op, SyncOp::Delta);
            assert_eq!(s.data, serde_json::json!({"price": 10.5}));
        }
        other => panic!("第二帧应为 Sync::Delta，实际 {other:?}"),
    }

    // 多字段变化：Delta 含全部差异字段
    let third = next_versioned_frame(&hub, channels::REAL_TIME_QUOTES, &sample_data(11.0, 250));
    match third {
        WsMessage::Sync(s) => {
            assert_eq!(s.seq, 3);
            assert_eq!(s.op, SyncOp::Delta);
            assert_eq!(s.data, serde_json::json!({"price": 11.0, "volume": 250}));
        }
        other => panic!("第三帧应为 Sync::Delta，实际 {other:?}"),
    }
}

/// seq 逐通道独立递增（channel 维度隔离，互不串号）
#[test]
fn test_versioned_seq_increments_per_channel() {
    let hub = Mutex::new(HashMap::new());
    for _ in 0..3 {
        next_versioned_frame(&hub, channels::REAL_TIME_QUOTES, &sample_data(1.0, 1));
    }
    next_versioned_frame(&hub, channels::MARKET_DEPTH, &sample_data(2.0, 2));

    let hub = hub.lock().unwrap();
    assert_eq!(hub[channels::REAL_TIME_QUOTES].seq, 3);
    assert_eq!(hub[channels::MARKET_DEPTH].seq, 1);
}

/// Resync：未知通道/无快照返回 None；已广播过则回当前版本的 Full 快照
#[test]
fn test_resync_returns_current_full_snapshot() {
    let hub = Mutex::new(HashMap::new());
    // 未知通道
    assert!(resync_full_snapshot(&hub, "nope").is_none());

    // 广播两帧后 Resync：全量恢复 + 当前 seq
    next_versioned_frame(&hub, channels::REAL_TIME_QUOTES, &sample_data(10.0, 100));
    next_versioned_frame(&hub, channels::REAL_TIME_QUOTES, &sample_data(10.5, 100));
    let full = resync_full_snapshot(&hub, channels::REAL_TIME_QUOTES).unwrap();
    match full {
        WsMessage::Sync(s) => {
            assert_eq!(s.seq, 2);
            assert_eq!(s.op, SyncOp::Full);
            assert_eq!(s.data["price"], 10.5);
            // Full 快照必须含全字段（客户端替换本地快照的基线）
            assert_eq!(s.data["symbol"], "sz000001");
            assert_eq!(s.data["volume"], 100);
        }
        other => panic!("Resync 响应应为 Sync::Full，实际 {other:?}"),
    }
}

/// Resync 应答：未知通道/尚无快照必须显式回 Error 帧（客户端据此重订阅
/// 而非空等）；已广播通道照常回 Full 快照
#[test]
fn test_resync_reply_errors_on_unknown_channel() {
    let hub = Mutex::new(HashMap::new());
    match resync_reply(&hub, "nope") {
        WsMessage::Error(e) => {
            assert_eq!(e.code, RESYNC_UNKNOWN_CHANNEL_CODE);
            assert!(
                e.message.contains("nope"),
                "错误信息应包含通道名: {}",
                e.message
            );
        }
        other => panic!("未知通道 Resync 应回 Error 帧，实际 {other:?}"),
    }

    // 已广播通道：回当前版本 Full 快照
    next_versioned_frame(&hub, channels::REAL_TIME_QUOTES, &sample_data(9.0, 10));
    match resync_reply(&hub, channels::REAL_TIME_QUOTES) {
        WsMessage::Sync(s) => {
            assert_eq!(s.op, SyncOp::Full);
            assert_eq!(s.seq, 1);
        }
        other => panic!("已知通道 Resync 应回 Sync::Full，实际 {other:?}"),
    }
}

/// 服务端 Delta 与客户端 (alpha_core::sync) 合成为闭环：服务端发的差异
/// 在客户端引擎里应用后，快照收敛到服务端最新全量（端到端一致）
#[test]
fn test_server_delta_applies_on_client_engine() {
    use alpha_core::sync::SyncEngine;
    use alpha_protocols::websocket::SyncMessage;

    let hub = Mutex::new(HashMap::new());
    let mut engine = SyncEngine::new();

    // 服务端广播两帧（Full + Delta），模拟客户端逐帧应用
    for data in [sample_data(10.0, 100), sample_data(10.5, 150)] {
        match next_versioned_frame(&hub, channels::REAL_TIME_QUOTES, &data) {
            WsMessage::Sync(SyncMessage {
                channel,
                seq,
                op,
                data,
                ..
            }) => match op {
                SyncOp::Full => {
                    engine.apply_full(&channel, seq, data);
                }
                SyncOp::Delta => {
                    engine.apply_delta(&channel, seq, data).unwrap();
                }
            },
            other => panic!("应为 Sync 帧，实际 {other:?}"),
        }
    }

    // 客户端快照与服务端最新快照一致（字段逐项核对）
    let client_snapshot = engine.snapshot(channels::REAL_TIME_QUOTES).unwrap();
    let hub_guard = hub.lock().unwrap();
    let server_snapshot = hub_guard[channels::REAL_TIME_QUOTES]
        .last_snapshot
        .as_ref()
        .unwrap();
    assert_eq!(client_snapshot, server_snapshot);
    assert_eq!(client_snapshot["price"], 10.5);
    assert_eq!(client_snapshot["volume"], 150);
    assert_eq!(engine.last_seq(channels::REAL_TIME_QUOTES), Some(2));
}
