//! 数据压缩与列式存储优化算法（L448）：列编码选择器与三种核心编码实现。
//!
//! L445 §9 归属本项的「列编码/压缩策略」侧；行组尺寸与 zstd 级别的实测
//! 调优归 lake writer 落地项（需真实数据量）。全模块纯函数、往返可逆，
//! 单测锁定压缩语义（往返一致 + 估算口径）。

use serde::{Deserialize, Serialize};

/// 列编码方式（对应湖文件列块级选择，语义对齐 Parquet 编码族）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnEncoding {
    /// 原样定宽存储（基线）
    Plain,
    /// 字典编码：去重值表 + 索引向量（低基数列，如 symbol/exchange）
    Dictionary,
    /// 游程编码：`(值, 连续长度)` 对（排序后重复度高的列）
    RunLength,
    /// 差分编码：首值 + 差分序列（单调列，如 timestamp）
    Delta,
}

/// 列统计（编码选择的输入；lake writer 落地时由列块扫描产出）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnStats {
    pub len: u64,
    pub distinct: u64,
    /// 是否按值非降序（单调列可 Delta）
    pub sorted: bool,
}

/// 编码选择（纯函数决策矩阵，单测锁定）：
/// 单调列 → Delta（timestamp 类）；基数 ≤ 1/4 → Dictionary；
/// 其余 → Plain（Rle 需显式指定——重复结构常由调用方按 schema 知晓）。
pub fn choose_encoding(stats: &ColumnStats) -> ColumnEncoding {
    if stats.sorted && stats.len > 1 {
        ColumnEncoding::Delta
    } else if stats.distinct * 4 <= stats.len {
        ColumnEncoding::Dictionary
    } else {
        ColumnEncoding::Plain
    }
}

/// 估算压缩比（plain 字节 / 编码后字节；plain 定宽按每值 8 字节、
/// 字符串列按每值 8 字节指针口径估算——统一 8B/值保持可比）
pub fn estimate_ratio(stats: &ColumnStats, encoding: ColumnEncoding) -> f64 {
    let plain_bytes = stats.len * 8;
    let encoded_bytes = match encoding {
        ColumnEncoding::Plain => plain_bytes,
        ColumnEncoding::Delta => 8 + (stats.len.saturating_sub(1)).max(1) * 2, // 首值 + 小差分
        ColumnEncoding::Dictionary => stats.distinct * 8 + stats.len * 2,      // 表 + 索引
        ColumnEncoding::RunLength => stats.distinct * 12,                      // (值, 长度) 对
    };
    if encoded_bytes == 0 {
        return 1.0;
    }
    plain_bytes as f64 / encoded_bytes as f64
}

// ---------------------------------------------------------------------------
// Delta 编码（单调时间戳列）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeltaChunk {
    pub first: i64,
    pub deltas: Vec<i64>,
    /// 原始行数（空列 = 0；first 无意义的唯一情形）
    pub len: u64,
}

pub fn delta_encode(values: &[i64]) -> DeltaChunk {
    match values.split_first() {
        None => DeltaChunk {
            first: 0,
            deltas: Vec::new(),
            len: 0,
        },
        Some((first, rest)) => DeltaChunk {
            first: *first,
            deltas: rest
                .iter()
                .scan(*first, |prev, v| {
                    let delta = *v - *prev;
                    *prev = *v;
                    Some(delta)
                })
                .collect(),
            len: values.len() as u64,
        },
    }
}

pub fn delta_decode(chunk: &DeltaChunk) -> Vec<i64> {
    if chunk.len == 0 {
        return Vec::new();
    }
    std::iter::once(chunk.first)
        .chain(chunk.deltas.iter().scan(chunk.first, |prev, d| {
            *prev += d;
            Some(*prev)
        }))
        .collect()
}

// ---------------------------------------------------------------------------
// RLE 编码（重复游程序列）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RleRun {
    pub value: u64,
    pub run_len: u32,
}

pub fn rle_encode(values: &[u64]) -> Vec<RleRun> {
    let mut runs: Vec<RleRun> = Vec::new();
    for value in values {
        match runs.last_mut() {
            Some(run) if run.value == *value => run.run_len += 1,
            _ => runs.push(RleRun {
                value: *value,
                run_len: 1,
            }),
        }
    }
    runs
}

pub fn rle_decode(runs: &[RleRun]) -> Vec<u64> {
    runs.iter()
        .flat_map(|run| std::iter::repeat(run.value).take(run.run_len as usize))
        .collect()
}

// ---------------------------------------------------------------------------
// 字典编码（低基数文本列）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DictionaryChunk {
    /// 去重值表（出现顺序）
    pub dictionary: Vec<String>,
    /// 每行 → 字典下标
    pub indices: Vec<u32>,
}

