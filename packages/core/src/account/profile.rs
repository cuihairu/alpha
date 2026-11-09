//! 账户档案（展示身份 + 语言偏好）

use serde::{Deserialize, Serialize};

use super::validate::AccountError;

/// 账户档案（展示身份 + 语言偏好；`rev` 同样服务端权威）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountProfile {
    /// 账户 id 原样（JWT `sub` 或 [`crate::account::LOCAL_ACCOUNT_ID`]；展示/审计用，
    /// 存储键走 [`crate::account::normalize_account_id`] 的 slug）
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
