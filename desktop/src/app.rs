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
    /// 反向域名标识符（Tauri 1.x 的 `tauri.bundle.identifier`）
    ///
    /// 只能从 Tauri 运行时读到，框架层零 Tauri 依赖，故由接线层注入。
    pub identifier: String,
}

/// 采集当前构建的应用信息；`identifier` 由接线层从 `AppHandle` 的配置注入
pub fn app_info(identifier: impl Into<String>) -> AppInfo {
    AppInfo {
        name: APP_NAME.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        identifier: identifier.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_reports_product_name_and_crate_version() {
        let info = app_info("");
        assert_eq!(info.name, APP_NAME);
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert!(!info.version.is_empty());
    }

    #[test]
    fn info_reports_platform_targets() {
        let info = app_info("");
        assert!(!info.os.is_empty());
        assert!(!info.arch.is_empty());
        assert_eq!(info.os, std::env::consts::OS);
        assert_eq!(info.arch, std::env::consts::ARCH);
    }

    #[test]
    fn info_is_serializable_for_frontend() {
        let info = app_info("com.alpha.finance");
        let json = serde_json::to_string(&info).expect("序列化");
        let parsed: AppInfo = serde_json::from_str(&json).expect("反序列化");
        assert_eq!(parsed, info);
        assert_eq!(parsed.identifier, "com.alpha.finance");
    }

    /// 标识符只能从 Tauri 运行时读到，框架层不该自己编——注入什么就原样带出
    #[test]
    fn info_carries_injected_identifier_verbatim() {
        assert_eq!(app_info("a.b.c").identifier, "a.b.c");
        assert_eq!(app_info(String::from("x.y")).identifier, "x.y");
    }
}
