//! 前端 ↔ Rust 的 IPC 请求契约
//!
//! Tauri 命令把前端 JSON 直接反序列化成这些结构，故字段名即前端契约：
//! 改名会破坏 web/dist 里的调用方，故此处补契约测试锁定（含 serde 往返）。
//! 前端现有调用见 web/app.js 的 `invoke('...')`。

use alpha_core::models::TimeRange;
use serde::{Deserialize, Serialize};

/// `analyze_symbol` 请求
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalyzeRequest {
    /// 标的代码
    pub symbol: String,
    /// 周期标签（如 "1m"/"1d"；骨架期仅透传给演示数据层，不参与计算）
    pub timeframe: String,
    /// 关注的指标名（骨架期由分析引擎统一产出，不按此裁剪）
    pub indicators: Vec<String>,
}

impl AnalyzeRequest {
    /// 构造请求
    pub fn new(symbol: impl Into<String>, timeframe: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            timeframe: timeframe.into(),
            indicators: Vec::new(),
        }
    }
}

/// `export_data` 请求
///
/// 不派生 `PartialEq`：`alpha_core::models::TimeRange` 未实现该 trait（避免为断言
/// 给共享核心模型加派生），故此处按字段断言。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportRequest {
    /// 待导出标的
    pub symbols: Vec<String>,
    /// 导出格式：csv / json
    pub format: String,
    /// 可选时间区间（骨架期不裁剪数据，仅透传回前端）
    pub date_range: Option<TimeRange>,
}

impl ExportRequest {
    /// 构造请求
    pub fn new(symbols: Vec<String>, format: impl Into<String>) -> Self {
        Self {
            symbols,
            format: format.into(),
            date_range: None,
        }
    }
}

/// `send_notification` 请求
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotificationRequest {
    /// 标的代码
    pub symbol: String,
    /// 通知标题
    pub title: String,
    /// 通知正文
    pub body: String,
    /// 级别串：info / warning / critical
    pub level: String,
}

/// `initialize_app` 应答
///
/// 放在框架层而非接线层：载荷的**形状**（前端据此渲染「已回退默认配置」提示）
/// 属于契约，`tests/wiring_contract.rs` 要能在无 GUI 环境断言它。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InitPayload {
    /// 当前生效的配置（缺失或损坏时是回退后的默认值）
    pub config: crate::config::AppConfig,
    /// 配置来源标签：`defaults` / `file` / `recovered`（见 `ConfigSource::as_str`）
    pub source: String,
    /// 配置校验问题列表（空表示无问题；前端提示但不阻断启动）
    pub validation: Vec<String>,
}

impl InitPayload {
    /// 由配置与来源组装载荷（问题列表由 `AppConfig::validation_problems` 给出）
    pub fn new(config: crate::config::AppConfig, source: crate::config::ConfigSource) -> Self {
        let validation = config.validation_problems();
        Self {
            config,
            source: source.as_str().to_string(),
            validation,
        }
    }

    /// 是否发生过损坏回退（前端据此显示醒目提示）
    pub fn is_recovered(&self) -> bool {
        self.source == crate::config::ConfigSource::Recovered.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConfigSource, DEFAULT_SYMBOLS};

    #[test]
    fn analyze_request_deserializes_frontend_payload() {
        let payload = r#"{"symbol":"600519","timeframe":"1d","indicators":["RSI","SMA20"]}"#;
        let req: AnalyzeRequest = serde_json::from_str(payload).expect("解析前端负载");
        assert_eq!(req.symbol, "600519");
        assert_eq!(req.timeframe, "1d");
        assert_eq!(req.indicators, ["RSI", "SMA20"]);
    }

