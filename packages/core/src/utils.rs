//! 工具函数模块

use crate::errors::AlphaResult;
use chrono::{DateTime, Utc, Duration};

/// 时间工具函数
pub mod time {
    use super::*;

    /// 获取当前时间戳 (毫秒)
    pub fn current_timestamp_ms() -> i64 {
        Utc::now().timestamp_millis()
    }

    /// 时间戳转换为 DateTime
    pub fn timestamp_to_datetime(timestamp: i64) -> AlphaResult<DateTime<Utc>> {
        DateTime::from_timestamp_millis(timestamp)
            .ok_or_else(|| crate::errors::AlphaError::invalid_input("Invalid timestamp"))
    }

    /// 获取交易时间范围 (9:30-16:00)
    pub fn is_trading_time(dt: &DateTime<Utc>) -> bool {
        // 简化的美股交易时间判断
        let hour = dt.hour() % 24; // UTC 转换简化处理
        let minute = dt.minute();

        (hour == 14 && minute >= 30) || (hour > 14 && hour < 21) || (hour == 21 && minute == 0)
    }

    /// 获取下一个交易日
    pub fn next_trading_day(dt: &DateTime<Utc>) -> DateTime<Utc> {
        let mut next_day = *dt + Duration::days(1);

        // 简化处理：跳过周末
        while next_day.weekday().num_days_from_monday() >= 5 {
            next_day = next_day + Duration::days(1);
        }

        next_day
    }
}

/// 数值工具函数
pub mod numeric {
    /// 保留指定位数的小数
    pub fn round_to(value: f64, precision: usize) -> f64 {
        let multiplier = 10_f64.powi(precision as i32);
        (value * multiplier).round() / multiplier
    }

    /// 计算百分比变化
    pub fn percent_change(old_value: f64, new_value: f64) -> f64 {
        if old_value == 0.0 {
            return 0.0;
        }
        ((new_value - old_value) / old_value) * 100.0
    }

    /// 安全除法，避免除零错误
    pub fn safe_divide(numerator: f64, denominator: f64, default: f64) -> f64 {
        if denominator.abs() < f64::EPSILON {
            default
        } else {
            numerator / denominator
        }
    }

    /// 计算移动平均
    pub fn moving_average(values: &[f64], window: usize) -> Vec<f64> {
        if values.len() < window {
            return vec![0.0; values.len()];
        }

        let mut result = vec![0.0; values.len()];
        let mut sum: f64 = values.iter().take(window).sum();

        result[window - 1] = sum / window as f64;

        for i in window..values.len() {
            sum = sum - values[i - window] + values[i];
            result[i] = sum / window as f64;
        }

        result
    }
}

/// 字符串工具函数
pub mod string {
    /// 安全截断字符串
    pub fn safe_truncate(s: &str, max_len: usize) -> String {
        if s.len() <= max_len {
            s.to_string()
        } else {
            format!("{}...", &s[..max_len.saturating_sub(3)])
        }
    }

    /// 移除空白字符
    pub fn trim_whitespace(s: &str) -> String {
        s.chars().filter(|c| !c.is_whitespace()).collect()
    }

    /// 检查是否为有效的股票代码
    pub fn is_valid_symbol(symbol: &str) -> bool {
        !symbol.is_empty()
            && symbol.len() <= 10
            && symbol.chars().all(|c| c.is_alphanumeric() || c == '.')
    }
}

/// 数据验证工具
pub mod validation {
    use crate::models::MarketData;

    /// 验证市场数据有效性
    pub fn validate_market_data(data: &MarketData) -> AlphaResult<()> {
        if data.symbol.is_empty() {
            return Err(crate::errors::AlphaError::invalid_input("Symbol cannot be empty"));
        }

        if data.price <= 0.0 {
            return Err(crate::errors::AlphaError::invalid_input("Price must be positive"));
        }

        if data.timestamp > chrono::Utc::now() {
            return Err(crate::errors::AlphaError::invalid_input("Timestamp cannot be in the future"));
        }

        Ok(())
    }

    /// 验证价格范围合理性
    pub fn validate_price_range(price: f64, symbol: &str) -> AlphaResult<()> {
        // 基本的价格合理性检查
        if price <= 0.0 {
            return Err(crate::errors::AlphaError::invalid_input("Price must be positive"));
        }

        if price > 1_000_000.0 {
            return Err(crate::errors::AlphaError::invalid_input("Price seems unreasonably high"));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_numeric_utils() {
        assert_eq!(numeric::round_to(3.14159, 2), 3.14);
        assert_eq!(numeric::percent_change(100.0, 110.0), 10.0);
        assert_eq!(numeric::safe_divide(10.0, 2.0, 0.0), 5.0);
        assert_eq!(numeric::safe_divide(10.0, 0.0, -1.0), -1.0);
    }

    #[test]
    fn test_string_utils() {
        assert_eq!(string::safe_truncate("hello world", 5), "he...");
        assert_eq!(string::trim_whitespace(" hello  world "), "helloworld");
        assert!(string::is_valid_symbol("AAPL"));
        assert!(!string::is_valid_symbol(""));
        assert!(!string::is_valid_symbol("TOO_LONG_SYMBOL_12345"));
    }

    #[test]
    fn test_time_utils() {
        let now = Utc::now();
        let timestamp = time::current_timestamp_ms();
        let dt = time::timestamp_to_datetime(timestamp).unwrap();

        // 时间戳应该在合理范围内
        assert!((now - dt).abs() < Duration::seconds(1));
    }

    #[test]
    fn test_validation() {
        use chrono::Utc;

        let valid_data = MarketData {
            symbol: "AAPL".to_string(),
            timestamp: Utc::now(),
            price: 150.0,
            volume: 1000,
            bid: Some(149.5),
            ask: Some(150.5),
            open: Some(149.0),
            high: Some(151.0),
            low: Some(148.5),
        };

        assert!(validation::validate_market_data(&valid_data).is_ok());
        assert!(validation::validate_price_range(150.0, "AAPL").is_ok());
        assert!(validation::validate_price_range(0.0, "AAPL").is_err());
    }
}