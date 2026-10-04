//! 统一账户与跨端数据同步的服务端存储面（TODO L476）
//!
//! 领域语义（[`alpha_core::account`]）是唯一事实源；本模块只负责**服务端
//! 权威面**：每账户一份档案 + 记录表，接受乐观并发推送、回传增量。
//!
//! - **账户隔离**：存储键 = `normalize_account_id(sub)` 的 slug（存储层
//!   再加一层固定前缀），一个账户的同步包绝不可能落到另一个账户名下；
//! - **权威 rev**：服务端分配 `rev`（记录的当前版本）与 `touched_seq`
//!   （账户内单调水位，客户端增量游标）。客户端只提交 `base_rev`，
//!   与当前 `rev` 不符即判冲突并回权威副本（乐观并发，非最后写入者
//!   胜——后者会让离线设备静默丢数据）；
//! - **增量拉取**：`cursor` 之后被改过的记录全量下发（记录数面按账户
//!   规模小而取全量，不做逐记录水位——见 docs/account-sync.md 规模口径）；
//! - **持久化**：内存为默认（零外部依赖、单机形态即可跑），配置
//!   `ALPHA_GATEWAY_ACCOUNT_STORE_URL` 时经 [`StorageBackend`] 写穿
//!   （Postgres KV 表，账户状态整体 JSON 落一个键）——写失败只告警不
//!   拒绝请求（内存仍是权威 serving 层，与 data-engine 的 Timescale
//!   镜像口径一致）。
//!
//! **锁与 await 的边界**：`std::sync::Mutex` 只护内存结构（临界区是纯
//! 计算，微秒级），持久化的 `retrieve`/`store` 一律在锁外 `.await`——
//! 把网络往返放进临界区会让一个后端慢查询把所有账户请求一起堵住，
//! 而 `block_in_place` 在 current_thread 运行时上还会直接 panic。代价是
//! 写穿为「快照后覆盖写」：并发两次改动时后到者覆盖先到者（内存已收敛，
//! 重启后取到的是其中一个版本，不会出现撕裂快照）。

use alpha_core::account::{
    normalize_account_id, AccountProfile, RejectionReason, SyncPush, SyncRecord, SyncRejection,
    SyncRequest, SyncResponse, LOCAL_ACCOUNT_ID, MAX_PUSHES_PER_REQUEST,
};
use alpha_storage::StorageBackend;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

/// 持久化键前缀（与 alpha-storage 自身表前缀区分）
const STORE_PREFIX: &str = "alpha:account:";

/// 账户档案响应（rev 为服务端权威版本）
#[derive(Debug, Clone, Serialize)]
pub struct ProfileResponse {
    pub account_id: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    pub locale: String,
    pub rev: u64,
}

/// 服务端账户状态（内存结构；序列化即持久化快照）
#[derive(Debug, Clone)]
struct AccountData {
    profile: AccountProfile,
    /// 当前权威记录表（键 → 记录 + 最后改动水位）
    records: BTreeMap<String, StoredRecord>,
    /// 账户内单调水位分配器（next_seq 之前的值都已下发过）
    next_seq: u64,
}

#[derive(Debug, Clone)]
struct StoredRecord {
    record: SyncRecord,
    /// 该记录最后一次被接受改动时的账户内水位（增量拉取过滤量）
    touched_seq: u64,
}

impl Default for AccountData {
    fn default() -> Self {
        Self {
            profile: AccountProfile::new("", "", 0),
            records: BTreeMap::new(),
            next_seq: 1,
        }
    }
}

impl AccountData {
    fn ensure_profile(&mut self, account_id: &str, now_ms: i64) {
        if self.profile.rev == 0 {
            self.profile =
                AccountProfile::new(account_id, default_display_name(account_id), now_ms);
            self.profile.rev = 1;
            self.next_seq = 1;
        }
    }

    fn allocate_seq(&mut self) -> u64 {
        let seq = self.next_seq.max(1);
        self.next_seq = seq + 1;
        seq
    }