    #[test]
    fn analyze_request_rejects_missing_field() {
        assert!(serde_json::from_str::<AnalyzeRequest>(r#"{"symbol":"AAPL"}"#).is_err());
    }

    #[test]
    fn analyze_request_defaults_indicators_empty() {
        let req = AnalyzeRequest::new("AAPL", "1m");
        assert!(req.indicators.is_empty());
        assert_eq!(req, AnalyzeRequest::new("AAPL", "1m"));
    }

    #[test]
    fn export_request_accepts_missing_date_range() {
        let payload = r#"{"symbols":["600519","000001"],"format":"csv"}"#;
        let req: ExportRequest = serde_json::from_str(payload).expect("解析前端负载");
        assert_eq!(req.symbols.len(), 2);
        assert_eq!(req.format, "csv");
        assert!(req.date_range.is_none(), "未传时间区间时应为 None");
    }

    #[test]
    fn export_request_parses_date_range_when_present() {
        let payload = r#"{
            "symbols":["600519"],
            "format":"json",
            "date_range":{"start":"2024-01-01T00:00:00Z","end":"2024-02-01T00:00:00Z"}
        }"#;
        let req: ExportRequest = serde_json::from_str(payload).expect("解析时间区间");
        let range = req.date_range.expect("应有时间区间");
        assert_eq!(range.start.to_rfc3339(), "2024-01-01T00:00:00+00:00");
        assert_eq!(range.duration().num_days(), 31);
    }

    #[test]
    fn export_request_roundtrips() {
        let req = ExportRequest::new(vec!["AAPL".to_string()], "csv");
        let json = serde_json::to_string(&req).expect("序列化");
        let parsed: ExportRequest = serde_json::from_str(&json).expect("反序列化");
        assert_eq!(parsed.symbols, req.symbols);
        assert_eq!(parsed.format, req.format);
        assert!(parsed.date_range.is_none());
    }

    #[test]
    fn init_payload_carries_config_source_and_problems() {
        let payload = InitPayload::new(crate::config::AppConfig::default(), ConfigSource::File);
        assert_eq!(payload.source, "file");
        assert!(payload.validation.is_empty(), "默认配置应无问题");
        assert!(!payload.is_recovered());
        assert_eq!(payload.config.symbols.len(), DEFAULT_SYMBOLS.len());
    }

    #[test]
    fn init_payload_from_defaults_is_recoverable() {
        let payload = InitPayload::new(crate::config::AppConfig::default(), ConfigSource::Defaults);
        assert_eq!(payload.source, "defaults");
        assert!(!payload.is_recovered(), "defaults 不是损坏回退");
        let recovered =
            InitPayload::new(crate::config::AppConfig::default(), ConfigSource::Recovered);
        assert!(recovered.is_recovered());
    }

    #[test]
    fn notification_request_deserializes_frontend_payload() {
        let payload =
            r#"{"symbol":"600519","title":"价格告警","body":"当前价 101","level":"critical"}"#;
        let req: NotificationRequest = serde_json::from_str(payload).expect("解析前端负载");
        assert_eq!(req.symbol, "600519");
        assert_eq!(req.title, "价格告警");
        assert_eq!(req.level, "critical");
    }

    #[test]
    fn notification_request_roundtrips() {
        let req = NotificationRequest {
            symbol: "AAPL".to_string(),
            title: "t".to_string(),
            body: "b".to_string(),
            level: "info".to_string(),
        };
        let json = serde_json::to_string(&req).expect("序列化");
        let parsed: NotificationRequest = serde_json::from_str(&json).expect("反序列化");
        assert_eq!(parsed, req);
    }

    /// 问题列表随载荷下行：接线层不再自己算，形状由框架层单测锁定
    #[test]
    fn init_payload_embeds_validation_problems() {
        let mut config = crate::config::AppConfig::default();
        config.symbols.clear();
        let payload = InitPayload::new(config, ConfigSource::Recovered);
        assert!(
            payload.validation.iter().any(|p| p.contains("symbols")),
            "{:?}",
            payload.validation
        );
    }

    /// 前端契约：字段名即 DOM 读取的键，桌面兜底壳按 config/source/validation 渲染
    #[test]
    fn init_payload_field_names_match_frontend_contract() {
        let json = serde_json::to_value(InitPayload::new(
            crate::config::AppConfig::default(),
            ConfigSource::File,
        ))
        .expect("序列化");
        for key in ["config", "source", "validation"] {
            assert!(json.get(key).is_some(), "载荷应含字段 {key}: {json}");
        }
        let payload: InitPayload = serde_json::from_value(json).expect("往返");
        assert_eq!(payload.source, "file");
    }
}
