//! 移动端核心状态与 FFI 导出（L118）
//!
//! [`MobileCore`] 是平台壳经 uniffi 拿到的唯一对象：观察列表 + 配置槽 +
//! 分析引擎 + 演示数据源。FFI 载荷为 JSON 字符串（字段契约由单测锁定，见
//! docs/mobile-core-architecture.md §4）；错误经 [`MobileError`]（`uniffi::Error`）
//! 以类型化异常跨桥（§5）。观察列表是核心库唯一的业务判断：列表外标的统一
//! `InvalidSymbol`（§7），平台壳不重复实现。

use alpha_core::analytics::AnalysisEngine;
use alpha_core::errors::AlphaError;
use alpha_core::models::{AnalysisResult, MarketData};
use std::future::Future;
use std::sync::Arc;

/// 移动端 FFI 错误（跨桥为类型化异常；分类在 Rust 侧判定，平台壳只展示）
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum MobileError {
    /// 观察列表外的标的（含空 symbol、空列表退化——能力边界统一在核心库）
    #[error("标的 {symbol} 不在观察列表")]
    InvalidSymbol {
        /// 被拒绝的标的代码
        symbol: String,
    },
    /// 其余核心错误（`AlphaError` 等经 Display 全文透传，不猜不缩）
    ///
    /// 字段名不能叫 `message`：uniffi 生成的 Kotlin 会给错误类自动加
    /// `override val message` 展示属性，构造属性与之同名即冲突（K1/K2 都
    /// 编不过）——L301 实证，字段一律避开 `message`。
    #[error("{detail}")]
    Failed {
        /// 错误全文（`Display` 输出）
        detail: String,
    },
}

impl From<AlphaError> for MobileError {
    fn from(err: AlphaError) -> Self {
        MobileError::Failed {
            detail: err.to_string(),
        }
    }
}

/// 移动端核心状态（uniffi Object，`Arc` 共享、可跨线程）
///
/// 纯数据 + 引擎，无运行期句柄——`analyze` 按调用建 current-thread 运行时驱动
/// alpha-core 的 future（其 `analyze_symbol` 形为 async、体为零 await 纯计算），
/// 故本对象 `Send + Sync`，uniffi 侧多线程调用安全（压测属真机边界）。
#[derive(Debug, uniffi::Object)]
pub struct MobileCore {
    /// 观察列表（能力边界，见模块文档）
    symbols: Vec<String>,
    /// 后端地址配置槽（骨架期只随 `status_json` 下行；探测/同步归后续 TODO）
    api_url: String,
    /// 分享给平台壳的分析引擎（与 web/desktop 同一份 alpha-core 计算）
    engine: AnalysisEngine,
}

/// 驱动一个 future 到完成（仅骨架期使用：见模块文档与 §6 线程模型；
/// 复用 `Runtime` 的优化留到真机有实测数据后）
fn block_on<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("创建 current-thread 运行时")
        .block_on(future)
}

impl MobileCore {
    /// 观察列表
    pub fn symbols(&self) -> &[String] {
        &self.symbols
    }

    /// 后端地址配置槽
    pub fn api_url(&self) -> &str {
        &self.api_url
    }

    /// 观察列表判定（核心库唯一业务判断；列表外一律 `InvalidSymbol`）
    fn ensure_watched(&self, symbol: &str) -> Result<(), MobileError> {
        if self.symbols.iter().any(|watched| watched == symbol) {
            Ok(())
        } else {
            Err(MobileError::InvalidSymbol {
                symbol: symbol.to_string(),
            })
        }
    }

    /// 快照行情（观察列表内；演示数据源，见 `market` 模块）
    pub fn quote(&self, symbol: &str) -> Result<MarketData, MobileError> {
        self.ensure_watched(symbol)?;
        Ok(crate::market::synthetic_quote(symbol))
    }

    /// 技术分析（观察列表内；走 alpha-core `AnalysisEngine`）
    pub fn analyze(&self, symbol: &str) -> Result<AnalysisResult, MobileError> {
        self.ensure_watched(symbol)?;
        let series = crate::market::synthetic_series(symbol, crate::market::DEFAULT_BARS);
        Ok(block_on(self.engine.analyze_symbol(&series, None))?)
    }
}

/// FFI 导出面：构造器 + 三个 JSON 载荷方法（`setup_scaffolding!` 在 lib.rs，
/// proc-macro-only、无 UDL 副本）
#[uniffi::export]
impl MobileCore {
    /// 构造核心状态（uniffi 主构造器，Kotlin/Swift 侧为 `MobileCore(...)`）
    #[uniffi::constructor]
    pub fn new(symbols: Vec<String>, api_url: String) -> Arc<Self> {
        Arc::new(Self {
            symbols,
            api_url,
            engine: AnalysisEngine::new(),
        })
    }

    /// 快照行情的 JSON 载荷（`MarketData` serde 字段即契约）
    ///
    /// FFI 参数用 owned `String`：uniffi 0.25 的 proc-macro 路径不支持 `&str`
    /// 提升（`LiftRef` 只对已导出类型与 owned 内建类型实现）
    pub fn quote_json(&self, symbol: String) -> Result<String, MobileError> {
        serde_json::to_string(&self.quote(&symbol)?).map_err(|e| MobileError::Failed {
            detail: e.to_string(),
        })
    }

