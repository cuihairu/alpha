//! 研究数据集与实验登记契约（architecture-review §3.5）
//!
//! 回测/研究引用的是**某一版数据**而非当前表：`DatasetDescriptor` 描述
//! 一份不可变数据集（`dataset://{domain}/{granularity}/{version}` 概念），
//! `ExperimentRecord` 记录「哪份策略代码 × 哪版数据 × 什么参数 × 什么种子
//! → 什么结果」——回答「为什么昨天 1.32 今天 1.17」。
//!
//! 本模块只定义契约（纯 serde，无存储依赖）；登记表实现见
//! `packages/storage/src/dataset_registry.rs`（包级 API 面归 P3）。
//!
//! 字段口径以 review §3.5 为准（克制：不预加未登记字段）。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 数据集描述符：一份不可变数据集的登记条目
///
/// （`serde(default)` 加性演进：老 JSON 少字段可读，新字段后加不破老登记）
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DatasetDescriptor {
    /// 数据集 ID（如 `kline` / `quotes`；与 version 组合定位一份不可变数据）
    #[serde(default)]
    pub dataset_id: String,
    /// 领域（如 `kline` / `indicators` / `snapshot`）
    #[serde(default)]
    pub domain: String,
    /// 粒度（如 `1m` / `1d` / `tick`）
    #[serde(default)]
    pub granularity: String,
    /// 版本（同一 dataset_id 的不同版本并存，回测引用具体版本）
    #[serde(default)]
    pub version: String,
    /// 数据来源（eastmoney / sina / …）
    #[serde(default)]
    pub source: String,
    /// 覆盖时间窗起点（含）
    #[serde(default)]
    pub time_start: Option<DateTime<Utc>>,
    /// 覆盖时间窗终点（含）
    #[serde(default)]
    pub time_end: Option<DateTime<Utc>>,
    /// 覆盖标的（空 = 全市场语义，由消费方按 domain 约定解释）
    #[serde(default)]
    pub symbols: Vec<String>,
    /// 数据 schema 版本（表结构/字段口径，区别于数据版本）
    #[serde(default)]
    pub schema_version: u32,
    /// 内容校验和（生成时固定，消费方可验证引用完整性）
    #[serde(default)]
    pub checksum: String,
    /// 登记时刻
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
}

/// 实验记录：策略代码 × 数据集版本 × 参数 × 种子 → 结果
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ExperimentRecord {
    /// 实验 ID（唯一）
    #[serde(default)]
    pub experiment_id: String,
    /// 引用的数据集 ID
    #[serde(default)]
    pub dataset_id: String,
    /// 引用的数据集版本（不可变引用的另一半）
    #[serde(default)]
    pub dataset_version: String,
    /// 策略名
    #[serde(default)]
    pub strategy: String,
    /// 策略代码版本（commit/构建号——可复现的代码侧锚点）
    #[serde(default)]
    pub code_version: String,
    /// 策略参数（键序稳定用 BTreeMap，序列化可 diff）
    #[serde(default)]
    pub parameters: BTreeMap<String, serde_json::Value>,
    /// 随机种子（None = 策略无随机性）
    #[serde(default)]
    pub seed: Option<u64>,
    /// 结果（自由结构：指标/曲线引用/产物指针，由策略域自行约定）
    #[serde(default)]
    pub result: serde_json::Value,
    /// 登记时刻
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_dataset() -> DatasetDescriptor {
        DatasetDescriptor {
            dataset_id: "kline".into(),
            domain: "kline".into(),
            granularity: "1d".into(),
            version: "2026.10.07-a".into(),
            source: "eastmoney".into(),
            time_start: Some(
                DateTime::parse_from_rfc3339("2020-01-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            time_end: Some(
                DateTime::parse_from_rfc3339("2025-12-31T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
            symbols: vec!["600519".into(), "000001".into()],
            schema_version: 1,
            checksum: "sha256:abcd".into(),
            created_at: Some(Utc::now()),
        }
    }

    /// serde roundtrip：字段一个不丢
    #[test]
    fn dataset_roundtrip_preserves_fields() {
        let original = sample_dataset();
        let json = serde_json::to_string(&original).unwrap();
        let decoded: DatasetDescriptor = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, original);
    }

    /// 加性演进：老登记 JSON 缺字段 → default 补齐，解码不破
    #[test]
    fn dataset_decodes_older_json_with_missing_fields() {
        let older: DatasetDescriptor =
            serde_json::from_str(r#"{"dataset_id": "kline", "version": "v1"}"#).unwrap();
        assert_eq!(older.dataset_id, "kline");
        assert_eq!(older.version, "v1");
        assert_eq!(older.symbols, Vec::<String>::new());
        assert_eq!(older.schema_version, 0);
        assert!(older.created_at.is_none());
    }

    /// 实验记录 roundtrip：参数键序稳定（BTreeMap），seed None 语义保留
    #[test]
    fn experiment_roundtrip_preserves_parameters_and_seed() {
        let mut parameters = BTreeMap::new();
        parameters.insert("window".to_string(), serde_json::json!(20));
        parameters.insert("threshold".to_string(), serde_json::json!(1.5));
        let original = ExperimentRecord {
            experiment_id: "exp-001".into(),
            dataset_id: "kline".into(),
            dataset_version: "2026.10.07-a".into(),
            strategy: "sma-cross".into(),
            code_version: "934003c".into(),
            parameters,
            seed: None,
            result: serde_json::json!({"sharpe": 1.32}),
            created_at: Some(Utc::now()),
        };
        let json = serde_json::to_string(&original).unwrap();
        let decoded: ExperimentRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, original);
        assert!(decoded.seed.is_none());
    }
}
