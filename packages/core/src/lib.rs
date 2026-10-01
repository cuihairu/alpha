//! Alpha Finance 跨平台核心库
//!
//! 提供所有平台共享的数据模型、算法和工具函数

#![cfg_attr(not(feature = "std"), no_std)]
#![deny(unsafe_code)]

pub mod alloc_tracking;
pub mod analytics;
pub mod backtest;
pub mod diagnosis;
pub mod errors;
pub mod hybrid_cache;
pub mod indicators;
pub mod memory;
pub mod models;
pub mod optimize;
pub mod parallel;
pub mod platform;
pub mod risk;
#[cfg(feature = "std")]
pub mod safety_audit;
pub mod simd;
pub mod streaming;
pub mod sync;
pub mod utils;

// 重新导出主要类型
pub use analytics::AnalysisEngine;
pub use errors::*;
pub use indicators::TechnicalIndicators;
pub use memory::*;
pub use models::*;
pub use sync::{SyncEngine, SyncError, SyncOutcome};
pub use utils::numeric;
pub use utils::string;
pub use utils::time;
pub use utils::validation;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_functionality() {
        // 基础功能测试
        let indicator = TechnicalIndicators::new();
        let data = vec![100.0, 101.0, 102.0, 103.0, 104.0];

        let sma = indicator.calculate_sma(&data, 3);
        assert!(!sma.is_empty());
    }
}
