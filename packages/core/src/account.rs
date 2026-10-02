//! 统一账户与跨端数据同步的协议/领域面（纯函数，TODO L476）
//!
//! 本模块是「账户 + 数据同步」的**语义唯一事实源**：服务端（网关账户面）、
//! 各端客户端（web/desktop/移动）都按这里的类型与判定实现，wire 形状
//! （`SyncRequest`/`SyncResponse`/`SyncRecord`）可直接过网线。
//!
//! 协议模型（三件套）：
//! - **账户档案** [`AccountProfile`]：`sub` 归一后的展示身份 + 语言偏好，
//!   带单调 `rev`（服务端权威版本，客户端据此做乐观并发）；
//! - **同步记录** [`SyncRecord`]：跨端共享数据的基本单元（工作区/偏好/
//!   告警规则…），**删除即墓碑**（`deleted = true`，载荷必须为 Null）——
//!   不带墓碑的话删除无法传播，他端会把旧值当新值保留；
//! - **同步往返** [`SyncRequest`] → [`SyncResponse`]：客户端带 `cursor`
//!   （增量水位）与 `base`（上次同步后各键的服务端 rev 快照）上行，服务端
//!   只接受 `base_rev` 与当前权威 rev 相等的推送（乐观并发），不符即回
//!   [`SyncRejection`]（带服务端权威副本供客户端解冲突），并回传
//!   `cursor` 之后的变更。
//!
//! 客户端侧三方合并 [`plan_sync`]：本地与服务端都相对共同基线改动过、
//! 且内容不同 = 冲突，按 [`ConflictPolicy`] 裁决（`NewestWins` 用记录
//! 时间戳、并列取服务端——**两端算出的裁决必然相同**，这是收敛的前提；
//! `LocalWins`/`RemoteWins` 留给单端显式偏好）。
//!
//! 设计约定：
//! - 无时钟读取、无随机性：时间戳与账户 id 全显式入参，同输入必同输出；
//! - 服务端 rev 权威：客户端只提交 `base_rev` 与内容，不自增 rev（乐观
//!   并发而非离线乱序分配，避免两台设备写出同一个 rev）；
//! - 键名空间隔离：`namespace:local_id`（如 `workspace:9f2c`），命名空间
//!   形状校验 + 载荷 64 KiB 上限，防单键撑爆同步包；
//! - 账户 id → 存储键的 slug 化在 [`normalize_account_id`]（`sub` 可含
//!   任意 IdP 字符，slug 不做就穿进存储键；不安全字符折叠后追加原串
//!   摘要尾巴，两个不同 `sub` 折叠成同一 slug 时仍不撞键）。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// 单条同步载荷上限（字节；超出即拒——同步包要能在移动端弱网下往返）
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024;

/// 同步键总长上限（含命名空间前缀）
pub const MAX_KEY_LEN: usize = 64;

/// 命名空间上限（`namespace:` 部分）
pub const MAX_NAMESPACE_LEN: usize = 16;

/// 键内本地 id 上限
pub const MAX_LOCAL_ID_LEN: usize = 48;

/// 单次请求建议携带的推送条数上限（超出由客户端分片）
pub const MAX_PUSHES_PER_REQUEST: usize = 100;

/// 未认证时的本机账户 id（网关 `--auth-mode off` 形态：单账户本机语义）
pub const LOCAL_ACCOUNT_ID: &str = "local";

/// 账户/同步面校验错误（服务端 400、客户端本地校验共用同一组语义）
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AccountError {
    #[error("account id 非法（空或超长）")]
    InvalidAccountId,
    #[error("显示名不能为空")]
    EmptyDisplayName,
    #[error("显示名超长（上限 64 字符）")]
    DisplayNameTooLong,
    #[error("邮箱格式非法")]
    InvalidEmail,
    #[error("locale 标签非法（期望 BCP 47 形状，如 zh-CN / en）")]
    InvalidLocale,
    #[error("同步键非法：{0}")]
    InvalidKey(String),
    #[error("载荷超长（{actual} > {limit} 字节）")]
    PayloadTooLarge { actual: usize, limit: usize },
    #[error("墓碑记录必须携带空载荷（deleted=true 时 payload 须为 null）")]
    TombstonePayloadNotNull,
    #[error("rev 非法（服务端 rev 从 1 起，0 只作 base 缺省）")]
    InvalidRev,
}

