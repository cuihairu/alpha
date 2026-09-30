//! 应用配置：默认值、校验、原子读写、容错加载
//!
//! 骨架级约定：
//! * 文件缺失 → 落默认配置并写盘（首次启动自举）；
//! * 文件损坏（半写入/手工改坏）→ 回退默认并上报 [`ConfigSource::Recovered`]，
//!   不让应用因一个配置文件起不来；
//! * 写盘走「临时文件 + rename」——同目录 rename 在 POSIX/Windows 上均为原子替换，
//!   崩溃或断电不会留下半截配置。

use crate::error::DesktopResult;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 允许的主题取值（system 跟随系统）
pub const THEMES: [&str; 3] = ["light", "dark", "system"];
/// 默认后端地址
pub const DEFAULT_API_URL: &str = "http://localhost:8080";
/// 默认观察列表
pub const DEFAULT_SYMBOLS: [&str; 3] = ["AAPL", "GOOGL", "MSFT"];

/// 应用配置（持久化到配置目录）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    /// 后端 API 根地址
    pub api_url: String,
    /// 默认观察列表
    pub symbols: Vec<String>,
    /// 主题：light / dark / system
    pub theme: String,
    /// 是否自动刷新行情
    pub auto_update: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            api_url: DEFAULT_API_URL.to_string(),
            symbols: DEFAULT_SYMBOLS.iter().map(|s| s.to_string()).collect(),
            theme: "system".to_string(),
            auto_update: true,
        }
    }
}

impl AppConfig {
    /// 结构校验：返回全部问题（空表示合法）
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();
        if self.api_url.trim().is_empty() {
            problems.push("api_url 不能为空".to_string());
        }
        if self.symbols.is_empty() {
            problems.push("symbols 至少需要一个标的".to_string());
        }
        for symbol in &self.symbols {
            if symbol.trim().is_empty() {
                problems.push("symbols 含空标的".to_string());
                break;
            }
        }
        if !THEMES.contains(&self.theme.as_str()) {
            problems.push(format!(
                "theme 取值非法: {}（允许 {})",
                self.theme,
                THEMES.join("/")
            ));
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }

    /// 校验问题列表（接线层与 IPC 返回用）
    ///
    /// 与 [`Self::validate`] 同一套规则，但返回列表而非 `Result`，避免接线层写
    /// `validate().unwrap_or_default()` 时把 `()` 当成问题列表（首次 CI 就栽在这：
    /// `Result<(), Vec<String>>` 的成功值是 `()`，`is_empty()` 直接编译不过）。
    pub fn validation_problems(&self) -> Vec<String> {
        self.validate().err().unwrap_or_default()
    }

    /// 缩进 JSON（落盘与 IPC 返回共用同一序列化口径）
    pub fn to_json(&self) -> DesktopResult<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

/// 配置加载来源
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    /// 文件不存在或为空 → 使用默认值
    Defaults,
    /// 读到配置文件
    File,
    /// 配置文件损坏 → 回退默认值（调用方应提示用户）
    Recovered,
}

impl ConfigSource {
    /// 是否发生过损坏回退
    pub fn is_recovered(self) -> bool {
        matches!(self, Self::Recovered)
    }

    /// 稳定标签（前端提示 / 测试断言）
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Defaults => "defaults",
            Self::File => "file",
            Self::Recovered => "recovered",
        }
    }
}

/// 容错加载：文件缺失/损坏均不报错，返回可用配置与来源
pub fn load_or_default(path: &Path) -> (AppConfig, ConfigSource) {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return (AppConfig::default(), ConfigSource::Defaults);
    };
    if raw.trim().is_empty() {
        return (AppConfig::default(), ConfigSource::Defaults);
    }
    match serde_json::from_str::<AppConfig>(&raw) {
        Ok(config) => (config, ConfigSource::File),
        // 解析失败视为损坏：回退默认并上报，交由界面提示
        Err(_) => (AppConfig::default(), ConfigSource::Recovered),
    }
}

/// 原子写入配置（临时文件 + rename，自动建父目录）
pub fn save(path: &Path, config: &AppConfig) -> DesktopResult<()> {
    let json = config.to_json()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// 主题偏好（L115：深浅色适配的规范型；壳层据此落 `data-theme`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemePref {
    /// 强制浅色（内容层覆盖系统深色）
    Light,
    /// 强制深色
    Dark,
    /// 跟随系统（`prefers-color-scheme` 变化实时跟随）
    System,
}

