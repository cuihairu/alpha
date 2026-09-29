//! 实时数据同步状态机（L0 纯计算层；WebSocket 增量更新 + 版本控制的客户端核心）
//!
//! 线上帧类型见 `alpha_protocols::websocket::{SyncMessage, SyncOp, ResyncRequest}`。
//! 本模块刻意只依赖 `serde_json`（无网络/wasm 依赖）：状态推进、丢帧检测、
//! 增量生成与合并全是纯函数，native 单测锁定语义；wasm 侧仅做薄绑定
//! （wasm-analyzer 的 `WebSocketClient` 只负责传输，状态机在本模块）。
//!
//! ## 版本与增量契约（最小可用口径，TODO「实现实时数据同步协议」）
//!
//! * **版本**：每通道单调递增 `seq`。客户端记录 `last_seq`，收到帧时校验
//!   `seq == last_seq + 1`，断裂即 [`SyncOutcome::Gap`]——调用方据此发
//!   [`ResyncRequest`]，服务端回 `Full` 快照补齐。
//! * **增量**：`Delta` 帧只携带与上一帧的差异（[`build_delta`] 的深度 1 字段差），
//!   客户端用 [`apply_delta`] 合入本地快照，与全量帧收敛到相同状态。
//! * **全量**：`Full` 帧直接替换本地快照，兼作连接建立后的基线。
//!
//! 扩展接口缝（留待后续立项）：嵌套（深度 >1）差异、数组 diff、撤销/乱序重放
//! 缓冲。增量生成在服务端广播路径复用同一套 [`build_delta`]（real-time-feed），
//! 两端同一实现，避免协议漂移。

use std::collections::HashMap;

use serde_json::Value;

/// 单通道同步状态
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChannelSyncState {
    /// 最近应用的 seq；`None` = 尚未收到该通道任何帧
    pub last_seq: Option<u64>,
    /// 本地快照（Full 全量 / Delta 合并后的结果）
    pub snapshot: Option<Value>,
}

/// 多通道同步引擎：逐帧推进并做一致性校验（丢帧检测/幂等忽略）
#[derive(Debug, Clone, Default)]
pub struct SyncEngine {
    channels: HashMap<String, ChannelSyncState>,
}

/// 帧应用结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncOutcome {
    /// 正常应用（seq 已推进）
    Advanced { seq: u64 },
    /// 重放/乱序旧帧（`seq <= last_seq`），幂等忽略
    Idempotent { seq: u64 },
    /// 丢帧：期望 `expected`，实际收到 `got`，需要 Resync
    Gap { expected: u64, got: u64 },
}

/// 无法应用的协议违约（区别于丢帧：帧本身不合法）
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SyncError {
    #[error("Delta 帧 {seq} 到达时通道 {channel} 尚无本地快照：应先 Full 或 Resync")]
    DeltaWithoutSnapshot { channel: String, seq: u64 },
}