fn is_lower_alnum(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit()
}

/// FNV-1a 64 位摘要（slug 折叠后追加尾巴用；非密码学用途，仅防键碰撞）
fn fnv1a64(data: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in data.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// 账户 id（JWT `sub` / 本机缺省）→ 存储键安全 slug。
///
/// 规则：原串 trim 后小写；仅保留 `[a-z0-9._-]`，其余折叠为 `-`（连续
/// 折叠、首尾去横杠）；空 → [`LOCAL_ACCOUNT_ID`]；**折叠过的 slug 追加
/// 原串 8 位摘要尾巴**（`a b` 与 `a-b` 同折叠但键不同——账户串不可当
/// 目录名用，撞键等于两个账户互相看见数据）。
pub fn normalize_account_id(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return LOCAL_ACCOUNT_ID.to_string();
    }
    let lowered = trimmed.to_ascii_lowercase();
    let mut slug = String::with_capacity(lowered.len());
    let mut last_dash = false;
    for ch in lowered.chars() {
        if is_lower_alnum(ch) || ch == '.' || ch == '_' {
            slug.push(ch);
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    let slug = if slug.is_empty() {
        "acct".to_string()
    } else {
        slug.chars().take(32).collect()
    };
    if slug == lowered {
        slug
    } else {
        format!("{}-{:08x}", slug, fnv1a64(trimmed) as u32)
    }
}

/// 同步键合法性判定：`namespace:local_id`，命名空间小写起、其余字符集收紧。
pub fn validate_key(key: &str) -> Result<(), AccountError> {
    let Some((namespace, local_id)) = key.split_once(':') else {
        return Err(AccountError::InvalidKey(key.to_string()));
    };
    if key.len() > MAX_KEY_LEN {
        return Err(AccountError::InvalidKey(key.to_string()));
    }
    let ns_ok = !namespace.is_empty()
        && namespace.len() <= MAX_NAMESPACE_LEN
        && namespace.starts_with(is_lower_alnum)
        && namespace
            .chars()
            .all(|c| is_lower_alnum(c) || c == '_' || c == '-');
    let id_ok = !local_id.is_empty()
        && local_id.chars().count() <= MAX_LOCAL_ID_LEN
        && local_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-');
    if ns_ok && id_ok {
        Ok(())
    } else {
        Err(AccountError::InvalidKey(key.to_string()))
    }
}

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

/// 账户档案（展示身份 + 语言偏好；`rev` 同样服务端权威）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountProfile {
    /// 账户 id 原样（JWT `sub` 或 [`LOCAL_ACCOUNT_ID`]；展示/审计用，
    /// 存储键走 [`normalize_account_id`] 的 slug）
    pub account_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default = "default_locale")]
    pub locale: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub rev: u64,
}

fn default_locale() -> String {
    "zh".to_string()
}

impl AccountProfile {
    /// 新账户初始档案（`rev = 0` = 服务端尚未落库，落库时置 1）
    pub fn new(
        account_id: impl Into<String>,
        display_name: impl Into<String>,
        now_ms: i64,
    ) -> Self {
        Self {
            account_id: account_id.into(),
            display_name: display_name.into(),
            email: None,
            locale: default_locale(),
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            rev: 0,
        }
    }

