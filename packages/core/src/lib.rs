//! Alpha Finance 跨平台核心库
//!
//! 提供所有平台共享的数据模型、算法和工具函数

#![cfg_attr(not(feature = "std"), no_std)]

pub mod models;
pub mod indicators;
pub mod analytics;
pub mod utils;
pub mod errors;

// 重新导出主要类型
pub use models::*;
pub use indicators::TechnicalIndicators;
pub use analytics::AnalysisEngine;
pub use errors::*;

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