impl SyncEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// 查询单通道状态（不存在返回 None）
    pub fn channel(&self, channel: &str) -> Option<&ChannelSyncState> {
        self.channels.get(channel)
    }

    /// 已跟踪通道数
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// 通道最近应用版本
    pub fn last_seq(&self, channel: &str) -> Option<u64> {
        self.channels.get(channel).and_then(|s| s.last_seq)
    }

    /// 通道本地快照
    pub fn snapshot(&self, channel: &str) -> Option<&Value> {
        self.channels.get(channel).and_then(|s| s.snapshot.as_ref())
    }

    /// 应用全量帧：替换本地快照并推进版本
    ///
    /// Full 帧携带**权威快照**，除旧帧（`seq <= last_seq`）幂等忽略外一律接受：
    /// Resync 补齐时服务端回的是最新 `seq` 的 Full（可能大于 `last_seq + 1`），
    /// 若照搬 Delta 的连续性校验会永远 Gap（基线无法恢复）。连续性校验只对
    /// Delta 生效——它是依赖前置状态的增量。
    pub fn apply_full(&mut self, channel: &str, seq: u64, data: Value) -> SyncOutcome {
        let state = self.channels.entry(channel.to_string()).or_default();
        match state.last_seq {
            None => {
                // 首帧（连接基线）：无论 seq 起点，直接建立快照
                state.last_seq = Some(seq);
                state.snapshot = Some(data);
                SyncOutcome::Advanced { seq }
            }
            Some(last) if seq <= last => SyncOutcome::Idempotent { seq },
            Some(_) => {
                state.last_seq = Some(seq);
                state.snapshot = Some(data);
                SyncOutcome::Advanced { seq }
            }
        }
    }

    /// 应用增量帧：合并进本地快照并推进版本（无快照即协议违约）
    pub fn apply_delta(
        &mut self,
        channel: &str,
        seq: u64,
        data: Value,
    ) -> Result<SyncOutcome, SyncError> {
        let state = self.channels.entry(channel.to_string()).or_default();
        let Some(last) = state.last_seq else {
            return Err(SyncError::DeltaWithoutSnapshot {
                channel: channel.to_string(),
                seq,
            });
        };
        if seq <= last {
            return Ok(SyncOutcome::Idempotent { seq });
        }
        if seq != last + 1 {
            return Ok(SyncOutcome::Gap {
                expected: last + 1,
                got: seq,
            });
        }
        let Some(base) = state.snapshot.take() else {
            return Err(SyncError::DeltaWithoutSnapshot {
                channel: channel.to_string(),
                seq,
            });
        };
        let merged = apply_delta(&base, &data);
        state.snapshot = Some(merged);
        state.last_seq = Some(seq);
        Ok(SyncOutcome::Advanced { seq })
    }

    /// 重同步辅助：客户端在收到 [`SyncOutcome::Gap`] 后调用，返回应从哪个版本
    /// 发起 Resync（即已连续应用的 last_seq，服务端以此为基准回 Full 快照）。
    pub fn resync_from(&self, channel: &str) -> u64 {
        self.last_seq(channel).unwrap_or(0)
    }

    /// 连接重建/主动清理时重置某通道状态（本地快照与版本一并丢弃）
    pub fn reset_channel(&mut self, channel: &str) {
        self.channels.remove(channel);
    }
}

/// 深度 1 字段差集：`prev` 与 `next` 同为 object 时，仅保留 `next` 中
/// **新增或值变化**的字段（值整体替换，不做嵌套递归）。非 object 输入
/// 直接返回 `next` 自身——增量的最小可用口径，不含数组/嵌套 diff。
pub fn build_delta(prev: &Value, next: &Value) -> Value {
    match (prev, next) {
        (Value::Object(prev_map), Value::Object(next_map)) => {
            let mut delta = serde_json::Map::new();
            for (key, value) in next_map {
                match prev_map.get(key) {
                    Some(prev_value) if prev_value == value => {}
                    _ => {
                        delta.insert(key.clone(), value.clone());
                    }
                }
            }
            Value::Object(delta)
        }
        _ => next.clone(),
    }
}

/// 增量合并：`base`（本地快照）与 `delta`（增量帧）的深度 1 合并。
/// 同为 object 时逐字段覆盖（delta 胜出），否则直接取 `delta`。
pub fn apply_delta(base: &Value, delta: &Value) -> Value {
    match (base, delta) {
        (Value::Object(base_map), Value::Object(delta_map)) => {
            let mut merged = base_map.clone();
            for (key, value) in delta_map {
                merged.insert(key.clone(), value.clone());
            }
            Value::Object(merged)
        }
        _ => delta.clone(),
    }
}

/// 版本表帧的操作类型（服务端发布侧记录；线上形态见
/// `alpha_protocols::websocket::SyncOp`，一一对应）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryOp {
    Full,
    Delta,
}

/// 版本表中的一帧（seq + 操作 + 该帧载荷）
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryFrame {
    pub seq: u64,
    pub op: HistoryOp,
    pub data: Value,
}

/// 客户端追平方案
#[derive(Debug, Clone, PartialEq)]
pub enum CatchUp {
    /// 可增量拉齐：按序重放这些帧即从 `last_seq` 推进到最新（可能为空 = 已追平）
    Frames(Vec<HistoryFrame>),
    /// 无法增量拉齐（客户端落后超出保留窗口/通道无历史）：需全量重同步，
    /// `latest_seq` 为服务端当前最新版本（通道未知时 `None`）
    Resync { latest_seq: Option<u64> },
}

/// 内存态版本表（服务端发布侧）：逐通道环形保留最近 [`SyncHistory::retention`]
/// 帧，支撑客户端「从上次版本拉齐到最新」——保留窗口内走增量重放
/// （[`SyncHistory::catch_up`] 返回 [`CatchUp::Frames`]），落后超出窗口回落
/// 全量重同步（[`CatchUp::Resync`]）。
///
/// 契约：[`SyncHistory::record`] 按 seq 单调递增调用（服务端发布路径在锁内
/// 保证，见 real-time-feed `next_versioned_frame`）；本结构不做乱序防御——
/// 追平的连续性校验按「保留帧是否覆盖 `last_seq+1..=最新`」判定。
pub struct SyncHistory {
    retention: usize,
    channels: HashMap<String, Vec<HistoryFrame>>,
}