    /// 分析结果的 JSON 载荷（`AnalysisResult` serde 字段即契约）
    pub fn analyze_json(&self, symbol: String) -> Result<String, MobileError> {
        serde_json::to_string(&self.analyze(&symbol)?).map_err(|e| MobileError::Failed {
            detail: e.to_string(),
        })
    }

    /// 状态快照：版本 + 观察列表 + 配置槽（平台壳启动时读一次）
    pub fn status_json(&self) -> String {
        serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "symbols": self.symbols,
            "api_url": self.api_url,
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core() -> Arc<MobileCore> {
        MobileCore::new(
            vec!["600519".to_string(), "000001".to_string()],
            "http://localhost:8080".to_string(),
        )
    }

    #[test]
    fn new_stores_watch_list_and_api_url() {
        let core = core();
        assert_eq!(core.symbols(), ["600519", "000001"]);
        assert_eq!(core.api_url(), "http://localhost:8080");
    }

    #[test]
    fn quote_within_watch_list_is_deterministic() {
        let core = core();
        let a = core.quote("600519").expect("列表内");
        let b = core.quote("600519").expect("列表内");
        assert_eq!(a.price, b.price, "演示行情与调用时刻无关");
        assert_eq!(a.symbol, "600519");
    }

    #[test]
    fn quote_outside_watch_list_is_invalid_symbol() {
        let core = core();
        let err = core.quote("300750").expect_err("列表外应拒绝");
        match &err {
            MobileError::InvalidSymbol { symbol } => assert_eq!(symbol, "300750"),
            other => panic!("应为 InvalidSymbol，实际 {other:?}"),
        }
        // 错误文案带标的（平台壳展示即用）
        assert!(err.to_string().contains("300750"));
    }

    /// 空 symbol 与空观察列表的退化：一律 InvalidSymbol，不 panic 不编造
    #[test]
    fn empty_cases_degrade_to_invalid_symbol() {
        let empty = MobileCore::new(Vec::new(), "http://localhost:8080".to_string());
        assert!(matches!(
            empty.quote("600519"),
            Err(MobileError::InvalidSymbol { .. })
        ));
        assert!(matches!(
            core().quote(""),
            Err(MobileError::InvalidSymbol { .. })
        ));
    }

    #[test]
    fn analyze_within_watch_list_returns_indicators() {
        let core = core();
        let result = core.analyze("600519").expect("列表内");
        assert_eq!(result.symbol, "600519");
        assert!(!result.indicators.is_empty(), "应有指标（RSI/SMA/MACD）");
    }

    #[test]
    fn analyze_outside_watch_list_is_invalid_symbol() {
        let core = core();
        assert!(matches!(
            core.analyze("300750"),
            Err(MobileError::InvalidSymbol { .. })
        ));
    }

    /// FFI 载荷字段契约：quote_json = MarketData serde 字段（§4）
    #[test]
    fn quote_json_keeps_market_data_field_contract() {
        let core = core();
        let value: serde_json::Value =
            serde_json::from_str(&core.quote_json("600519".to_string()).expect("JSON"))
                .expect("解析");
        for key in [
            "symbol",
            "timestamp",
            "price",
            "volume",
            "bid",
            "ask",
            "open",
        ] {
            assert!(value.get(key).is_some(), "quote_json 缺字段 {key}: {value}");
        }
        assert_eq!(value["symbol"], "600519");
    }

    /// FFI 载荷字段契约：analyze_json = AnalysisResult serde 字段（§4）
    #[test]
    fn analyze_json_keeps_analysis_result_field_contract() {
        let core = core();
        let value: serde_json::Value =
            serde_json::from_str(&core.analyze_json("600519".to_string()).expect("JSON"))
                .expect("解析");
        for key in [
            "symbol",
            "indicators",
            "recommendation",
            "confidence",
            "risk_metrics",
        ] {
            assert!(
                value.get(key).is_some(),
                "analyze_json 缺字段 {key}: {value}"
            );
        }
        let indicators = value["indicators"].as_array().expect("指标数组");
        assert!(!indicators.is_empty());
        assert!(indicators[0].get("name").is_some() && indicators[0].get("values").is_some());
    }

    #[test]
    fn status_json_reports_config_slot() {
        let core = core();
        let value: serde_json::Value =
            serde_json::from_str(&core.status_json()).expect("解析为 JSON");
        assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(value["api_url"], "http://localhost:8080");
        assert_eq!(value["symbols"], serde_json::json!(["600519", "000001"]));
    }

    /// 列表外标的在 FFI 侧同样抛 InvalidSymbol（不因走 JSON 方法而绕过边界）
    #[test]
    fn json_methods_enforce_watch_list_too() {
        let core = core();
        assert!(matches!(
            core.quote_json("300750".to_string()),
            Err(MobileError::InvalidSymbol { .. })
        ));
        assert!(matches!(
            core.analyze_json("300750".to_string()),
            Err(MobileError::InvalidSymbol { .. })
        ));
    }

    /// From<AlphaError>：分类不变、Display 全文进 Failed.detail（§5）
    #[test]
    fn alpha_error_maps_to_failed_with_display_text() {
        let source = AlphaError::invalid_input("bad input");
        let mapped: MobileError = source.clone().into();
        assert_eq!(mapped.to_string(), source.to_string());
        assert!(matches!(mapped, MobileError::Failed { .. }));
    }
}