    /// 档案校验（显示名非空且 ≤64 字符、邮箱形状、locale 为 BCP 47 形状）
    pub fn validate(&self) -> Result<(), AccountError> {
        let account_id = self.account_id.trim();
        if account_id.is_empty() || account_id.chars().count() > 128 {
            return Err(AccountError::InvalidAccountId);
        }
        let name = self.display_name.trim();
        if name.is_empty() {
            return Err(AccountError::EmptyDisplayName);
        }
        if name.chars().count() > 64 {
            return Err(AccountError::DisplayNameTooLong);
        }
        if let Some(email) = self.email.as_deref() {
            let email = email.trim();
            // 轻量形状校验（不含 @ / 有空白 / 本地段或域段为空 → 非法），
            // 真实可达性验证（确认邮件）不在本层
            let valid = !email.is_empty()
                && email.len() <= 254
                && !email.chars().any(char::is_whitespace)
                && email.split_once('@').is_some_and(|(local, domain)| {
                    !local.is_empty()
                        && domain.contains('.')
                        && !domain.starts_with('.')
                        && !domain.ends_with('.')
                });
            if !valid {
                return Err(AccountError::InvalidEmail);
            }
        }
        let locale_ok = self.locale.split('-').count() <= 3
            && self.locale.split('-').next().is_some_and(|tag| {
                (2..=3).contains(&tag.len()) && tag.chars().all(|c| c.is_ascii_alphabetic())
            })
            && self
                .locale
                .split('-')
                .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric()));
        if !locale_ok {
            return Err(AccountError::InvalidLocale);
        }
        Ok(())
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn records(pairs: Vec<SyncRecord>) -> BTreeMap<String, SyncRecord> {
        pairs.into_iter().map(|r| (r.key.clone(), r)).collect()
    }

    #[test]
    fn account_id_slug_is_stable_and_collision_free() {
        // 干净串原样返回
        assert_eq!(normalize_account_id("alice"), "alice");
        assert_eq!(normalize_account_id("  bob_1.2-x "), "bob_1.2-x");
        assert_eq!(normalize_account_id("ALICE"), "alice");
        // 空 → 本机缺省
        assert_eq!(normalize_account_id("   "), LOCAL_ACCOUNT_ID);
        // 折叠后追加摘要尾巴：不同原串不撞键
        let a = normalize_account_id("Alice@corp.com");
        let b = normalize_account_id("a l i c e");
        assert_ne!(a, b);
        assert!(a.starts_with("alice-corp.com-"), "got {}", a);
        assert!(b.starts_with("a-l-i-c-e-"), "got {}", b);
        // 幂等（同一输入两次得到同键）
        assert_eq!(a, normalize_account_id("Alice@corp.com"));
        // 全非安全字符也要有键
        assert!(normalize_account_id("!!!").starts_with("acct-"));
    }

    #[test]
    fn key_validation_shapes() {
        assert!(validate_key("workspace:9f2c").is_ok());
        assert!(validate_key("alert-rule:x.y_z-1").is_ok());
        assert_eq!(
            validate_key("workspace"),
            Err(AccountError::InvalidKey("workspace".into()))
        );
        assert_eq!(
            validate_key(":abc"),
            Err(AccountError::InvalidKey(":abc".into()))
        );
        assert_eq!(
            validate_key("Workspace:abc"),
            Err(AccountError::InvalidKey("Workspace:abc".into()))
        );
        assert_eq!(
            validate_key("workspace:"),
            Err(AccountError::InvalidKey("workspace:".into()))
        );
        // 命名空间超长 / 本地 id 超长 / 总长超限
        assert!(validate_key("aaaaaaaaaaaaaaaaa:x").is_err());
        let long_id = format!("workspace:{}", "x".repeat(MAX_LOCAL_ID_LEN + 1));
        assert!(validate_key(&long_id).is_err());
        let long_ns = format!("{}:x", "n".repeat(MAX_NAMESPACE_LEN + 1));
        assert!(validate_key(&long_ns).is_err());
    }

    #[test]
    fn record_validation_covers_rev_tombstone_and_size() {
        let ok = SyncRecord::live("workspace:a", 1, 10, json!({"name": "盯盘"}));
        assert_eq!(ok.validate(), Ok(()));
        // rev 从 1 起
        let bad_rev = SyncRecord::live("workspace:a", 0, 10, json!({}));
        assert_eq!(bad_rev.validate(), Err(AccountError::InvalidRev));
        // 墓碑必须空载荷
        let mut tomb = SyncRecord::tombstone("workspace:a", 2, 10);
        assert_eq!(tomb.validate(), Ok(()));
        tomb.payload = json!({"name": "x"});
        assert_eq!(tomb.validate(), Err(AccountError::TombstonePayloadNotNull));
        // 超大载荷（64 KiB 上限）
        let big = SyncRecord::live(
            "workspace:a",
            1,
            10,
            json!({ "blob": "x".repeat(MAX_PAYLOAD_BYTES) }),
        );
        assert!(matches!(
            big.validate(),
            Err(AccountError::PayloadTooLarge { limit, .. }) if limit == MAX_PAYLOAD_BYTES
        ));
    }

    #[test]
    fn plan_sync_pushes_local_only_change() {
        let local = records(vec![SyncRecord::live(
            "workspace:a",
            1,
            100,
            json!({"n": 1}),
        )]);
        let plan = plan_sync(
            &local,
            &BTreeMap::new(),
            &BTreeMap::new(),
            ConflictPolicy::NewestWins,
        );
        assert_eq!(plan.push.len(), 1);
        assert!(plan.pull.is_empty());
        assert!(plan.conflicts.is_empty());
        assert!(!plan.is_empty());
    }

    #[test]
    fn plan_sync_pulls_remote_only_change() {
        let base = BTreeMap::from([("workspace:a".to_string(), 1u64)]);
        let remote = records(vec![SyncRecord::live(
            "workspace:a",
            2,
            100,
            json!({"n": 2}),
        )]);
        let plan = plan_sync(&BTreeMap::new(), &remote, &base, ConflictPolicy::NewestWins);
        assert_eq!(plan.pull.len(), 1);
        assert!(plan.push.is_empty());
        assert!(plan.conflicts.is_empty());
    }

    #[test]
    fn plan_sync_converges_when_both_sides_identical() {
        // 并发同改同值：内容一致 → 下发 rev 高者推进基线，不记冲突
        let base = BTreeMap::from([("workspace:a".to_string(), 1u64)]);
        let local = records(vec![SyncRecord::live(
            "workspace:a",
            2,
            100,
            json!({"n": 7}),
        )]);
        let remote = records(vec![SyncRecord::live(
            "workspace:a",
            3,
            110,
            json!({"n": 7}),
        )]);
        let plan = plan_sync(&local, &remote, &base, ConflictPolicy::NewestWins);
        assert!(plan.push.is_empty());
        assert_eq!(plan.pull.len(), 1);
        assert_eq!(plan.pull[0].rev, 3, "取 rev 高者推进基线");
        assert!(plan.conflicts.is_empty());
    }

    #[test]
    fn plan_sync_conflict_newest_wins_in_both_directions() {
        let base = BTreeMap::from([("workspace:a".to_string(), 1u64)]);
        let local = records(vec![SyncRecord::live(
            "workspace:a",
            2,
            200,
            json!({"n": "local"}),
        )]);
        let remote = records(vec![SyncRecord::live(
            "workspace:a",
            2,
            300,
            json!({"n": "remote"}),
        )]);
        let plan = plan_sync(&local, &remote, &base, ConflictPolicy::NewestWins);
        assert_eq!(plan.pull.len(), 1);
        assert!(plan.push.is_empty());
        assert_eq!(plan.conflicts.len(), 1);
        assert_eq!(plan.conflicts[0].winner, ConflictWinner::Remote);

        // 本地更新 → 本地胜
        let local_newer = records(vec![SyncRecord::live(
            "workspace:a",
            2,
            400,
            json!({"n": "local"}),
        )]);
        let plan = plan_sync(&local_newer, &remote, &base, ConflictPolicy::NewestWins);
        assert_eq!(plan.push.len(), 1);
        assert!(plan.pull.is_empty());
        assert_eq!(plan.conflicts[0].winner, ConflictWinner::Local);
    }

    #[test]
    fn plan_sync_conflict_tie_breaks_to_remote_deterministically() {
        // 时间戳并列：两端必须算出同一裁决，否则反复互相覆盖
        let base = BTreeMap::from([("workspace:a".to_string(), 1u64)]);
        let local = records(vec![SyncRecord::live(
            "workspace:a",
            2,
            100,
            json!({"n": "local"}),
        )]);
        let remote = records(vec![SyncRecord::live(
            "workspace:a",
            2,
            100,
            json!({"n": "remote"}),
        )]);
        let from_local = plan_sync(&local, &remote, &base, ConflictPolicy::NewestWins);
        let from_remote = plan_sync(&remote, &local, &base, ConflictPolicy::NewestWins);
        assert_eq!(from_local.pull[0].key, "workspace:a");
        assert_eq!(from_remote.pull[0].key, "workspace:a");
        assert_eq!(from_local.conflicts[0].winner, ConflictWinner::Remote);
        assert_eq!(from_remote.conflicts[0].winner, ConflictWinner::Remote);
    }

    #[test]
    fn plan_sync_explicit_policies_override_recency() {
        let base = BTreeMap::from([("workspace:a".to_string(), 1u64)]);
        let local = records(vec![SyncRecord::live(
            "workspace:a",
            2,
            500,
            json!({"n": "local"}),
        )]);
        let remote = records(vec![SyncRecord::live(
            "workspace:a",
            2,
            100,
            json!({"n": "remote"}),
        )]);
        let forced_local = plan_sync(&local, &remote, &base, ConflictPolicy::LocalWins);
        assert_eq!(forced_local.push.len(), 1);
        assert_eq!(forced_local.conflicts[0].winner, ConflictWinner::Local);
        let forced_remote = plan_sync(&local, &remote, &base, ConflictPolicy::RemoteWins);
        assert_eq!(forced_remote.pull.len(), 1);
        assert_eq!(forced_remote.conflicts[0].winner, ConflictWinner::Remote);
    }

    #[test]
    fn plan_sync_tombstones_propagate_and_conflict_with_live() {
        let base = BTreeMap::from([("workspace:a".to_string(), 3u64)]);
        // 本端删除（较新）→ 墓碑上行
        let local = records(vec![SyncRecord::tombstone("workspace:a", 4, 500)]);
        let remote = records(vec![SyncRecord::live(
            "workspace:a",
            3,
            100,
            json!({"n": 1}),
        )]);
        let plan = plan_sync(&local, &remote, &base, ConflictPolicy::NewestWins);
        assert_eq!(plan.push.len(), 1);
        assert!(plan.push[0].deleted);
        assert!(plan.conflicts.is_empty());

        // 他端删除（较新）→ 下发墓碑，本地视图按墓碑移除
        let remote_tomb = records(vec![SyncRecord::tombstone("workspace:a", 4, 600)]);
        let plan = plan_sync(&local, &remote_tomb, &base, ConflictPolicy::NewestWins);
        assert!(plan.push.is_empty());
        assert_eq!(plan.pull.len(), 1);
        assert!(plan.pull[0].deleted);
    }

    #[test]
    fn plan_sync_missing_key_is_no_opinion_not_deletion() {
        // 服务端删除该键（基线有、本地无、服务端无）→ 无传输（本地缺键只是
        // 「没这份数据」，不等于本地删过——删除必须走墓碑）
        let base = BTreeMap::from([("workspace:gone".to_string(), 2u64)]);
        let plan = plan_sync(
            &BTreeMap::new(),
            &BTreeMap::new(),
            &base,
            ConflictPolicy::NewestWins,
        );
        assert!(plan.is_empty());
        assert!(plan.conflicts.is_empty());
    }

    #[test]
    fn two_sided_conflict_converges_after_resolution() {
        // 端到端收敛模拟：A 与 B 都改了同一键，A 裁决后上行，服务端下发，
        // B 应用后与 A 视图逐字节一致
        let base = BTreeMap::from([("workspace:a".to_string(), 1u64)]);
        let a_view = records(vec![SyncRecord::live(
            "workspace:a",
            2,
            200,
            json!({"n": "A"}),
        )]);
        let b_view = records(vec![SyncRecord::live(
            "workspace:a",
            2,
            100,
            json!({"n": "B"}),
        )]);
        let a_base = base.clone();
        let b_base = base.clone();

        // A 侧：本地较新 → 上行
        let a_plan = plan_sync(&a_view, &b_view, &a_base, ConflictPolicy::NewestWins);
        let pushed = a_plan.push.clone();
        assert_eq!(pushed.len(), 1);

        // 服务端接受（rev 3）并回传权威副本
        let mut server = b_view.clone();
        for record in &pushed {
            server.insert(
                record.key.clone(),
                SyncRecord::live(record.key.clone(), 3, 400, record.payload.clone()),
            );
        }
        // A 应用服务端回执，基线推进
        let mut a_state = SyncState::new();
        a_state.base = base.clone();
        let mut a_local = a_view.clone();
        let response = SyncResponse {
            cursor: 10,
            accepted: server.values().cloned().collect(),
            rejected: vec![],
            changes: vec![],
        };
        let outcome = a_state.apply_response(&response);
        SyncState::apply_to_view(&mut a_local, &outcome.applied);

        // B 侧：拉到权威副本后与服务端一致
        let b_plan = plan_sync(&b_view, &server, &b_base, ConflictPolicy::NewestWins);
        let mut b_local = b_view.clone();
        SyncState::apply_to_view(&mut b_local, &b_plan.pull);
        assert_eq!(a_local, b_local, "两端收敛到同一份数据");
        // B 的本地改动被 A 覆盖 = 真冲突，须留痕（胜方服务端：NewestWins
        // 比 updated_at，A 的 400 > B 的 100）
        assert_eq!(b_plan.conflicts.len(), 1);
        assert_eq!(b_plan.conflicts[0].winner, ConflictWinner::Remote);
    }

    #[test]
    fn sync_state_outbox_coalesces_and_bounds() {
        let mut state = SyncState::new();
        for i in 0..(MAX_PUSHES_PER_REQUEST + 10) {
            state.note_local_change(
                &format!("workspace:{:03}", i),
                json!({"i": i}),
                100 + i as i64,
            );
        }
        let pending = state.pending_pushes();
        assert_eq!(pending.len(), MAX_PUSHES_PER_REQUEST, "超出部分留队下一轮");
        // 同键后写覆盖前写（连续编辑不堆多条）
        state.note_local_change("workspace:000", json!({"i": 999}), 999);
        assert_eq!(state.outbox.len(), MAX_PUSHES_PER_REQUEST + 10);
        assert_eq!(state.outbox["workspace:000"].payload, json!({"i": 999}));
    }

    #[test]
    fn sync_state_build_request_carries_base_revs() {
        let mut state = SyncState::new();
        state.base.insert("workspace:a".to_string(), 4);
        state.note_local_change("workspace:a", json!({"n": 1}), 10);
        state.note_local_delete("workspace:b", 11);
        let request = state.build_request();
        assert_eq!(request.cursor, 0);
        assert_eq!(request.base.get("workspace:a"), Some(&4));
        let a = request
            .pushes
            .iter()
            .find(|p| p.key == "workspace:a")
            .unwrap();
        assert_eq!(a.base_rev, 4, "基线 rev 随推送上行，乐观并发靠它比对");
        assert!(!a.deleted);
        let b = request
            .pushes
            .iter()
            .find(|p| p.key == "workspace:b")
            .unwrap();
        assert!(b.deleted && b.payload.is_null());
    }

    #[test]
    fn sync_state_apply_response_advances_base_and_acks() {
        let mut state = SyncState::new();
        state.note_local_change("workspace:a", json!({"n": 1}), 10);
        state.note_local_change("workspace:b", json!({"n": 2}), 10);
        let response = SyncResponse {
            cursor: 42,
            accepted: vec![SyncRecord::live("workspace:a", 1, 20, json!({"n": 1}))],
            rejected: vec![SyncRejection {
                key: "workspace:b".into(),
                reason: RejectionReason::Conflict,
                server: Some(SyncRecord::live(
                    "workspace:b",
                    9,
                    5,
                    json!({"n": "server"}),
                )),
            }],
            changes: vec![
                SyncRecord::live("workspace:z", 7, 30, json!({"n": 3})),
                SyncRecord::tombstone("workspace:old", 8, 31),
            ],
        };
        let outcome = state.apply_response(&response);
        assert_eq!(state.cursor, 42);
        assert_eq!(state.base.get("workspace:a"), Some(&1));
        assert!(state.base.contains_key("workspace:z"));
        assert_eq!(outcome.acked, vec!["workspace:a"]);
        assert_eq!(outcome.applied.len(), 3, "1 接受 + 2 增量");
        assert_eq!(outcome.conflicts.len(), 1);
        assert_eq!(outcome.conflicts[0].server.as_ref().unwrap().rev, 9);
        assert!(state.outbox.contains_key("workspace:b"), "冲突项留队待裁决");

        // 墓碑落地即从本地视图移除
        let mut view = records(vec![SyncRecord::live(
            "workspace:old",
            8,
            31,
            json!({"n": 0}),
        )]);
        SyncState::apply_to_view(&mut view, &outcome.applied);
        assert!(!view.contains_key("workspace:old"), "墓碑 = 本地删除");
        assert!(view.contains_key("workspace:z"));
    }

    #[test]
    fn sync_state_ignores_cursor_regression() {
        // 服务端重启可能从低水位重发（游标回退）；旧增量不得被当新数据应用
        let mut state = SyncState::new();
        state.cursor = 100;
        let response = SyncResponse {
            cursor: 5,
            accepted: vec![],
            rejected: vec![],
            changes: vec![SyncRecord::live(
                "workspace:a",
                1,
                1,
                json!({"stale": true}),
            )],
        };
        let outcome = state.apply_response(&response);
        assert!(outcome.applied.is_empty());
        assert_eq!(state.cursor, 100);
    }

    #[test]
    fn sync_push_projection_validates_locally() {
        let push = SyncPush {
            key: "workspace:a".into(),
            base_rev: 2,
            deleted: false,
            payload: json!({"n": 1}),
            updated_at_ms: 10,
        };
        assert_eq!(push.validate(), Ok(()));
        assert_eq!(
            push.to_record().rev,
            3,
            "投影 rev = base_rev + 1（乐观占位）"
        );

        let mut bad = push.clone();
        bad.key = "nope".into();
        assert!(matches!(bad.validate(), Err(AccountError::InvalidKey(_))));

        let mut tomb_bad = push.clone();
        tomb_bad.deleted = true;
        assert_eq!(
            tomb_bad.validate(),
            Err(AccountError::TombstonePayloadNotNull)
        );
    }

    #[test]
    fn profile_validation_and_defaults() {
        let profile = AccountProfile::new("alice", "Alice", 100);
        assert_eq!(profile.locale, "zh");
        assert_eq!(profile.rev, 0);
        assert_eq!(profile.validate(), Ok(()));

        let mut p = profile.clone();
        p.display_name = "   ".into();
        assert_eq!(p.validate(), Err(AccountError::EmptyDisplayName));
        p.display_name = "x".repeat(65);
        assert_eq!(p.validate(), Err(AccountError::DisplayNameTooLong));
        p.display_name = "Alice".into();
        p.email = Some("bad-at".into());
        assert_eq!(p.validate(), Err(AccountError::InvalidEmail));
        p.email = Some("a@b.com".into());
        assert_eq!(p.validate(), Ok(()));
        p.email = Some("a@ b.com".into());
        assert_eq!(p.validate(), Err(AccountError::InvalidEmail));
        p.email = None;
        p.locale = "zh_CN".into();
        assert_eq!(p.validate(), Err(AccountError::InvalidLocale));
        p.locale = "en-US".into();
        assert_eq!(p.validate(), Ok(()));
        p.locale = "e".into();
        assert_eq!(p.validate(), Err(AccountError::InvalidLocale));
        p.account_id = "".into();
        assert_eq!(p.validate(), Err(AccountError::InvalidAccountId));
    }

    #[test]
    fn wire_shapes_round_trip_with_server_contract() {
        // wire 契约锁定：snake_case 字段、判别式拒绝原因原名（服务端与
        // 各端客户端共用同一份解析代码，形状变了就是协议破坏）
        let request = SyncRequest {
            cursor: 3,
            base: BTreeMap::from([("workspace:a".to_string(), 2u64)]),
            pushes: vec![SyncPush {
                key: "workspace:a".into(),
                base_rev: 2,
                deleted: false,
                payload: json!({"name": "盯盘"}),
                updated_at_ms: 100,
            }],
        };
        let text = serde_json::to_string(&request).unwrap();
        assert!(text.contains("\"base_rev\":2"), "got {}", text);
        assert_eq!(serde_json::from_str::<SyncRequest>(&text).unwrap(), request);

        let response = SyncResponse {
            cursor: 4,
            accepted: vec![SyncRecord::live(
                "workspace:a",
                3,
                110,
                json!({"name": "盯盘"}),
            )],
            rejected: vec![SyncRejection {
                key: "workspace:b".into(),
                reason: RejectionReason::Conflict,
                server: None,
            }],
            changes: vec![SyncRecord::tombstone("workspace:old", 4, 120)],
        };
        let text = serde_json::to_string(&response).unwrap();
        assert!(text.contains("\"reason\":\"conflict\""), "got {}", text);
        assert!(text.contains("\"deleted\":true"), "got {}", text);
        assert_eq!(
            serde_json::from_str::<SyncResponse>(&text).unwrap(),
            response
        );

        assert_eq!(
            serde_json::to_string(&ConflictPolicy::NewestWins).unwrap(),
            "\"newest_wins\""
        );
    }
}
