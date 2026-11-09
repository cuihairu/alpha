use std::{collections::HashMap, sync::Mutex};

use alpha_core::sync::build_delta;
use alpha_protocols::websocket::{SyncOp, WsMessage};

use crate::RealTimeData;

/// 服务端单通道同步状态（版本控制的服务端半边，见 [`next_versioned_frame`]）
#[derive(Debug, Clone)]
pub(crate) struct ChannelSyncState {
    /// 通道内已发布的帧数（= 最新版本号），全连接共享同一序列
    pub(crate) seq: u64,
    /// 最近一次广播的全量快照：非空时下一帧发 Delta（相对此快照的字段差）
    pub(crate) last_snapshot: Option<serde_json::Value>,
}

impl ChannelSyncState {
    fn new() -> Self {
        Self {
            seq: 0,
            last_snapshot: None,
        }
    }
}

/// 构造版本化同步帧：通道 seq 递增；首帧 `Full` 全量，后续 `Delta` 差异（只含变化字段）。
/// seq 与快照在锁内更新——全连接共享同一序列，客户端据此做丢帧检测/Resync。
pub(crate) fn next_versioned_frame(
    hub: &Mutex<HashMap<String, ChannelSyncState>>,
    channel: &str,
    data: &RealTimeData,
) -> WsMessage {
    let value = serde_json::to_value(data).unwrap_or_default();
    let mut hub = hub.lock().unwrap();
    let state = hub
        .entry(channel.to_string())
        .or_insert_with(ChannelSyncState::new);
    state.seq += 1;
    let payload = match &state.last_snapshot {
        Some(prev) => build_delta(prev, &value),
        None => value.clone(),
    };
    let op = if state.last_snapshot.is_some() {
        SyncOp::Delta
    } else {
        SyncOp::Full
    };
    state.last_snapshot = Some(value);
    WsMessage::sync(channel.to_string(), state.seq, op, payload)
}

/// Resync 响应：返回通道当前版本的 `Full` 快照帧（无快照/未知通道返回 None）
pub(crate) fn resync_full_snapshot(
    hub: &Mutex<HashMap<String, ChannelSyncState>>,
    channel: &str,
) -> Option<WsMessage> {
    let hub = hub.lock().unwrap();
    let state = hub.get(channel)?;
    let snapshot = state.last_snapshot.clone()?;
    Some(WsMessage::sync(
        channel.to_string(),
        state.seq,
        SyncOp::Full,
        snapshot,
    ))
}

/// Resync 打到未知通道/尚无快照时的错误码（HTTP 语义沿用：not found）
pub(crate) const RESYNC_UNKNOWN_CHANNEL_CODE: i32 = 404;

/// Resync 应答：已知通道回当前版本 `Full` 快照；未知通道/尚无快照回 `Error` 帧——
/// 客户端需要显式失败信号才能重订阅，静默丢弃只会让它空等重传。
pub(crate) fn resync_reply(
    hub: &Mutex<HashMap<String, ChannelSyncState>>,
    channel: &str,
) -> WsMessage {
    resync_full_snapshot(hub, channel).unwrap_or_else(|| {
        WsMessage::error(
            RESYNC_UNKNOWN_CHANNEL_CODE,
            format!("resync 失败：通道 {channel} 无快照或不存在"),
            None,
        )
    })
}
