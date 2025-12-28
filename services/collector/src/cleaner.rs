//! 数据清洗和标准化模块
//!
//! 负责对爬取的数据进行验证、清洗和标准化处理

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

use crate::sources::{KlineData, Market, RealtimeQuote, StockInfo, StockStatus};

/// 数据清洗错误
#[derive(Debug, Error)]
pub enum CleanError {
    #[error("Invalid price value: {0}")]
    InvalidPrice(f64),

    #[error("Invalid volume value: {0}")]
    InvalidVolume(u64),

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Invalid timestamp")]
    InvalidTimestamp,

    #[error("Price out of range: {0}")]
    PriceOutOfRange(f64),

    #[error("Duplicate data: {0}")]
    DuplicateData(String),
}

/// 数据质量等级
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataQuality {
    /// 高质量 - 数据完整且准确
    High,
    /// 中等质量 - 数据基本完整但可能有小问题
    Medium,
    /// 低质量 - 数据不完整或有明显问题
    Low,
    /// 无效 - 数据无法使用
    Invalid,
}

/// 数据清洗结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanResult<T> {
    /// 清洗后的数据
    pub data: Option<T>,
    /// 数据质量等级
    pub quality: DataQuality,
    /// 错误信息列表
    pub errors: Vec<String>,
    /// 警告信息列表
    pub warnings: Vec<String>,
}

