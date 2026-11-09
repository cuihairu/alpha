//! 同步协议（记录/请求/响应/冲突裁决）与客户端 SyncState 生命周期

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::validate::{validate_key, AccountError, MAX_PAYLOAD_BYTES, MAX_PUSHES_PER_REQUEST};

/// 同步记录（跨端共享数据的基本单元；`rev` 由服务端权威分配）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncRecord {
    /// `namespace:local_id`
    pub key: String,
    /// 服务端权威版本（单调递增，从 1 起；客户端只读回，不自增）
    pub rev: u64,
    /// 记录修改时间（毫秒 epoch，冲突裁决的 `NewestWins` 比较量）
    pub updated_at_ms: i64,
    /// 墓碑位：删除必须留痕，否则删除无法传播到其他端
    #[serde(default)]
    pub deleted: bool,
    /// 记录载荷（墓碑必须为 Null）
    pub payload: Value,
}

impl SyncRecord {
    /// 构造存活记录（`rev` 由调用方填服务端返回值）
    pub fn live(key: impl Into<String>, rev: u64, updated_at_ms: i64, payload: Value) -> Self {
        Self {
            key: key.into(),
            rev,
            updated_at_ms,
            deleted: false,
            payload,
        }
    }

    /// 构造墓碑（删除态；载荷恒为 Null）
    pub fn tombstone(key: impl Into<String>, rev: u64, updated_at_ms: i64) -> Self {
        Self {
            key: key.into(),
            rev,
            updated_at_ms,
            deleted: true,
            payload: Value::Null,
        }
    }

    /// 载荷字节长度（`Value` 的紧凑序列化长度）
    pub fn payload_bytes(&self) -> usize {
        self.payload.to_string().len()
    }

    /// 内容等价（忽略 rev）：载荷 + 墓碑位都一致即两端已收敛到同一份数据
    pub fn same_content(&self, other: &SyncRecord) -> bool {
        self.deleted == other.deleted && self.payload == other.payload
    }

    /// 全量校验（服务端入库前、客户端本地入队前同一套口径）
    pub fn validate(&self) -> Result<(), AccountError> {
        validate_key(&self.key)?;
        if self.rev == 0 {
            return Err(AccountError::InvalidRev);
        }
        if self.deleted {
            if !self.payload.is_null() {
                return Err(AccountError::TombstonePayloadNotNull);
            }
            return Ok(());
        }
        let bytes = self.payload_bytes();
        if bytes > MAX_PAYLOAD_BYTES {
            return Err(AccountError::PayloadTooLarge {
                actual: bytes,
                limit: MAX_PAYLOAD_BYTES,
            });
        }
        Ok(())
    }
}

/// 冲突裁决策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    /// 按记录时间戳取新；时间戳并列取服务端（两端同规则 → 必然收敛）
    NewestWins,
    /// 本地无条件胜（用户显式「以我的为准」）
    LocalWins,
    /// 服务端无条件胜（保守：以已共享数据为准）
    RemoteWins,
}

/// 冲突裁决胜方
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictWinner {
    Local,
    Remote,
}

/// 一次冲突裁决的留痕（客户端可展示「这条被覆盖了」）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncConflict {
    pub key: String,
    pub local_rev: u64,
    pub remote_rev: u64,
    pub winner: ConflictWinner,
}

/// 三方合并计划：本地与服务端各自要动的记录
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SyncPlan {
    /// 需上行服务端的本地记录
    pub push: Vec<SyncRecord>,
    /// 需从服务端拉取下发的记录（墓碑在此落地为本地删除）
    pub pull: Vec<SyncRecord>,
    /// 冲突留痕（含裁决胜方）
    pub conflicts: Vec<SyncConflict>,
}

impl SyncPlan {
    /// 计划为空 = 两端已收敛，无需任何传输
    pub fn is_empty(&self) -> bool {
        self.push.is_empty() && self.pull.is_empty()
    }
}