    /// 当前可下发的水位（= 已分配过的最大值）
    fn cursor(&self) -> u64 {
        self.next_seq.saturating_sub(1)
    }

    fn to_snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "profile": self.profile,
            "records": self.records
                .values()
                .map(|stored| serde_json::json!({
                    "record": stored.record,
                    "touched_seq": stored.touched_seq,
                }))
                .collect::<Vec<_>>(),
            "next_seq": self.next_seq,
        })
    }

    fn from_snapshot(value: &serde_json::Value) -> Option<Self> {
        let profile: AccountProfile = serde_json::from_value(value.get("profile")?.clone()).ok()?;
        let entries = value.get("records")?.as_array()?;
        let mut records = BTreeMap::new();
        for entry in entries {
            let record: SyncRecord = serde_json::from_value(entry.get("record")?.clone()).ok()?;
            let touched_seq = entry.get("touched_seq")?.as_u64()?;
            records.insert(
                record.key.clone(),
                StoredRecord {
                    record,
                    touched_seq,
                },
            );
        }
        let next_seq = value.get("next_seq")?.as_u64()?;
        Some(Self {
            profile,
            records,
            next_seq,
        })
    }
}

/// 缺省展示名（`sub` 本体；账户档案由用户改，不替 IdP 编造姓名）
fn default_display_name(account_id: &str) -> String {
    if account_id == LOCAL_ACCOUNT_ID {
        "本机用户".to_string()
    } else {
        account_id.to_string()
    }
}

/// 账户存储（内存权威 + 可选写穿持久化）
#[derive(Clone)]
pub struct AccountStore {
    inner: Arc<Mutex<HashMap<String, AccountData>>>,
    persistence: Option<Arc<dyn StorageBackend>>,
}

impl std::fmt::Debug for AccountStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountStore")
            .field("persistence", &self.persistence.is_some())
            .finish()
    }
}