impl<T> CleanResult<T> {
    /// 创建成功的清洗结果
    pub fn ok(data: T, quality: DataQuality) -> Self {
        Self {
            data: Some(data),
            quality,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// 创建有警告的清洗结果
    pub fn with_warnings(data: T, quality: DataQuality, warnings: Vec<String>) -> Self {
        Self {
            data: Some(data),
            quality,
            errors: Vec::new(),
            warnings,
        }
    }

    /// 创建失败的清洗结果
    pub fn error(errors: Vec<String>) -> Self {
        Self {
            data: None,
            quality: DataQuality::Invalid,
            errors,
            warnings: Vec::new(),
        }
    }

    /// 是否有效
    pub fn is_valid(&self) -> bool {
        self.data.is_some() && self.quality != DataQuality::Invalid
    }
}

/// 数据验证规则
#[derive(Debug, Clone)]
pub struct ValidationRules {
    /// 价格范围限制 (min, max)
    pub price_range: (f64, f64),
    /// 是否允许零价格
    pub allow_zero_price: bool,
    /// 是否允许负价格
    pub allow_negative_price: bool,
    /// 最小成交量
    pub min_volume: u64,
    /// 价格精度（小数位数）
    pub price_precision: u8,
    /// 时间戳容忍度（秒）
    pub timestamp_tolerance: i64,
}

impl Default for ValidationRules {
    fn default() -> Self {
        Self {
            price_range: (0.01, 10000.0), // 0.01元到10000元
            allow_zero_price: false,
            allow_negative_price: false,
            min_volume: 0,
            price_precision: 2,
            timestamp_tolerance: 86400, // 24小时
        }
    }
}

/// 数据清洗器
pub struct DataCleaner {
    rules: ValidationRules,
    /// 已处理的数据ID集合（用于去重）
    processed_ids: HashSet<String>,
}

impl DataCleaner {
    /// 创建新的数据清洗器
    pub fn new(rules: ValidationRules) -> Self {
        Self {
            rules,
            processed_ids: HashSet::new(),
        }
    }

    /// 使用默认规则创建
    pub fn with_default_rules() -> Self {
        Self::new(ValidationRules::default())
    }

    /// 验证价格
    fn validate_price(&self, price: f64) -> Result<(), CleanError> {
        if !self.rules.allow_negative_price && price < 0.0 {
            return Err(CleanError::InvalidPrice(price));
        }

        if !self.rules.allow_zero_price && price == 0.0 {
            return Err(CleanError::InvalidPrice(price));
        }

        if price < self.rules.price_range.0 || price > self.rules.price_range.1 {
            return Err(CleanError::PriceOutOfRange(price));
        }

        Ok(())
    }

    /// 验证成交量
    fn validate_volume(&self, volume: u64) -> Result<(), CleanError> {
        if volume < self.rules.min_volume {
            return Err(CleanError::InvalidVolume(volume));
        }
        Ok(())
    }

    /// 验证时间戳
    fn validate_timestamp(&self, timestamp: DateTime<Utc>) -> Result<(), CleanError> {
        let now = Utc::now();
        let diff = (now - timestamp).num_seconds().abs();

        if diff > self.rules.timestamp_tolerance {
            return Err(CleanError::InvalidTimestamp);
        }

        Ok(())
    }

    /// 生成数据唯一ID
    fn generate_data_id(&self, symbol: &str, timestamp: i64, data_type: &str) -> String {
        format!("{}:{}:{}", symbol, timestamp, data_type)
    }

    /// 清洗实时行情数据
    pub fn clean_realtime_quote(&mut self, mut quote: RealtimeQuote) -> CleanResult<RealtimeQuote> {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut quality = DataQuality::High;

        // 验证股票代码
        if quote.symbol.is_empty() {
            errors.push("股票代码为空".to_string());
            return CleanResult::error(errors);
        }

        // 验证股票名称
        if quote.name.is_empty() {
            warnings.push("股票名称为空".to_string());
            quality = DataQuality::Medium;
        }

        // 验证价格
        let prices = vec![
            ("当前价", quote.price),
            ("开盘价", quote.open),
            ("最高价", quote.high),
            ("最低价", quote.low),
            ("昨收价", quote.pre_close),
        ];

        for (name, price) in prices {
            if let Err(e) = self.validate_price(price) {
                errors.push(format!("{}验证失败: {}", name, e));
            }
        }

        // 验证 OHLC 逻辑关系
        if quote.high < quote.low {
            errors.push("最高价小于最低价".to_string());
        }

        if quote.price > quote.high {
            errors.push("当前价大于最高价".to_string());
        }

        if quote.price < quote.low {
            errors.push("当前价小于最低价".to_string());
        }

        // 验证成交量
        if let Err(e) = self.validate_volume(quote.volume) {
            warnings.push(format!("成交量验证警告: {}", e));
            quality = DataQuality::Medium;
        }

        // 验证时间戳
        if let Err(e) = self.validate_timestamp(quote.timestamp) {
            warnings.push(format!("时间戳验证警告: {}", e));
            quality = DataQuality::Medium;
        }

        // 验证成交额
        if quote.amount < 0.0 {
            errors.push("成交额为负数".to_string());
        }

        // 验证成交额和成交量的关系（粗略估算）
        if quote.volume > 0 && quote.amount > 0.0 {
            let avg_price = quote.amount / (quote.volume as f64 * 100.0);
            if avg_price < quote.price * 0.5 || avg_price > quote.price * 1.5 {
                warnings.push("成交额与成交量不匹配".to_string());
                quality = DataQuality::Medium;
            }
        }

        // 验证买卖价差
        if let (Some(bid), Some(ask)) = (quote.bid1, quote.ask1) {
            if bid > ask {
                errors.push("买价大于卖价".to_string());
            }
            let spread = ask - bid;
            let spread_percent = (spread / quote.price) * 100.0;
            if spread_percent > 10.0 {
                warnings.push(format!("买卖价差异常: {:.2}%", spread_percent));
                quality = DataQuality::Medium;
            }
        }

        // 验证涨跌停
        if quote.is_limit_up() && quote.price != quote.high {
            warnings.push("涨停但价格不等于最高价".to_string());
        }

        if quote.is_limit_down() && quote.price != quote.low {
            warnings.push("跌停但价格不等于最低价".to_string());
        }

        // 去重检查
        let data_id = self.generate_data_id(&quote.symbol, quote.timestamp.timestamp(), "quote");
        if self.processed_ids.contains(&data_id) {
            warnings.push(format!("重复数据: {}", data_id));
            quality = DataQuality::Low;
        } else {
            self.processed_ids.insert(data_id);
        }

        // 如果有错误，返回无效结果
        if !errors.is_empty() {
            return CleanResult::error(errors);
        }

        // 修正涨跌幅计算
        quote.calculate_change();

        CleanResult::with_warnings(quote, quality, warnings)
    }

    /// 清洗 K线数据
    pub fn clean_kline_data(&mut self, mut kline: KlineData) -> CleanResult<KlineData> {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut quality = DataQuality::High;

        // 验证股票代码
        if kline.symbol.is_empty() {
            errors.push("股票代码为空".to_string());
            return CleanResult::error(errors);
        }

        // 验证价格
        let prices = vec![
            ("开盘价", kline.open),
            ("最高价", kline.high),
            ("最低价", kline.low),
            ("收盘价", kline.close),
        ];

        for (name, price) in prices {
            if let Err(e) = self.validate_price(price) {
                errors.push(format!("{}验证失败: {}", name, e));
            }
        }

        // 验证 OHLC 逻辑关系
        if kline.high < kline.low {
            errors.push("最高价小于最低价".to_string());
        }

        if kline.close > kline.high {
            errors.push("收盘价大于最高价".to_string());
        }

        if kline.close < kline.low {
            errors.push("收盘价小于最低价".to_string());
        }

        if kline.open > kline.high {
            errors.push("开盘价大于最高价".to_string());
        }

        if kline.open < kline.low {
            errors.push("开盘价小于最低价".to_string());
        }

        // 验证成交量
        if let Err(e) = self.validate_volume(kline.volume) {
            warnings.push(format!("成交量验证警告: {}", e));
            quality = DataQuality::Medium;
        }

        // 验证成交额
        if kline.amount < 0.0 {
            errors.push("成交额为负数".to_string());
        }

        // 验证时间戳
        if kline.timestamp == 0 {
            errors.push("时间戳为0".to_string());
        }

        // 去重检查
        let data_id = self.generate_data_id(&kline.symbol, kline.timestamp, "kline");
        if self.processed_ids.contains(&data_id) {
            warnings.push(format!("重复数据: {}", data_id));
            quality = DataQuality::Low;
        } else {
            self.processed_ids.insert(data_id);
        }

        // 如果有错误，返回无效结果
        if !errors.is_empty() {
            return CleanResult::error(errors);
        }

        // 修正涨跌幅计算
        if kline.open > 0.0 {
            kline.change = kline.close - kline.open;
            kline.change_percent = (kline.change / kline.open) * 100.0;
        }

        CleanResult::with_warnings(kline, quality, warnings)
    }

    /// 批量清洗实时行情数据
    pub fn clean_realtime_quotes(&mut self, quotes: Vec<RealtimeQuote>) -> Vec<CleanResult<RealtimeQuote>> {
        quotes.into_iter()
            .map(|quote| self.clean_realtime_quote(quote))
            .collect()
    }

    /// 批量清洗 K线数据
    pub fn clean_kline_data_batch(&mut self, klines: Vec<KlineData>) -> Vec<CleanResult<KlineData>> {
        klines.into_iter()
            .map(|kline| self.clean_kline_data(kline))
            .collect()
    }

    /// 清洗股票信息数据
    pub fn clean_stock_info(&self, info: StockInfo) -> CleanResult<StockInfo> {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut quality = DataQuality::High;

        // 验证股票代码
        if info.symbol.is_empty() {
            errors.push("股票代码为空".to_string());
            return CleanResult::error(errors);
        }

        // 验证股票名称
        if info.name.is_empty() {
            warnings.push("股票名称为空".to_string());
            quality = DataQuality::Medium;
        }

        // 验证市场
        match info.stock_type {
            crate::sources::StockType::Stock => {
                // A股代码规则验证
                let code = info.symbol.chars()
                    .skip_while(|c| c.is_alphabetic())
                    .collect::<String>();

                if code.len() != 6 {
                    warnings.push("股票代码长度异常".to_string());
                    quality = DataQuality::Medium;
                }
            }
            _ => {}
        }

        // 验证状态
        if matches!(info.status, StockStatus::Delisted) {
            warnings.push("股票已退市".to_string());
        }

        if !errors.is_empty() {
            return CleanResult::error(errors);
        }

        CleanResult::with_warnings(info, quality, warnings)
    }

    /// 清除已处理数据ID缓存
    pub fn clear_processed_ids(&mut self) {
        self.processed_ids.clear();
    }

    /// 获取已处理数据数量
    pub fn processed_count(&self) -> usize {
        self.processed_ids.len()
    }

    /// 检查数据是否已处理
    pub fn is_processed(&self, symbol: &str, timestamp: i64, data_type: &str) -> bool {
        let data_id = self.generate_data_id(symbol, timestamp, data_type);
        self.processed_ids.contains(&data_id)
    }
}

/// 价格标准化器
pub struct PriceNormalizer {
    /// 价格精度（小数位数）
    precision: u8,
}

impl PriceNormalizer {
    pub fn new(precision: u8) -> Self {
        Self { precision }
    }

    /// 标准化价格
    pub fn normalize(&self, price: f64) -> f64 {
        let multiplier = 10_f64.powi(self.precision as i32);
        (price * multiplier).round() / multiplier
    }

    /// 批量标准化价格
    pub fn normalize_batch(&self, prices: &[f64]) -> Vec<f64> {
        prices.iter().map(|&p| self.normalize(p)).collect()
    }
}

/// 股票代码标准化器
pub struct SymbolNormalizer;

impl SymbolNormalizer {
    /// 标准化股票代码为小写市场前缀格式 (sh600000)
    pub fn normalize(symbol: &str) -> String {
        let lower = symbol.to_lowercase();
        if lower.starts_with("sh") || lower.starts_with("sz") || lower.starts_with("bj") {
            return lower;
        }

        // 自动判断市场
        if let Some(market) = Market::from_symbol(symbol) {
            return format!("{}{}", market.prefix(), symbol);
        }

        lower
    }

    /// 提取纯数字代码
    pub fn extract_code(symbol: &str) -> String {
        symbol.chars()
            .skip_while(|c| c.is_alphabetic())
            .collect()
    }

    /// 获取市场
    pub fn get_market(symbol: &str) -> Option<Market> {
        Market::from_symbol(symbol)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_price() {
        let cleaner = DataCleaner::with_default_rules();

        // 有效价格
        assert!(cleaner.validate_price(10.5).is_ok());

        // 无效价格（超出范围）
        assert!(cleaner.validate_price(0.001).is_err());
        assert!(cleaner.validate_price(20000.0).is_err());

        // 无效价格（负数）
        assert!(cleaner.validate_price(-1.0).is_err());
    }

    #[test]
    fn test_clean_realtime_quote() {
        let mut cleaner = DataCleaner::with_default_rules();

        let quote = RealtimeQuote {
            symbol: "sh600000".to_string(),
            name: "浦发银行".to_string(),
            price: 10.5,
            pre_close: 10.0,
            open: 10.2,
            high: 10.8,
            low: 10.1,
            volume: 1000000,
            amount: 10800000.0,
            change: 0.5,
            change_percent: 5.0,
            bid1: Some(10.5),
            ask1: Some(10.51),
            bid1_volume: Some(1000),
            ask1_volume: Some(1000),
            timestamp: Utc::now(),
            source: "test".to_string(),
        };

        let result = cleaner.clean_realtime_quote(quote);
        assert!(result.is_valid());
    }

    #[test]
    fn test_symbol_normalizer() {
        assert_eq!(SymbolNormalizer::normalize("600000"), "sh600000");
        assert_eq!(SymbolNormalizer::normalize("000001"), "sz000001");
        assert_eq!(SymbolNormalizer::normalize("sh600000"), "sh600000");
        assert_eq!(SymbolNormalizer::extract_code("sh600000"), "600000");
    }

    #[test]
    fn test_price_normalizer() {
        let normalizer = PriceNormalizer::new(2);
        assert_eq!(normalizer.normalize(10.567), 10.57);
        assert_eq!(normalizer.normalize(10.564), 10.56);
    }
}