/// 三方合并：本地 vs 服务端，基线为上次同步后各键的 rev 快照。
///
/// 缺失即「无意见」——某端没有该键时其 rev 视作等于基线（**删除必须走
/// 墓碑**，缺键不等于本地删了它，否则会与服务端真实删除混淆）；仅当
/// 双方内容一致（忽略 rev）时按 rev 高者下发一份让基线推进，不记冲突。
pub fn plan_sync(
    local: &BTreeMap<String, SyncRecord>,
    remote: &BTreeMap<String, SyncRecord>,
    base: &BTreeMap<String, u64>,
    policy: ConflictPolicy,
) -> SyncPlan {
    let mut keys: BTreeSet<&str> = BTreeSet::new();
    keys.extend(local.keys().map(String::as_str));
    keys.extend(remote.keys().map(String::as_str));
    keys.extend(base.keys().map(String::as_str));

    let mut plan = SyncPlan::default();
    for key in keys {
        let base_rev = base.get(key).copied().unwrap_or(0);
        let local_rec = local.get(key);
        let remote_rec = remote.get(key);
        let local_changed = local_rec.is_some_and(|r| r.rev > base_rev);
        let remote_changed = remote_rec.is_some_and(|r| r.rev > base_rev);

        let (push, pull, conflict) = match (local_rec, remote_rec) {
            (Some(l), Some(r)) => {
                // 单侧改动（另一侧停在基线上）= 普通增量，不是冲突
                if local_changed && !remote_changed {
                    (Some(l.clone()), None, None)
                } else if !local_changed && remote_changed {
                    (None, Some(r.clone()), None)
                } else if l.same_content(r) {
                    // 内容已一致（并发同改同值）：下发 rev 高者推进基线
                    let newer = if l.rev >= r.rev { l } else { r };
                    (None, Some(newer.clone()), None)
                } else {
                    let local_wins = match policy {
                        ConflictPolicy::LocalWins => true,
                        ConflictPolicy::RemoteWins => false,
                        // 并列取服务端：两端同规则才能收敛到同一份数据
                        ConflictPolicy::NewestWins => l.updated_at_ms > r.updated_at_ms,
                    };
                    let conflict = SyncConflict {
                        key: key.to_string(),
                        local_rev: l.rev,
                        remote_rev: r.rev,
                        winner: if local_wins {
                            ConflictWinner::Local
                        } else {
                            ConflictWinner::Remote
                        },
                    };
                    if local_wins {
                        (Some(l.clone()), None, Some(conflict))
                    } else {
                        (None, Some(r.clone()), Some(conflict))
                    }
                }
            }
            // 仅本地有该键（服务端从未见过）：本地动过则新建上行，否则无事
            (Some(l), None) => {
                if local_changed {
                    (Some(l.clone()), None, None)
                } else {
                    (None, None, None)
                }
            }
            // 仅服务端有该键（本地基线之后才出现）：服务端动过则下发
            (None, Some(r)) => {
                if remote_changed {
                    (None, Some(r.clone()), None)
                } else {
                    (None, None, None)
                }
            }
            // 双端都无该键但基线有过 = 服务端删除后本地尚未拉取增量；
            // 无需传输，下一轮拉取按 cursor 自然补上墓碑
            (None, None) => (None, None, None),
        };

        if let Some(record) = push {
            plan.push.push(record);
        }
        if let Some(record) = pull {
            plan.pull.push(record);
        }
        if let Some(conflict) = conflict {
            plan.conflicts.push(conflict);
        }
    }
    plan
}

/// 上行推送项（客户端提交内容 + 自己的基线 rev，不自增 rev）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncPush {
    pub key: String,
    /// 客户端最后见到的服务端 rev（0 = 新建）；与服务端当前 rev 不符即冲突
    #[serde(default)]
    pub base_rev: u64,
    #[serde(default)]
    pub deleted: bool,
    pub payload: Value,
    /// 客户端修改时间（服务端 `NewestWins` 解冲突的参考量；0 = 未提供）
    #[serde(default)]
    pub updated_at_ms: i64,
}

impl SyncPush {
    /// 投影为待校验记录（`rev = base_rev + 1` 只是入队期的乐观占位，
    /// 服务端接受后以权威 rev 为准——故本投影仅供本地校验/合并比较）
    pub fn to_record(&self) -> SyncRecord {
        SyncRecord {
            key: self.key.clone(),
            rev: self.base_rev.saturating_add(1),
            updated_at_ms: self.updated_at_ms,
            deleted: self.deleted,
            payload: self.payload.clone(),
        }
    }

    /// 校验（键形状 / 墓碑空载荷 / 载荷上限；`base_rev` 无需校验）
    pub fn validate(&self) -> Result<(), AccountError> {
        SyncRecord {
            key: self.key.clone(),
            rev: self.base_rev.saturating_add(1),
            updated_at_ms: self.updated_at_ms,
            deleted: self.deleted,
            payload: self.payload.clone(),
        }
        .validate()
    }
}

/// 推送拒绝原因（服务端回传；`Invalid*` 为请求格式问题，`Conflict` 为
/// 乐观并发失配——后者带服务端权威副本由客户端裁决）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectionReason {
    Conflict,
    InvalidKey,
    PayloadTooLarge,
    TombstonePayloadNotNull,
}

/// 推送拒绝项
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncRejection {
    pub key: String,
    pub reason: RejectionReason,
    /// 服务端当前权威副本（`Conflict` 时在位，供客户端三方合并）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<SyncRecord>,
}

