//! 应用元信息（关于面板与 IPC 通用）

use serde::{Deserialize, Serialize};

/// 产品名
pub const APP_NAME: &str = "Alpha Finance";

/// 应用信息
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppInfo {
    /// 产品名
    pub name: String,
    /// 版本（取 crate 版本）
    pub version: String,
    /// 操作系统
    pub os: String,
    /// CPU 架构
    pub arch: String,
}

/// 采集当前构建的应用信息
pub fn app_info() -> AppInfo {
    AppInfo {
        name: APP_NAME.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_reports_product_name_and_crate_version() {
        let info = app_info();
        assert_eq!(info.name, APP_NAME);
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert!(!info.version.is_empty());
    }

    #[test]
    fn info_reports_platform_targets() {
        let info = app_info();
        assert!(!info.os.is_empty());
        assert!(!info.arch.is_empty());
        assert_eq!(info.os, std::env::consts::OS);
        assert_eq!(info.arch, std::env::consts::ARCH);
    }

    #[test]
    fn info_is_serializable_for_frontend() {
        let json = serde_json::to_string(&app_info()).expect("序列化");
        let parsed: AppInfo = serde_json::from_str(&json).expect("反序列化");
        assert_eq!(parsed, app_info());
    }
}
