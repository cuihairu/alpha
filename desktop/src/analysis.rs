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

/// 按 IPC 请求跑分析（`analyze_symbol` 命令的实现体）
///
/// 存在的理由：请求字段的语义校验（空标的）属于业务口径，留在接线层就要在
/// 两处（命令 + 框架层）各写一遍；下沉后接线层只做「取 state → 委派」。
/// `timeframe`/`indicators` 在骨架期不参与计算（演示行情口径，见 market 模块），
/// 但保留在 DTO 里以锁定前端契约。
pub async fn analyze_request(
    engine: &AnalysisEngine,
    request: &crate::ipc::AnalyzeRequest,
) -> AlphaResult<AnalysisResult> {
    analyze(engine, &request.symbol).await
}

/// 快照请求校验：空标的列表返回 Err 而非静默返回空数组
pub fn quotes_request(symbols: &[String]) -> AlphaResult<Vec<MarketData>> {
    if symbols.is_empty() {
        return Err(alpha_core::errors::AlphaError::invalid_input(
            "symbols 不能为空",
        ));
    }
    Ok(quotes(symbols))
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

    #[tokio::test]
    async fn analyze_request_delegates_to_symbol_analysis() {
        let request = crate::ipc::AnalyzeRequest::new("600519", "1d");
        let result = analyze_request(&engine(), &request)
            .await
            .expect("分析应成功");
        assert_eq!(result.symbol, "600519");
    }

    #[tokio::test]
    async fn analyze_request_rejects_blank_symbol_in_request() {
        let mut request = crate::ipc::AnalyzeRequest::new("  ", "1d");
        request.indicators.push("RSI".to_string());
        let err = analyze_request(&engine(), &request)
            .await
            .expect_err("空白标的应报错");
        assert!(matches!(err, AlphaError::InvalidInput(_)), "实际 {err:?}");
    }

    #[test]
    fn quotes_request_rejects_empty_symbol_list() {
        let err = quotes_request(&[]).expect_err("空标的列表应报错");
        assert!(matches!(err, AlphaError::InvalidInput(_)), "实际 {err:?}");
        assert!(err.to_string().contains("symbols"), "错误应点名入参: {err}");
    }

    #[test]
    fn quotes_request_preserves_order() {
        let symbols = vec!["600519".to_string(), "000001".to_string()];
        let got = quotes_request(&symbols).expect("应成功");
        let order: Vec<&str> = got.iter().map(|q| q.symbol.as_str()).collect();
        assert_eq!(order, ["600519", "000001"]);
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