/// 同步上行请求（`cursor` 增量水位 + `base` 基线快照 + 待推送项）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncRequest {
    /// 客户端已消费到的服务端水位（拉取增量起点）
    #[serde(default)]
    pub cursor: u64,
    /// 上次同步后各键的服务端 rev 快照（缺失键 = 0）
    #[serde(default)]
    pub base: BTreeMap<String, u64>,
    #[serde(default)]
    pub pushes: Vec<SyncPush>,
}

/// 同步下行响应（接受项 / 拒绝项 / 增量变更 / 新水位）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncResponse {
    /// 服务端新水位（下次拉取起点；服务端权威单调）
    pub cursor: u64,
    /// 已接受的推送（带服务端权威 rev，客户端据此更新基线并出队）
    #[serde(default)]
    pub accepted: Vec<SyncRecord>,
    #[serde(default)]
    pub rejected: Vec<SyncRejection>,
    /// 水位之后的增量变更（含墓碑）
    #[serde(default)]
    pub changes: Vec<SyncRecord>,
}

/// 客户端同步状态（发件箱 + 水位 + 基线；纯归约器）
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SyncState {
    /// 已消费到的服务端水位
    pub cursor: u64,
    /// 各键最后确认的服务端 rev（提交推送时的 `base_rev` 来源）
    pub base: BTreeMap<String, u64>,
    /// 待推送发件箱（同键后写覆盖前写——本地连续编辑不该堆出多条）
    pub outbox: BTreeMap<String, SyncPush>,
}

/// 一次下行应用的结果（客户端展示/日志用）
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SyncOutcome {
    /// 落地的服务端记录（应用后的本地视图变更面）
    pub applied: Vec<SyncRecord>,
    /// 出队的本地推送键（服务端已接受）
    pub acked: Vec<String>,
    /// 需客户端裁决的冲突（服务端拒绝的推送）
    pub conflicts: Vec<SyncRejection>,
}

impl SyncState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 本地改动入队（`now_ms` 显式入参：归约器不读时钟）
    pub fn note_local_change(&mut self, key: &str, payload: Value, now_ms: i64) {
        self.outbox.insert(
            key.to_string(),
            SyncPush {
                key: key.to_string(),
                base_rev: self.base.get(key).copied().unwrap_or(0),
                deleted: false,
                payload,
                updated_at_ms: now_ms,
            },
        );
    }

    /// 本地删除入队（墓碑：删除必须传播，否则他端保留旧值）
    pub fn note_local_delete(&mut self, key: &str, now_ms: i64) {
        self.outbox.insert(
            key.to_string(),
            SyncPush {
                key: key.to_string(),
                base_rev: self.base.get(key).copied().unwrap_or(0),
                deleted: true,
                payload: Value::Null,
                updated_at_ms: now_ms,
            },
        );
    }

    /// 待推送项（按键序稳定输出，条数受 [`MAX_PUSHES_PER_REQUEST`] 限制，
    /// 超出部分留队下一轮——同步包大小对移动端弱网是硬约束）
    pub fn pending_pushes(&self) -> Vec<SyncPush> {
        self.outbox
            .values()
            .take(MAX_PUSHES_PER_REQUEST)
            .cloned()
            .collect()
    }

    /// 构造上行请求
    pub fn build_request(&self) -> SyncRequest {
        SyncRequest {
            cursor: self.cursor,
            base: self.base.clone(),
            pushes: self.pending_pushes(),
        }
    }

    /// 应用下行响应：接受项出队并推进基线、增量落基线（**墓碑同步推进
    /// 基线但不入本地视图**——本地视图的删除由调用方按 `deleted` 处理）、
    /// 拒绝项转为冲突交调用方裁决；水位单调前进（回退水位视为协议破坏，
    /// 直接忽略——防止服务端重启重发旧增量被误当新数据）
    pub fn apply_response(&mut self, response: &SyncResponse) -> SyncOutcome {
        let mut outcome = SyncOutcome::default();
        for record in &response.accepted {
            self.base.insert(record.key.clone(), record.rev);
            self.outbox.remove(&record.key);
            outcome.acked.push(record.key.clone());
            outcome.applied.push(record.clone());
        }
        for record in &response.changes {
            self.base.insert(record.key.clone(), record.rev);
            if response.cursor >= self.cursor {
                outcome.applied.push(record.clone());
            }
        }
        outcome.conflicts = response.rejected.clone();
        if response.cursor > self.cursor {
            self.cursor = response.cursor;
        }
        outcome
    }

    /// 本地视图应用（服务端记录落地：墓碑 → 从视图移除；其余 → 写入）
    pub fn apply_to_view(view: &mut BTreeMap<String, SyncRecord>, applied: &[SyncRecord]) {
        for record in applied {
            if record.deleted {
                view.remove(&record.key);
            } else {
                view.insert(record.key.clone(), record.clone());
            }
        }
    }
}