impl SyncHistory {
    /// `retention` 为每通道保留的最大帧数（须 > 0）
    pub fn new(retention: usize) -> Self {
        assert!(retention > 0, "SyncHistory 保留帧数须大于 0");
        Self {
            retention,
            channels: HashMap::new(),
        }
    }

    /// 记录一帧发布；超保留窗口时淘汰最旧帧
    pub fn record(&mut self, channel: &str, seq: u64, op: HistoryOp, data: Value) {
        let history = self.channels.entry(channel.to_string()).or_default();
        history.push(HistoryFrame { seq, op, data });
        if history.len() > self.retention {
            let excess = history.len() - self.retention;
            history.drain(..excess);
        }
    }

    /// 通道当前最新版本（无历史返回 None）
    pub fn latest_seq(&self, channel: &str) -> Option<u64> {
        self.channels
            .get(channel)
            .and_then(|h| h.last().map(|f| f.seq))
    }

    /// 客户端追平：从 `last_seq`（客户端已连续应用到的版本）拉齐到最新。
    /// 保留帧完整覆盖 `last_seq+1..=latest` 时返回增量重放序列，否则要求全量重同步。
    pub fn catch_up(&self, channel: &str, last_seq: u64) -> CatchUp {
        let Some(history) = self.channels.get(channel) else {
            return CatchUp::Resync { latest_seq: None };
        };
        let Some(latest) = history.last().map(|f| f.seq) else {
            return CatchUp::Resync { latest_seq: None };
        };
        if latest <= last_seq {
            return CatchUp::Frames(Vec::new());
        }
        // 连续性：保留的第一帧必须不晚于 last_seq+1，且帧间无空洞
        let contiguous = history.first().is_some_and(|f| f.seq <= last_seq + 1)
            && history.windows(2).all(|w| w[1].seq == w[0].seq + 1);
        if !contiguous {
            return CatchUp::Resync {
                latest_seq: Some(latest),
            };
        }
        CatchUp::Frames(
            history
                .iter()
                .filter(|f| f.seq > last_seq)
                .cloned()
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 首帧 Full 建立基线；随后 Delta 逐帧推进，快照收敛到与全量相同状态
    #[test]
    fn full_then_delta_advances_snapshot() {
        let mut engine = SyncEngine::new();
        assert_eq!(
            engine.apply_full("real_time_quotes", 1, json!({"symbol": "a", "price": 10.0})),
            SyncOutcome::Advanced { seq: 1 }
        );
        assert_eq!(engine.last_seq("real_time_quotes"), Some(1));

        assert_eq!(
            engine
                .apply_delta("real_time_quotes", 2, json!({"price": 10.5}))
                .unwrap(),
            SyncOutcome::Advanced { seq: 2 }
        );
        assert_eq!(engine.snapshot("real_time_quotes").unwrap()["price"], 10.5);
        // 未变化字段保留
        assert_eq!(engine.snapshot("real_time_quotes").unwrap()["symbol"], "a");
        assert_eq!(engine.last_seq("real_time_quotes"), Some(2));
    }

    /// Delta 无快照：协议违约（客户端应先收 Full），报错而非静默
    #[test]
    fn delta_without_snapshot_is_error() {
        let mut engine = SyncEngine::new();
        let err = engine
            .apply_delta("real_time_quotes", 1, json!({"price": 10.0}))
            .unwrap_err();
        assert!(matches!(
            err,
            SyncError::DeltaWithoutSnapshot { channel, seq: 1 } if channel == "real_time_quotes"
        ));
    }

    /// 丢帧检测：seq 跳跃（2 → 4）返回 Gap，并给出期望版本供 Resync
    #[test]
    fn gap_detected_on_missing_seq() {
        let mut engine = SyncEngine::new();
        engine.apply_full("rtq", 2, json!({"price": 10.0}));
        let outcome = engine
            .apply_delta("rtq", 4, json!({"price": 11.0}))
            .unwrap();
        assert_eq!(
            outcome,
            SyncOutcome::Gap {
                expected: 3,
                got: 4
            }
        );
        // Gap 不推进状态：快照与版本保持原样
        assert_eq!(engine.last_seq("rtq"), Some(2));
        assert_eq!(engine.snapshot("rtq").unwrap()["price"], 10.0);
        // resync_from 建议从已连续应用的版本（2）发起
        assert_eq!(engine.resync_from("rtq"), 2);
    }

    /// 重放/乱序旧帧：幂等忽略，不破坏版本与快照
    #[test]
    fn stale_frame_is_idempotent() {
        let mut engine = SyncEngine::new();
        engine.apply_full("rtq", 5, json!({"price": 10.0}));
        engine
            .apply_delta("rtq", 6, json!({"price": 10.5}))
            .unwrap();
        // 旧帧重放（seq=5 与 seq=6 均已应用）
        assert_eq!(
            engine.apply_full("rtq", 6, json!({"price": 99.0})),
            SyncOutcome::Idempotent { seq: 6 }
        );
        assert_eq!(
            engine.apply_delta("rtq", 5, json!({"price": 1.0})).unwrap(),
            SyncOutcome::Idempotent { seq: 5 }
        );
        // 状态未被旧帧污染
        assert_eq!(engine.last_seq("rtq"), Some(6));
        assert_eq!(engine.snapshot("rtq").unwrap()["price"], 10.5);
    }

    /// Gap 后 Full 补齐（Resync 路径）：快照恢复到与最新版本一致
    #[test]
    fn full_after_gap_restores_snapshot() {
        let mut engine = SyncEngine::new();
        engine.apply_full("rtq", 1, json!({"price": 10.0}));
        // 丢 seq=2，收到 3 → Gap
        assert_eq!(
            engine
                .apply_delta("rtq", 3, json!({"price": 12.0}))
                .unwrap(),
            SyncOutcome::Gap {
                expected: 2,
                got: 3
            }
        );
        // 客户端发起 Resync，服务端回 Full（携带最新 seq=3）
        assert_eq!(
            engine.apply_full("rtq", 3, json!({"price": 12.0, "volume": 100})),
            SyncOutcome::Advanced { seq: 3 }
        );
        assert_eq!(engine.last_seq("rtq"), Some(3));
        assert_eq!(engine.snapshot("rtq").unwrap()["volume"], 100);
        // 补齐后增量链路恢复
        engine
            .apply_delta("rtq", 4, json!({"price": 12.5}))
            .unwrap();
        assert_eq!(engine.last_seq("rtq"), Some(4));
    }

    /// 多通道隔离：版本各自独立推进，互不干扰
    #[test]
    fn multiple_channels_isolated() {
        let mut engine = SyncEngine::new();
        engine.apply_full("quotes", 1, json!({"price": 1.0}));
        engine.apply_full("depth", 7, json!({"bids": []}));
        engine
            .apply_delta("quotes", 2, json!({"price": 2.0}))
            .unwrap();
        engine.apply_delta("depth", 8, json!({"ask": 3.0})).unwrap();

        assert_eq!(engine.last_seq("quotes"), Some(2));
        assert_eq!(engine.last_seq("depth"), Some(8));
        assert_eq!(engine.resync_from("depth"), 8);
        // 未跟踪通道返回 None/0
        assert_eq!(engine.last_seq("nope"), None);
        assert_eq!(engine.resync_from("nope"), 0);
        assert_eq!(engine.channel_count(), 2);
    }

    /// build_delta：仅保留新增/变化字段，同值字段省略
    #[test]
    fn build_delta_keeps_only_changed_fields() {
        let prev = json!({"symbol": "a", "price": 10.0, "volume": 100});
        let next = json!({"symbol": "a", "price": 10.5, "volume": 100});
        let delta = build_delta(&prev, &next);
        assert_eq!(delta, json!({"price": 10.5}));
    }

    /// build_delta：新增字段进入 delta
    #[test]
    fn build_delta_includes_new_fields() {
        let prev = json!({"symbol": "a", "price": 10.0});
        let next = json!({"symbol": "a", "price": 10.0, "bid": 9.9});
        assert_eq!(build_delta(&prev, &next), json!({"bid": 9.9}));
    }

    /// apply_delta 与 build_delta 互为逆操作：base ⊕ delta(prev→next) == next
    #[test]
    fn build_then_apply_reconstructs_next() {
        let base = json!({"symbol": "a", "price": 10.0, "volume": 100, "tag": {"nested": true}});
        let next = json!({"symbol": "a", "price": 11.0, "volume": 100, "tag": {"nested": false}});
        let delta = build_delta(&base, &next);
        let merged = apply_delta(&base, &delta);
        assert_eq!(merged, next);
    }

    /// 非 object 输入：build_delta 整体替换、apply_delta 整体取 delta（文档化契约）
    #[test]
    fn non_object_inputs_replace_wholesale() {
        assert_eq!(build_delta(&json!([1, 2]), &json!([3])), json!([3]));
        assert_eq!(apply_delta(&json!(1.0), &json!(2.0)), json!(2.0));
    }

    /// reset_channel 丢弃本地状态；重连后首帧 Full 重新建立基线
    #[test]
    fn reset_channel_clears_state() {
        let mut engine = SyncEngine::new();
        engine.apply_full("rtq", 3, json!({"price": 10.0}));
        engine.reset_channel("rtq");
        assert_eq!(engine.last_seq("rtq"), None);
        assert_eq!(engine.channel_count(), 0);
        // 重置后任意 seq 起点的 Full 均可重新建立基线
        assert_eq!(
            engine.apply_full("rtq", 100, json!({"price": 99.0})),
            SyncOutcome::Advanced { seq: 100 }
        );
    }

    /// 追平：保留窗口内返回连续增量重放（从 last_seq+1 到最新）；已追平返回空
    #[test]
    fn catch_up_replays_missed_frames_contiguously() {
        let mut history = SyncHistory::new(8);
        history.record("rtq", 1, HistoryOp::Full, json!({"price": 10.0}));
        history.record("rtq", 2, HistoryOp::Delta, json!({"price": 10.5}));
        history.record("rtq", 3, HistoryOp::Delta, json!({"volume": 100}));
        history.record("rtq", 4, HistoryOp::Delta, json!({"bid": 10.4}));

        let CatchUp::Frames(frames) = history.catch_up("rtq", 1) else {
            panic!("窗口内应可增量追平");
        };
        assert_eq!(
            frames.iter().map(|f| f.seq).collect::<Vec<_>>(),
            vec![2, 3, 4],
            "重放序列从 last_seq+1 连续到最新"
        );
        assert_eq!(frames[0].op, HistoryOp::Delta);
        assert_eq!(frames[0].data, json!({"price": 10.5}));

        // 已追平（last_seq == latest）：空重放
        assert_eq!(history.catch_up("rtq", 4), CatchUp::Frames(Vec::new()));
        // 落后 0 帧以上但未越窗：只返回缺失段
        let CatchUp::Frames(tail) = history.catch_up("rtq", 3) else {
            panic!("尾部追平应可行");
        };
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].seq, 4);
    }