impl AccountStore {
    /// 内存形态（默认，零外部依赖）
    pub fn in_memory() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            persistence: None,
        }
    }

    /// 带持久化形态（`backend` 为写穿后端；连接失败由调用方决定是否启动）
    pub fn with_persistence(backend: Arc<dyn StorageBackend>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            persistence: Some(backend),
        }
    }

    fn store_key(account_id: &str) -> String {
        format!("{STORE_PREFIX}{}", normalize_account_id(account_id))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, AccountData>> {
        self.inner.lock().expect("account store mutex poisoned")
    }

    /// 首次访问该账户时从持久化懒加载（锁外 await；加载期间别处可能已建好
    /// 同一账户的内存状态，故最后 `or_insert` 保住先到者）
    async fn ensure_loaded(&self, account_id: &str, now_ms: i64) {
        if self.lock().contains_key(account_id) {
            return;
        }
        let loaded = match self.persistence.as_ref() {
            Some(backend) => {
                let key = Self::store_key(account_id);
                match backend.retrieve(&key).await {
                    Ok(Some(bytes)) => serde_json::from_slice::<serde_json::Value>(&bytes)
                        .ok()
                        .and_then(|value| AccountData::from_snapshot(&value)),
                    Ok(None) => None,
                    Err(err) => {
                        tracing::warn!(%account_id, %err, "account snapshot load failed");
                        None
                    }
                }
            }
            None => None,
        };
        let mut data = loaded.unwrap_or_default();
        data.ensure_profile(account_id, now_ms);
        self.lock().entry(account_id.to_string()).or_insert(data);
    }

    /// 内存状态的持久化快照（锁内克隆：临界区纯序列化，载荷面按账户规模小）
    fn snapshot(&self, account_id: &str) -> Option<serde_json::Value> {
        self.lock().get(account_id).map(|data| data.to_snapshot())
    }

    /// 写穿持久化（失败只告警——内存是权威 serving 层；锁外 await）
    async fn persist(&self, account_id: &str) {
        let Some(backend) = self.persistence.as_ref() else {
            return;
        };
        let Some(snapshot) = self.snapshot(account_id) else {
            return;
        };
        let Ok(payload) = serde_json::to_vec(&snapshot) else {
            return;
        };
        if let Err(err) = backend.store(&Self::store_key(account_id), payload).await {
            tracing::warn!(%account_id, %err, "account snapshot persist failed");
        }
    }

    /// 取账户状态并执行纯内存改动（调用方先 [`Self::ensure_loaded`]）
    fn with_data<T>(&self, account_id: &str, f: impl FnOnce(&mut AccountData) -> T) -> T {
        let mut guard = self.lock();
        f(guard.entry(account_id.to_string()).or_default())
    }

    /// 档案读取（不存在则按 `sub` 建初始档案）
    pub async fn profile(&self, account_id: &str, now_ms: i64) -> AccountProfile {
        self.ensure_loaded(account_id, now_ms).await;
        self.with_data(account_id, |data| data.profile.clone())
    }

    /// 档案更新（字段级部分更新；`None` = 不改）。校验失败返回错误串。
    /// 成功后 rev +1 并入增量水位（各端拉得到）。
    pub async fn update_profile(
        &self,
        account_id: &str,
        patch: &ProfilePatch,
        now_ms: i64,
    ) -> Result<AccountProfile, String> {
        self.ensure_loaded(account_id, now_ms).await;
        let updated = self.with_data(account_id, |data| {
            let mut profile = data.profile.clone();
            if let Some(display_name) = patch.display_name.as_deref() {
                profile.display_name = display_name.to_string();
            }
            if let Some(email) = patch.email.as_deref() {
                profile.email = if email.trim().is_empty() {
                    None
                } else {
                    Some(email.trim().to_string())
                };
            }
            if let Some(locale) = patch.locale.as_deref() {
                profile.locale = locale.trim().to_string();
            }
            profile.validate().map_err(|err| err.to_string())?;
            profile.updated_at_ms = now_ms;
            profile.rev += 1;
            // 档案不是同步记录（记录面才占增量位）；此处仍触碰一次水位，
            // 让「档案改过」这件事对按 cursor 观察的客户端可见
            let _ = data.allocate_seq();
            data.profile = profile.clone();
            Ok(profile)
        });
        // 校验失败不改状态，也就没什么可写穿的
        if updated.is_ok() {
            self.persist(account_id).await;
        }
        updated
    }

    /// 账户数据删除（`DELETE /api/v1/account`，data-privacy §4 / account-sync
    /// §6 登记的 purge 端点）：档案 + 同步记录整体清除。持久层先删、内存后
    /// 删——后端删除失败时内存态保持原样并上抛错误，绝不出现「声称已删而
    /// 快照还在持久层、懒加载一碰就复活」的半删状态。幂等：重复删或删不
    /// 存在的账户都返回 false（无数据可删），不是错误。
    pub async fn delete(&self, account_id: &str) -> Result<bool, String> {
        let existed_backend = match self.persistence.as_ref() {
            Some(backend) => backend
                .delete(&Self::store_key(account_id))
                .await
                .map_err(|err| {
                    tracing::warn!(%account_id, %err, "account snapshot delete failed");
                    err.to_string()
                })?,
            None => false,
        };
        let had_memory = self.lock().remove(account_id).is_some();
        Ok(had_memory || existed_backend)
    }

    /// 同步往返：接受推送 + 回传增量。
    ///
    /// 接受条件：键/载荷校验通过 且（服务端无该键 或 `base_rev` 等于
    /// 当前权威 `rev`）。不符者回 [`RejectionReason::Conflict`] 并带权威
    /// 副本，客户端按 `alpha_core::account::plan_sync` 裁决。
    pub async fn sync(&self, account_id: &str, request: &SyncRequest, now_ms: i64) -> SyncResponse {
        self.ensure_loaded(account_id, now_ms).await;
        let response = self.with_data(account_id, |data| {
            let mut accepted = Vec::new();
            let mut rejected = Vec::new();

            // 推送条数上限（客户端已分片，服务端再兜一层——单请求放大是
            // 现成的拒绝服务面）
            for push in request.pushes.iter().take(MAX_PUSHES_PER_REQUEST) {
                match try_apply(data, push, now_ms) {
                    Ok(record) => accepted.push(record),
                    Err(rejection) => rejected.push(rejection),
                }
            }

            // 增量拉取：水位之后被动过的记录（含本次接受项——客户端据
            // accepted 里的权威 rev 直接更新基线，changes 里重复出现也
            // 是幂等的）
            let cursor = request.cursor;
            let mut changes: Vec<SyncRecord> = data
                .records
                .values()
                .filter(|stored| stored.touched_seq > cursor)
                .map(|stored| stored.record.clone())
                .collect();
            changes.sort_by(|a, b| a.key.cmp(&b.key));
            let next_cursor = data.cursor();

            metrics::counter!(
                "alpha_gateway_account_sync_total",
                "outcome" => "applied"
            )
            .increment(accepted.len() as u64);
            metrics::counter!(
                "alpha_gateway_account_sync_total",
                "outcome" => "rejected"
            )
            .increment(rejected.len() as u64);

            SyncResponse {
                cursor: next_cursor,
                accepted,
                rejected,
                changes,
            }
        });
        if !request.pushes.is_empty() {
            self.persist(account_id).await;
        }
        response
    }
}

