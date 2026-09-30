//! 框架层统一错误类型
//!
//! 命令边界（`#[tauri::command]`）统一映射为字符串回前端，故实现 `Display`；
//! 内部按来源分类保留（IO / JSON / CSV / 核心库 / 输入非法），便于测试断言
//! 错误类别而不只是错误文案。

use alpha_core::errors::AlphaError;

/// 框架层结果别名
pub type DesktopResult<T> = Result<T, DesktopError>;

/// 桌面框架层错误
#[derive(Debug)]
pub enum DesktopError {
    /// 文件系统访问失败（目录创建、配置/告警读写、导出落盘）
    Io(std::io::Error),
    /// JSON 序列化/反序列化失败
    Json(serde_json::Error),
    /// CSV 写入失败
    Csv(csv::Error),
    /// alpha-core 计算/数据错误
    Core(AlphaError),
    /// 调用方输入非法（未知导出格式、空键、非法告警方向等）
    InvalidInput(String),
}

impl DesktopError {
    /// 分类标签（测试与前端提示用，不含可变文案）
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Io(_) => "io",
            Self::Json(_) => "json",
            Self::Csv(_) => "csv",
            Self::Core(_) => "core",
            Self::InvalidInput(_) => "invalid_input",
        }
    }
}

impl std::fmt::Display for DesktopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "文件操作失败: {e}"),
            Self::Json(e) => write!(f, "JSON 处理失败: {e}"),
            Self::Csv(e) => write!(f, "CSV 写入失败: {e}"),
            Self::Core(e) => write!(f, "核心计算失败: {e}"),
            Self::InvalidInput(msg) => write!(f, "输入非法: {msg}"),
        }
    }
}

impl std::error::Error for DesktopError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            Self::Csv(e) => Some(e),
            Self::Core(e) => Some(e),
            Self::InvalidInput(_) => None,
        }
    }
}

impl From<std::io::Error> for DesktopError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for DesktopError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

impl From<csv::Error> for DesktopError {
    fn from(e: csv::Error) -> Self {
        Self::Csv(e)
    }
}

impl From<AlphaError> for DesktopError {
    fn from(e: AlphaError) -> Self {
        Self::Core(e)
    }
}

impl From<DesktopError> for String {
    fn from(e: DesktopError) -> Self {
        e.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_labels_are_stable() {
        assert_eq!(
            DesktopError::InvalidInput("x".to_string()).kind(),
            "invalid_input"
        );
        assert_eq!(
            DesktopError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "nope")).kind(),
            "io"
        );
        assert_eq!(
            DesktopError::Core(AlphaError::not_found("x")).kind(),
            "core"
        );
    }

    #[test]
    fn invalid_input_displays_reason() {
        let err = DesktopError::InvalidInput("未知导出格式: xlsx".to_string());
        let msg = err.to_string();
        assert!(msg.contains("未知导出格式"), "实际: {msg}");
        assert!(std::error::Error::source(&err).is_none());
    }

    #[test]
    fn core_error_exposes_source() {
        let err = DesktopError::Core(AlphaError::network("断连"));
        assert!(std::error::Error::source(&err).is_some());
        assert!(err.to_string().contains("断连"));
    }

    #[test]
    fn converts_to_string_for_command_boundary() {
        let s: String = DesktopError::InvalidInput("坏格式".to_string()).into();
        assert!(s.contains("坏格式"));
    }
}
