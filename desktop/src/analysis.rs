//! 分析编排：演示行情 → alpha-core 分析引擎
//!
//! 计算全部委托 `alpha_core::analytics::AnalysisEngine`（L0 共享核心，桌面端不重复实现
//! 指标），本模块只负责「取哪段数据、报什么错」，因此可在无 GUI/无网络环境单测。

use crate::market;
use alpha_core::analytics::AnalysisEngine;
use alpha_core::errors::AlphaResult;
use alpha_core::models::{AnalysisResult, MarketData};

/// 对单个标的跑完整技术分析
pub async fn analyze(engine: &AnalysisEngine, symbol: &str) -> AlphaResult<AnalysisResult> {
    if symbol.trim().is_empty() {
        return Err(alpha_core::errors::AlphaError::invalid_input(
            "标的代码不能为空",
        ));
    }
    let series = market::synthetic_series(symbol, market::DEFAULT_BARS);
    engine.analyze_symbol(&series, None).await
}

/// 批量快照行情（保持请求顺序）
pub fn quotes(symbols: &[String]) -> Vec<MarketData> {
    symbols.iter().map(|s| market::synthetic_quote(s)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alpha_core::errors::AlphaError;

    fn engine() -> AnalysisEngine {
        AnalysisEngine::new()
    }

    #[tokio::test]
    async fn analyze_returns_result_for_requested_symbol() {
        let result = analyze(&engine(), "600519").await.expect("分析应成功");
        assert_eq!(result.symbol, "600519");
        assert!(!result.indicators.is_empty(), "应产出技术指标");
    }

    #[tokio::test]
    async fn analyze_is_deterministic_for_same_symbol() {
        let a = analyze(&engine(), "AAPL").await.expect("分析应成功");
        let b = analyze(&engine(), "AAPL").await.expect("分析应成功");
        assert_eq!(a.confidence, b.confidence, "同输入应同置信度");
        assert_eq!(a.recommendation, b.recommendation);
    }

    #[tokio::test]
    async fn analyze_rejects_blank_symbol() {
        let err = analyze(&engine(), "  ").await.expect_err("空白标的应报错");
        assert!(matches!(err, AlphaError::InvalidInput(_)), "实际 {err:?}");
    }

    #[tokio::test]
    async fn analyze_supports_non_ascii_symbols() {
        let result = analyze(&engine(), "000001").await.expect("A 股代码应支持");
        assert_eq!(result.symbol, "000001");
    }

    #[test]
    fn quotes_preserve_request_order_and_count() {
        let symbols = vec![
            "600519".to_string(),
            "000001".to_string(),
            "300750".to_string(),
        ];
        let quotes = quotes(&symbols);
        assert_eq!(quotes.len(), 3);
        let got: Vec<&str> = quotes.iter().map(|q| q.symbol.as_str()).collect();
        assert_eq!(got, ["600519", "000001", "300750"]);
    }

    #[test]
    fn quotes_of_empty_list_is_empty() {
        assert!(quotes(&[]).is_empty());
    }

    #[test]
    fn engine_result_is_serializable_for_ipc() {
        // 命令直接回传 AnalysisResult：确保它能被前端消费
        let result = analyze_blocking(&engine(), "AAPL");
        let json = serde_json::to_string(&result).expect("序列化分析结果");
        assert!(json.contains("\"symbol\""), "应有 symbol 字段");
        assert!(json.contains("\"indicators\""));
    }

    fn analyze_blocking(engine: &AnalysisEngine, symbol: &str) -> AnalysisResult {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("运行时")
            .block_on(analyze(engine, symbol))
            .expect("分析应成功")
    }
}