pub fn dictionary_encode(values: &[&str]) -> DictionaryChunk {
    let mut dictionary: Vec<String> = Vec::new();
    let mut lookup: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    let mut indices = Vec::with_capacity(values.len());
    for value in values {
        let next = u32::try_from(dictionary.len()).unwrap_or(u32::MAX);
        let idx = *lookup.entry(value).or_insert(next);
        if idx as usize == dictionary.len() {
            dictionary.push((*value).to_string());
        }
        indices.push(idx);
    }
    DictionaryChunk {
        dictionary,
        indices,
    }
}

pub fn dictionary_decode(chunk: &DictionaryChunk) -> Vec<String> {
    chunk
        .indices
        .iter()
        .map(|i| chunk.dictionary[*i as usize].clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choose_encoding_decision_matrix() {
        let base = |len: u64, distinct: u64, sorted: bool| ColumnStats {
            len,
            distinct,
            sorted,
        };
        // 单调列（timestamp）优先 Delta
        assert_eq!(
            choose_encoding(&base(1000, 1000, true)),
            ColumnEncoding::Delta
        );
        // 无序低基数（symbol 列 5000 股内重复）→ Dictionary
        assert_eq!(
            choose_encoding(&base(1000, 100, false)),
            ColumnEncoding::Dictionary
        );
        // 边界：distinct*4 == len 恰好命中 Dictionary
        assert_eq!(
            choose_encoding(&base(8, 2, false)),
            ColumnEncoding::Dictionary
        );
        // 无序高基数 → Plain
        assert_eq!(
            choose_encoding(&base(1000, 900, false)),
            ColumnEncoding::Plain
        );
        // 单元素列无差分意义 → 走基数判断
        assert_eq!(choose_encoding(&base(1, 1, true)), ColumnEncoding::Plain);
    }

    #[test]
    fn delta_roundtrip_and_ratio_beats_plain_on_monotonic_timestamps() {
        // A 股 1s 快照列：5ms 精度 ms 时间戳
        let timestamps: Vec<i64> = (0..10_000).map(|i| 1_790_870_400_000 + i * 5).collect();
        let chunk = delta_encode(&timestamps);
        assert_eq!(delta_decode(&chunk), timestamps);
        // 空列往返
        assert!(delta_decode(&delta_encode(&[])).is_empty());
        // 估算：差分很小 → 压缩比远超 1
        let stats = ColumnStats {
            len: timestamps.len() as u64,
            distinct: timestamps.len() as u64,
            sorted: true,
        };
        assert!(estimate_ratio(&stats, ColumnEncoding::Delta) > 3.0);
    }

    #[test]
    fn rle_roundtrip_and_repeated_column_compression() {
        let values: Vec<u64> = std::iter::repeat(7u64)
            .take(100)
            .chain(std::iter::repeat(9u64).take(50))
            .collect();
        let runs = rle_encode(&values);
        assert_eq!(
            runs,
            vec![
                RleRun {
                    value: 7,
                    run_len: 100
                },
                RleRun {
                    value: 9,
                    run_len: 50
                }
            ]
        );
        assert_eq!(rle_decode(&runs), values);
        assert!(rle_decode(&[]).is_empty());
        // 150 值压成 2 游程：plain 1200B / 2×12B=24 → 50x
        let stats = ColumnStats {
            len: 150,
            distinct: 2,
            sorted: false,
        };
        assert!((estimate_ratio(&stats, ColumnEncoding::RunLength) - 50.0).abs() < 1e-9);
    }

    #[test]
    fn dictionary_roundtrip_and_low_cardinality_selection() {
        let symbols = ["600519", "000001", "600519", "300750", "000001", "600519"];
        let chunk = dictionary_encode(&symbols);
        assert_eq!(chunk.dictionary, ["600519", "000001", "300750"]);
        assert_eq!(chunk.indices, [0, 1, 0, 2, 1, 0]);
        let decoded: Vec<String> = dictionary_decode(&chunk);
        assert_eq!(
            decoded,
            ["600519", "000001", "600519", "300750", "000001", "600519"]
        );
        // 6 行 3 基数不满足 1/4 阈值；100 行 6 基数满足
        assert_eq!(
            choose_encoding(&ColumnStats {
                len: 6,
                distinct: 3,
                sorted: false,
            }),
            ColumnEncoding::Plain
        );
        assert_eq!(
            choose_encoding(&ColumnStats {
                len: 100,
                distinct: 6,
                sorted: false,
            }),
            ColumnEncoding::Dictionary
        );
    }
}
