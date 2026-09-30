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

#[cfg(test)]
mod tests {
    use super::*;

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
}