    /// 追平：落后超出保留窗口 → 全量重同步（携带服务端最新版本）；
    /// 恰好落在窗口边缘仍可增量
    #[test]
    fn catch_up_requires_resync_when_retention_exceeded() {
        let mut history = SyncHistory::new(2);
        for seq in 1..=4 {
            history.record("rtq", seq, HistoryOp::Delta, json!({"seq": seq}));
        }
        // 只保留 3、4：last_seq=1 需要从 2 拉齐，但 2 已被淘汰
        assert_eq!(
            history.catch_up("rtq", 1),
            CatchUp::Resync {
                latest_seq: Some(4)
            }
        );
        // last_seq=2 恰好接上窗口首帧：可增量
        let CatchUp::Frames(frames) = history.catch_up("rtq", 2) else {
            panic!("窗口边缘应可增量追平");
        };
        assert_eq!(frames.iter().map(|f| f.seq).collect::<Vec<_>>(), vec![3, 4]);
    }

    /// 追平：未知通道 → Resync 且无最新版本号（服务端应以建立基线的 Full 回复）
    #[test]
    fn catch_up_unknown_channel_resyncs_without_latest() {
        let history = SyncHistory::new(4);
        assert_eq!(
            history.catch_up("ghost", 0),
            CatchUp::Resync { latest_seq: None }
        );
        assert_eq!(SyncHistory::new(4).latest_seq("ghost"), None);
    }