/// 单条推送的服务端判定（纯函数：只碰内存状态，无 IO）
fn try_apply(
    data: &mut AccountData,
    push: &SyncPush,
    now_ms: i64,
) -> Result<SyncRecord, SyncRejection> {
    if let Err(err) = push.validate() {
        let reason = match err {
            alpha_core::account::AccountError::InvalidKey(_) => RejectionReason::InvalidKey,
            alpha_core::account::AccountError::PayloadTooLarge { .. } => {
                RejectionReason::PayloadTooLarge
            }
            alpha_core::account::AccountError::TombstonePayloadNotNull => {
                RejectionReason::TombstonePayloadNotNull
            }
            _ => RejectionReason::InvalidKey,
        };
        return Err(SyncRejection {
            key: push.key.clone(),
            reason,
            server: None,
        });
    }

    let existing = data.records.get(&push.key);
    match existing {
        Some(stored) if stored.record.rev != push.base_rev => Err(SyncRejection {
            key: push.key.clone(),
            reason: RejectionReason::Conflict,
            server: Some(stored.record.clone()),
        }),
        _ => {
            let prev_rev = existing.map(|s| s.record.rev).unwrap_or(0);
            let seq = data.allocate_seq();
            let record = SyncRecord {
                key: push.key.clone(),
                rev: prev_rev + 1,
                // 服务端时间为准（客户端时钟不可信）；NewestWins 裁决
                // 用的 updated_at_ms 因此始终单调可比
                updated_at_ms: now_ms,
                deleted: push.deleted,
                payload: push.payload.clone(),
            };
            data.records.insert(
                record.key.clone(),
                StoredRecord {
                    record: record.clone(),
                    touched_seq: seq,
                },
            );
            Ok(record)
        }
    }
}

/// 档案更新请求（字段级部分更新；缺省 = 不改，空串邮箱 = 清除）
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ProfilePatch {
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub locale: Option<String>,
}

/// 同步结果统计（仅测试断言用：二进制 crate 无外部消费者，指标本身在
/// `sync` 内就地发出）
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncStats {
    pub accepted: usize,
    pub rejected: usize,
    pub changes: usize,
}