/// 解析配置里的主题字符串为规范型
///
/// 宽容口径：trim + 忽略大小写；未知取值按 [`ThemePref::System`] 处理——
/// 非法值已由 `AppConfig::validate` 单独上报，这里不再二次报错，保证壳层
/// 拿到的永远是可渲染的偏好。
pub fn theme_pref(raw: &str) -> ThemePref {
    match raw.trim().to_ascii_lowercase().as_str() {
        "light" => ThemePref::Light,
        "dark" => ThemePref::Dark,
        _ => ThemePref::System,
    }
}

/// 规范映射：偏好 × 系统是否深色 → 内容层主题（`"light"` / `"dark"`）
///
/// `System` 跟随 `system_prefers_dark`；Light/Dark 强制覆盖——这是
/// 「主题跟随系统**并覆盖既有组件**」的判断点（Tauri 1.x 原生装饰无运行期
/// `set_theme`，覆盖落在内容层 CSS 变量上）。
pub fn resolve_theme(pref: ThemePref, system_prefers_dark: bool) -> &'static str {
    match pref {
        ThemePref::Light => "light",
        ThemePref::Dark => "dark",
        ThemePref::System => {
            if system_prefers_dark {
                "dark"
            } else {
                "light"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::DesktopError;

    fn write(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, body).expect("写测试配置");
        path
    }

    #[test]
    fn defaults_are_valid_and_stable() {
        let config = AppConfig::default();
        assert_eq!(config.api_url, DEFAULT_API_URL);
        assert_eq!(config.symbols, DEFAULT_SYMBOLS);
        assert_eq!(config.theme, "system");
        assert!(config.auto_update);
        assert_eq!(config.validate(), Ok(()), "默认配置必须自洽");
        assert_eq!(AppConfig::default(), config, "默认值应可复现");
    }

    #[test]
    fn missing_file_falls_back_to_defaults() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let (config, source) = load_or_default(&tmp.path().join("config.json"));
        assert_eq!(source, ConfigSource::Defaults);
        assert!(!source.is_recovered());
        assert_eq!(config, AppConfig::default());
    }

    #[test]
    fn empty_file_counts_as_defaults() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let path = write(tmp.path(), "config.json", "   \n");
        let (_, source) = load_or_default(&path);
        assert_eq!(source, ConfigSource::Defaults);
    }

    #[test]
    fn valid_file_is_loaded_verbatim() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let config = AppConfig {
            theme: "dark".to_string(),
            symbols: vec!["600519".to_string()],
            ..AppConfig::default()
        };
        let path = tmp.path().join("config.json");
        save(&path, &config).expect("保存");

        let (loaded, source) = load_or_default(&path);
        assert_eq!(source, ConfigSource::File);
        assert_eq!(loaded, config, "读回应与写入完全一致");
    }

    #[test]
    fn corrupt_file_recovers_to_defaults() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let path = write(tmp.path(), "config.json", "{ 这不是 JSON ");
        let (config, source) = load_or_default(&path);
        assert_eq!(source, ConfigSource::Recovered);
        assert!(source.is_recovered());
        assert_eq!(config, AppConfig::default(), "损坏时回退默认");
    }

    #[test]
    fn wrong_shape_file_recovers_to_defaults() {
        let tmp = tempfile::tempdir().expect("临时目录");
        // 缺字段：serde 结构不匹配同样按损坏处理
        let path = write(tmp.path(), "config.json", r#"{"api_url":"http://x"}"#);
        let (_, source) = load_or_default(&path);
        assert_eq!(source, ConfigSource::Recovered);
    }

    #[test]
    fn save_creates_parent_dir_and_leaves_no_tmp_file() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let path = tmp.path().join("nested/deeper/config.json");
        save(&path, &AppConfig::default()).expect("保存应自建目录");
        assert!(path.is_file());
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().expect("父目录"))
            .expect("列目录")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            leftovers,
            vec!["config.json".to_string()],
            "临时文件应已被 rename 消耗，残留: {leftovers:?}"
        );
    }

    #[test]
    fn save_overwrites_previous_config() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let path = tmp.path().join("config.json");
        let first = AppConfig {
            theme: "dark".to_string(),
            ..AppConfig::default()
        };
        save(&path, &first).expect("首次保存");
        save(&path, &AppConfig::default()).expect("覆盖保存");

        let (loaded, source) = load_or_default(&path);
        assert_eq!(source, ConfigSource::File);
        assert_eq!(loaded.theme, "system");
    }

    #[test]
    fn roundtrip_through_json_preserves_fields() {
        let config = AppConfig {
            auto_update: false,
            symbols: vec!["600519".to_string(), "000001".to_string()],
            ..AppConfig::default()
        };
        let json = config.to_json().expect("序列化");
        let parsed: AppConfig = serde_json::from_str(&json).expect("反序列化");
        assert_eq!(parsed, config);
    }

    #[test]
    fn validate_flags_empty_api_url() {
        let config = AppConfig {
            api_url: "   ".to_string(),
            ..AppConfig::default()
        };
        let problems = config.validate().expect_err("应判非法");
        assert!(
            problems.iter().any(|p| p.contains("api_url")),
            "{problems:?}"
        );
    }

    #[test]
    fn validate_flags_unknown_theme() {
        let config = AppConfig {
            theme: "solarized".to_string(),
            ..AppConfig::default()
        };
        let problems = config.validate().expect_err("应判非法");
        assert!(problems.iter().any(|p| p.contains("theme")), "{problems:?}");
    }

    #[test]
    fn validate_flags_empty_symbol_list() {
        let mut config = AppConfig::default();
        config.symbols.clear();
        let problems = config.validate().expect_err("应判非法");
        assert!(
            problems.iter().any(|p| p.contains("symbols")),
            "{problems:?}"
        );
    }

    #[test]
    fn validation_problems_is_empty_for_valid_config() {
        assert!(
            AppConfig::default().validation_problems().is_empty(),
            "默认配置应无校验问题"
        );
    }

    #[test]
    fn validation_problems_mirrors_validate_err_payload() {
        let config = AppConfig {
            api_url: String::new(),
            theme: "neon".to_string(),
            ..AppConfig::default()
        };
        let expected = config.validate().expect_err("应判非法");
        assert_eq!(config.validation_problems(), expected);
        assert_eq!(
            config.validation_problems().len(),
            2,
            "接线层拿到的是问题列表本身（不是 ()）"
        );
    }

    #[test]
    fn validate_reports_all_problems_at_once() {
        let config = AppConfig {
            api_url: String::new(),
            symbols: vec!["  ".to_string()],
            theme: "neon".to_string(),
            auto_update: true,
        };
        let problems = config.validate().expect_err("应判非法");
        assert_eq!(problems.len(), 3, "全部问题应一次报出: {problems:?}");
        assert!(problems.iter().any(|p| p.contains("api_url")));
        assert!(problems.iter().any(|p| p.contains("symbols")));
        assert!(problems.iter().any(|p| p.contains("theme")));
    }

    #[test]
    fn validate_reports_empty_symbol_list_separately() {
        let mut config = AppConfig::default();
        config.symbols.clear();
        let problems = config.validate().expect_err("应判非法");
        assert_eq!(problems.len(), 1, "只报 symbols 一项: {problems:?}");
    }

    #[test]
    fn source_labels_are_stable() {
        assert_eq!(ConfigSource::Defaults.as_str(), "defaults");
        assert_eq!(ConfigSource::File.as_str(), "file");
        assert_eq!(ConfigSource::Recovered.as_str(), "recovered");
    }

    #[test]
    fn save_reports_json_error_for_unwritable_target() {
        // 目标是目录：rename 会失败，错误应映射为 io 而非静默成功
        let tmp = tempfile::tempdir().expect("临时目录");
        let path = tmp.path().to_path_buf();
        let err = save(&path, &AppConfig::default()).expect_err("写目录应失败");
        assert!(
            matches!(err, DesktopError::Io(_)),
            "错误类别应为 io，实际 {:?}",
            err.kind()
        );
    }

    #[test]
    fn theme_pref_parses_all_legal_values() {
        assert_eq!(theme_pref("light"), ThemePref::Light);
        assert_eq!(theme_pref("dark"), ThemePref::Dark);
        assert_eq!(theme_pref("system"), ThemePref::System);
        assert_eq!(theme_pref(" Dark "), ThemePref::Dark, "容忍空白与大小写");
    }

    #[test]
    fn theme_pref_defaults_unknown_to_system() {
        // 非法值由 validate 上报；解析层宽容回退，壳层永远拿到可渲染偏好
        assert_eq!(theme_pref("solarized"), ThemePref::System);
        assert_eq!(theme_pref(""), ThemePref::System);
    }

    #[test]
    fn resolve_theme_follows_system_only_for_system_pref() {
        assert_eq!(resolve_theme(ThemePref::System, true), "dark");
        assert_eq!(resolve_theme(ThemePref::System, false), "light");
        assert_eq!(
            resolve_theme(ThemePref::Dark, false),
            "dark",
            "强制深色覆盖系统浅色"
        );
        assert_eq!(
            resolve_theme(ThemePref::Light, true),
            "light",
            "强制浅色覆盖系统深色"
        );
    }
}