    /// 回环集成（发布侧版本表 × 客户端状态机）：服务端以 build_delta 生成增量
    /// 并记录版本表；「全程在线」客户端逐帧应用；「中途掉线」客户端经
    /// catch_up 重放拉齐——两端快照收敛一致；超出保留窗口走 Resync 全量，
    /// 亦收敛。覆盖 TODO 同步协议的完整闭环。
    #[test]
    fn publish_catch_up_round_trip_converges() {
        // 服务端：状态推进 → Full@1，此后逐帧 Delta，记入版本表（保留 3 帧）
        let states = [
            json!({"symbol": "sz000001", "price": 10.0, "volume": 100}),
            json!({"symbol": "sz000001", "price": 10.5, "volume": 100}),
            json!({"symbol": "sz000001", "price": 10.5, "volume": 250}),
            json!({"symbol": "sz000001", "price": 11.0, "volume": 250, "bid": 10.9}),
            json!({"symbol": "sz000001", "price": 12.0, "volume": 250, "bid": 11.9}),
        ];
        let mut history = SyncHistory::new(3);
        let mut prev: Option<Value> = None;
        let mut publish = |state: &Value, seq: u64| {
            let (op, payload) = match &prev {
                Some(p) => (HistoryOp::Delta, build_delta(p, state)),
                None => (HistoryOp::Full, state.clone()),
            };
            history.record("rtq", seq, op, payload);
            prev = Some(state.clone());
        };
        for (i, state) in states.iter().enumerate() {
            publish(state, (i + 1) as u64);
        }
        assert_eq!(history.latest_seq("rtq"), Some(5));

        // 全程在线客户端：逐帧应用（Full 建基线，Delta 增量推进）
        let mut live = SyncEngine::new();
        live.apply_full("rtq", 1, states[0].clone());
        for (i, state) in states.iter().enumerate().skip(1) {
            let delta = build_delta(&states[i - 1], state);
            live.apply_delta("rtq", (i + 1) as u64, delta).unwrap();
        }
        assert_eq!(live.snapshot("rtq").unwrap(), &states[4]);

        // 掉线客户端：看到前两帧（Full@1 + Delta@2）后掉线，重连时窗口首帧=3
        // （retention=3，seq 1..2 已淘汰）——恰好接上窗口边缘 → 增量重放 3..=5
        let mut roaming = SyncEngine::new();
        roaming.apply_full("rtq", 1, states[0].clone());
        roaming
            .apply_delta("rtq", 2, build_delta(&states[0], &states[1]))
            .unwrap();

        let CatchUp::Frames(frames) = history.catch_up("rtq", 2) else {
            panic!("窗口边缘（last_seq=2，窗口首帧=3）应可增量追平");
        };
        assert_eq!(
            frames.iter().map(|f| f.seq).collect::<Vec<_>>(),
            vec![3, 4, 5],
            "重放序列从 last_seq+1 连续到最新"
        );
        for frame in &frames {
            match frame.op {
                HistoryOp::Full => {
                    roaming.apply_full("rtq", frame.seq, frame.data.clone());
                }
                HistoryOp::Delta => {
                    roaming
                        .apply_delta("rtq", frame.seq, frame.data.clone())
                        .unwrap();
                }
            }
        }
        assert_eq!(
            roaming.snapshot("rtq").unwrap(),
            live.snapshot("rtq").unwrap(),
            "增量重放路径收敛到与在线客户端一致"
        );

        // 更早掉线的客户端（停在 seq=1，窗口首帧=3 接不上）→ 服务端回 Full 重同步
        let mut stale = SyncEngine::new();
        stale.apply_full("rtq", 1, states[0].clone());
        let CatchUp::Resync {
            latest_seq: Some(latest),
        } = history.catch_up("rtq", 1)
        else {
            panic!("超出保留窗口必须要求全量重同步");
        };
        assert_eq!(latest, 5);
        stale.apply_full("rtq", latest, states[4].clone());
        assert_eq!(
            stale.snapshot("rtq").unwrap(),
            live.snapshot("rtq").unwrap(),
            "Resync 全量后与在线客户端收敛"
        );

        // 补齐后增量链路恢复：下一帧 Delta 两条客户端路径均正常推进
        let mut next_state = states[4].clone();
        next_state["price"] = json!(12.5);
        let delta = build_delta(&states[4], &next_state);
        history.record("rtq", 6, HistoryOp::Delta, delta.clone());
        live.apply_delta("rtq", 6, delta.clone()).unwrap();
        roaming.apply_delta("rtq", 6, delta.clone()).unwrap();
        stale.apply_delta("rtq", 6, delta).unwrap();
        assert_eq!(
            roaming.snapshot("rtq").unwrap(),
            live.snapshot("rtq").unwrap()
        );
        assert_eq!(
            stale.snapshot("rtq").unwrap(),
            live.snapshot("rtq").unwrap()
        );
    }

    /// 追平契约护栏：retention=0 构造期拒绝
    #[test]
    fn sync_history_rejects_zero_retention() {
        assert!(std::panic::catch_unwind(|| SyncHistory::new(0)).is_err());
    }
}
