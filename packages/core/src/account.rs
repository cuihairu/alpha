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

mod profile;
mod sync;
mod validate;

pub use profile::AccountProfile;
pub use sync::{
    plan_sync, ConflictPolicy, ConflictWinner, RejectionReason, SyncConflict, SyncOutcome,
    SyncPlan, SyncPush, SyncRecord, SyncRejection, SyncRequest, SyncResponse, SyncState,
};
pub use validate::{
    normalize_account_id, validate_key, AccountError, LOCAL_ACCOUNT_ID, MAX_KEY_LEN,
    MAX_LOCAL_ID_LEN, MAX_NAMESPACE_LEN, MAX_PAYLOAD_BYTES, MAX_PUSHES_PER_REQUEST,
};

#[cfg(test)]
mod tests;
