//! 账户 ID / 键名校验与账户错误（纯函数，无状态）

use thiserror::Error;

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