#[cfg(test)]
impl SyncStats {
    pub fn of(response: &SyncResponse) -> Self {
        Self {
            accepted: response.accepted.len(),
            rejected: response.rejected.len(),
            changes: response.changes.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alpha_core::account::{ConflictPolicy, SyncState};
    use serde_json::json;

    fn store() -> AccountStore {
        AccountStore::in_memory()
    }

    fn request(cursor: u64, pushes: Vec<SyncPush>) -> SyncRequest {
        SyncRequest {
            cursor,
            base: BTreeMap::new(),
            pushes,
        }
    }

    fn push_of(key: &str, base_rev: u64, payload: serde_json::Value) -> SyncPush {
        SyncPush {
            key: key.to_string(),
            base_rev,
            deleted: false,
            payload,
            updated_at_ms: 1,
        }
    }

    #[tokio::test]
    async fn profile_defaults_from_sub_and_updates_rev() {
        let store = store();
        let created = store.profile("alice", 100).await;
        assert_eq!(created.account_id, "alice");
        assert_eq!(created.display_name, "alice");
        assert_eq!(created.rev, 1);
        // 缺省档案不重复初始化
        assert_eq!(store.profile("alice", 200).await.rev, 1);

        let updated = store
            .update_profile(
                "alice",
                &ProfilePatch {
                    display_name: Some("Alice Chen".into()),
                    locale: Some("en-US".into()),
                    email: Some("alice@example.com".into()),
                },
                300,
            )
            .await
            .unwrap();
        assert_eq!(updated.display_name, "Alice Chen");
        assert_eq!(updated.rev, 2);
        assert_eq!(updated.updated_at_ms, 300);
        assert_eq!(updated.created_at_ms, 100, "创建时间不变");
        // 只改 locale 不影响其他字段
        let locale_only = store
            .update_profile(
                "alice",
                &ProfilePatch {
                    locale: Some("zh".into()),
                    ..Default::default()
                },
                400,
            )
            .await
            .unwrap();
        assert_eq!(locale_only.rev, 3);
        assert_eq!(locale_only.display_name, "Alice Chen");
        assert_eq!(locale_only.email.as_deref(), Some("alice@example.com"));
    }

    #[tokio::test]
    async fn profile_update_rejects_invalid_and_keeps_state() {
        let store = store();
        store.profile("alice", 100).await;
        let err = store
            .update_profile(
                "alice",
                &ProfilePatch {
                    display_name: Some("  ".into()),
                    ..Default::default()
                },
                200,
            )
            .await
            .unwrap_err();
        assert!(err.contains("显示名"), "got {}", err);
        // 拒绝不推进 rev（失败不应改变服务端状态）
        assert_eq!(store.profile("alice", 300).await.rev, 1);
        // 空邮箱串 = 清除
        let cleared = store
            .update_profile(
                "alice",
                &ProfilePatch {
                    email: Some(String::new()),
                    ..Default::default()
                },
                400,
            )
            .await
            .unwrap();
        assert_eq!(cleared.email, None);
    }

    #[tokio::test]
    async fn local_account_id_is_used_when_auth_is_off() {
        let store = store();
        let profile = store.profile(LOCAL_ACCOUNT_ID, 100).await;
        assert_eq!(profile.display_name, "本机用户");
    }

    #[tokio::test]
    async fn sync_accepts_new_record_and_assigns_rev_one() {
        let store = store();
        let response = store
            .sync(
                "alice",
                &request(0, vec![push_of("workspace:a", 0, json!({"name": "盯盘"}))]),
                500,
            )
            .await;
        assert_eq!(
            SyncStats::of(&response),
            SyncStats {
                accepted: 1,
                rejected: 0,
                changes: 1
            }
        );
        assert_eq!(response.accepted[0].rev, 1);
        assert_eq!(
            response.accepted[0].updated_at_ms, 500,
            "时间戳以服务端时钟为准"
        );
        assert_eq!(response.cursor, 1);
    }

    #[tokio::test]
    async fn sync_rejects_stale_base_rev_with_authoritative_copy() {
        let store = store();
        store
            .sync(
                "alice",
                &request(0, vec![push_of("workspace:a", 0, json!({"n": 1}))]),
                100,
            )
            .await;
        // 客户端仍持 base_rev=0（其实已到 1）→ 冲突，回权威副本
        let response = store
            .sync(
                "alice",
                &request(1, vec![push_of("workspace:a", 0, json!({"n": 2}))]),
                200,
            )
            .await;
        assert_eq!(response.rejected.len(), 1);
        assert_eq!(response.rejected[0].reason, RejectionReason::Conflict);
        let server = response.rejected[0].server.as_ref().unwrap();
        assert_eq!(server.rev, 1);
        assert_eq!(server.payload, json!({"n": 1}), "服务端副本不被客户端覆盖");
        // 带正确 base_rev 即通过
        let ok = store
            .sync(
                "alice",
                &request(1, vec![push_of("workspace:a", 1, json!({"n": 2}))]),
                300,
            )
            .await;
        assert_eq!(ok.accepted[0].rev, 2);
        assert_eq!(ok.accepted[0].payload, json!({"n": 2}));
    }

    #[tokio::test]
    async fn sync_validates_keys_and_payloads() {
        let store = store();
        let bad_key = store
            .sync(
                "alice",
                &request(0, vec![push_of("nope", 0, json!({}))]),
                100,
            )
            .await;
        assert_eq!(bad_key.rejected[0].reason, RejectionReason::InvalidKey);

        let mut tombstone_with_payload = push_of("workspace:a", 0, json!({"n": 1}));
        tombstone_with_payload.deleted = true;
        let bad_tomb = store
            .sync("alice", &request(0, vec![tombstone_with_payload]), 100)
            .await;
        assert_eq!(
            bad_tomb.rejected[0].reason,
            RejectionReason::TombstonePayloadNotNull
        );

        let big = store
            .sync(
                "alice",
                &request(
                    0,
                    vec![push_of(
                        "workspace:a",
                        0,
                        json!({"blob": "x".repeat(alpha_core::account::MAX_PAYLOAD_BYTES)}),
                    )],
                ),
                100,
            )
            .await;
        assert_eq!(big.rejected[0].reason, RejectionReason::PayloadTooLarge);
        assert!(big.accepted.is_empty());
    }

    #[tokio::test]
    async fn sync_caps_push_batch_size() {
        let store = store();
        let pushes: Vec<SyncPush> = (0..MAX_PUSHES_PER_REQUEST + 5)
            .map(|i| push_of(&format!("workspace:{i:03}"), 0, json!({"i": i})))
            .collect();
        let response = store.sync("alice", &request(0, pushes), 100).await;
        assert_eq!(response.accepted.len(), MAX_PUSHES_PER_REQUEST);
        assert_eq!(response.changes.len(), MAX_PUSHES_PER_REQUEST);
    }

    #[tokio::test]
    async fn sync_pull_is_incremental_by_cursor() {
        let store = store();
        store
            .sync(
                "alice",
                &request(0, vec![push_of("workspace:a", 0, json!({"n": 1}))]),
                100,
            )
            .await;
        let after_first = store.sync("alice", &request(1, vec![]), 200).await;
        assert!(after_first.changes.is_empty(), "水位之后无变更 = 空增量");
        assert_eq!(after_first.cursor, 1);

        store
            .sync(
                "alice",
                &request(1, vec![push_of("workspace:b", 0, json!({"n": 2}))]),
                300,
            )
            .await;
        let next = store.sync("alice", &request(1, vec![]), 400).await;
        assert_eq!(next.changes.len(), 1);
        assert_eq!(next.changes[0].key, "workspace:b");
        assert_eq!(next.cursor, 2);
    }

    #[tokio::test]
    async fn sync_tombstone_propagates_as_record() {
        let store = store();
        store
            .sync(
                "alice",
                &request(0, vec![push_of("workspace:a", 0, json!({"n": 1}))]),
                100,
            )
            .await;
        let mut deletion = push_of("workspace:a", 1, serde_json::Value::Null);
        deletion.deleted = true;
        let response = store.sync("alice", &request(1, vec![deletion]), 200).await;
        assert!(response.accepted[0].deleted);
        assert_eq!(response.accepted[0].rev, 2);
        assert!(response.accepted[0].payload.is_null());
        // 墓碑在增量里也带 deleted 位，客户端据此删除本地
        assert!(response
            .changes
            .iter()
            .any(|r| r.key == "workspace:a" && r.deleted));
    }

    #[tokio::test]
    async fn accounts_are_isolated_from_each_other() {
        let store = store();
        store
            .sync(
                "alice",
                &request(0, vec![push_of("workspace:a", 0, json!({"secret": "A"}))]),
                100,
            )
            .await;
        // bob 用同一个键名推自己的数据，alice 的记录不受影响
        let bob = store
            .sync(
                "bob",
                &request(0, vec![push_of("workspace:a", 0, json!({"secret": "B"}))]),
                100,
            )
            .await;
        assert_eq!(bob.accepted[0].rev, 1);
        assert_eq!(bob.accepted[0].payload, json!({"secret": "B"}));
        // alice 拉取只会看到自己的
        let alice_view = store.sync("alice", &request(1, vec![]), 200).await;
        assert!(alice_view
            .changes
            .iter()
            .all(|r| r.payload.get("secret") != Some(&json!("B"))));
        // 档案同样隔离
        store
            .update_profile(
                "alice",
                &ProfilePatch {
                    display_name: Some("Alice".into()),
                    ..Default::default()
                },
                300,
            )
            .await
            .unwrap();
        assert_eq!(store.profile("bob", 400).await.display_name, "bob");
    }

    #[tokio::test]
    async fn client_state_machine_round_trips_against_store() {
        // 端到端：客户端 SyncState（本机改动 → 同步往返 → 视图落地）
        let store = store();
        let mut state = SyncState::new();
        state.note_local_change("workspace:a", json!({"name": "盯盘"}), 50);
        let response = store.sync("alice", &state.build_request(), 100).await;
        let outcome = state.apply_response(&response);
        assert_eq!(outcome.acked, vec!["workspace:a"]);
        let mut view = BTreeMap::new();
        SyncState::apply_to_view(&mut view, &outcome.applied);
        assert_eq!(view["workspace:a"].rev, 1);
        assert_eq!(state.cursor, 1);

        // 第二轮：本地再改 → base_rev 已是 1，服务端接受
        state.note_local_change("workspace:a", json!({"name": "打板"}), 150);
        assert_eq!(state.build_request().pushes[0].base_rev, 1);
        let response = store.sync("alice", &state.build_request(), 200).await;
        let outcome = state.apply_response(&response);
        assert_eq!(outcome.acked, vec!["workspace:a"]);
        SyncState::apply_to_view(&mut view, &outcome.applied);
        assert_eq!(view["workspace:a"].rev, 2);
        assert_eq!(view["workspace:a"].payload, json!({"name": "打板"}));
    }

    #[tokio::test]
    async fn conflicting_client_resolves_via_core_merge_policy() {
        // 两端并发改同一键：A 已被服务端接受，B 的旧 base_rev 被拒；
        // B 用 core 的三方合并裁决（RemoteWins）后重推成功
        let store = store();
        store
            .sync(
                "alice",
                &request(0, vec![push_of("workspace:a", 0, json!({"n": "A"}))]),
                100,
            )
            .await;

        let mut b_state = SyncState::new();
        b_state.note_local_change("workspace:a", json!({"n": "B"}), 50);
        let rejected = store.sync("alice", &b_state.build_request(), 200).await;
        assert_eq!(rejected.rejected.len(), 1);

        let local: BTreeMap<String, SyncRecord> = b_state
            .outbox
            .values()
            .map(|p| (p.key.clone(), p.to_record()))
            .collect();
        let remote = records_from(&rejected);
        let base = BTreeMap::from([("workspace:a".to_string(), 0u64)]);
        let plan =
            alpha_core::account::plan_sync(&local, &remote, &base, ConflictPolicy::RemoteWins);
        assert_eq!(plan.conflicts.len(), 1);
        assert_eq!(
            plan.conflicts[0].winner,
            alpha_core::account::ConflictWinner::Remote
        );
        assert!(plan.push.is_empty(), "取服务端胜 = 不回写");

        // 若用户显式选「以我的为准」（LocalWins）则带权威 base_rev 重推
        let forced =
            alpha_core::account::plan_sync(&local, &remote, &base, ConflictPolicy::LocalWins);
        assert_eq!(forced.push.len(), 1);
        let retry = store
            .sync(
                "alice",
                &request(1, vec![push_of("workspace:a", 1, json!({"n": "B"}))]),
                300,
            )
            .await;
        assert_eq!(retry.accepted[0].rev, 2);
        assert_eq!(retry.accepted[0].payload, json!({"n": "B"}));
    }

    fn records_from(response: &SyncResponse) -> BTreeMap<String, SyncRecord> {
        response
            .rejected
            .iter()
            .filter_map(|r| r.server.clone())
            .map(|r| (r.key.clone(), r))
            .collect()
    }

    #[tokio::test]
    async fn store_survives_snapshot_round_trip() {
        let store = store();
        store.profile("alice", 100).await;
        store
            .sync(
                "alice",
                &request(0, vec![push_of("workspace:a", 0, json!({"n": 1}))]),
                200,
            )
            .await;
        let snapshot = store.snapshot("alice").expect("账户已有快照");
        let restored = AccountData::from_snapshot(&snapshot).unwrap();
        assert_eq!(restored.profile.account_id, "alice");
        assert_eq!(restored.records.len(), 1);
        assert_eq!(restored.records["workspace:a"].record.rev, 1);
        assert_eq!(restored.records["workspace:a"].touched_seq, 1);
        assert_eq!(restored.next_seq, 2);
    }

    /// 损坏快照不得把账户面卡死：解析失败即按新账户起步（内存里此后
    /// 的写入会覆盖掉那份坏快照）
    #[tokio::test]
    async fn corrupt_snapshot_falls_back_to_fresh_account() {
        struct GarbageBackend(Vec<u8>);
        #[async_trait::async_trait]
        impl StorageBackend for GarbageBackend {
            async fn store(&self, _k: &str, _v: Vec<u8>) -> alpha_core::errors::AlphaResult<()> {
                Ok(())
            }
            async fn retrieve(&self, _k: &str) -> alpha_core::errors::AlphaResult<Option<Vec<u8>>> {
                Ok(Some(self.0.clone()))
            }
            async fn delete(&self, _k: &str) -> alpha_core::errors::AlphaResult<bool> {
                Ok(false)
            }
            async fn exists(&self, _k: &str) -> alpha_core::errors::AlphaResult<bool> {
                Ok(true)
            }
            async fn list_keys(&self, _p: &str) -> alpha_core::errors::AlphaResult<Vec<String>> {
                Ok(Vec::new())
            }
            async fn clear(&self) -> alpha_core::errors::AlphaResult<()> {
                Ok(())
            }
        }

        let store = AccountStore::with_persistence(Arc::new(GarbageBackend(b"{not json".to_vec())));
        let profile = store.profile("alice", 100).await;
        assert_eq!(profile.account_id, "alice", "坏快照后档案仍可用");
        assert_eq!(profile.rev, 1);
        let response = store.sync("alice", &request(0, vec![]), 200).await;
        assert_eq!(response.cursor, 0, "空账户水位为 0");
    }

    /// 并发改动下写穿不撕裂：内存是权威，后到快照覆盖先到（重启后取到
    /// 的是其中一个完整版本，而不是半写状态）
    #[tokio::test]
    async fn concurrent_writes_keep_snapshots_parseable() {
        let store = store();
        let mut handles = Vec::new();
        for i in 0..8u32 {
            let store = store.clone();
            handles.push(tokio::spawn(async move {
                store
                    .sync(
                        "alice",
                        &request(
                            0,
                            vec![push_of(&format!("workspace:{i}"), 0, json!({"i": i}))],
                        ),
                        100 + i as i64,
                    )
                    .await
            }));
        }
        for handle in handles {
            handle.await.unwrap();
        }
        let view = store.sync("alice", &request(0, vec![]), 900).await;
        assert_eq!(view.changes.len(), 8, "并发写入全部可见");
        let snapshot = store.snapshot("alice").unwrap();
        let restored = AccountData::from_snapshot(&snapshot).expect("快照可解析");
        assert_eq!(restored.records.len(), 8);
        assert_eq!(restored.cursor(), 8, "水位与记录数一致");
    }
}
