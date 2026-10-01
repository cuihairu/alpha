//! 智能数据分区策略（L447）：L445 湖布局 §3 的多维扩展与 compaction 计划。
//!
//! 三维取舍（「智能」的核心是维度到物理目录的映射规则）：
//! - 时间 → `trade_date=`（A 股 Asia/Shanghai 日切，基数 ~250/年，目录分区）
//! - 交易所 → `exchange=`（基数 3：SSE/SZSE/BSE，目录分区，裁剪收益直接）
//! - 股票 → symbol 基数 5000+，目录分区会爆炸成海量小目录；默认仅文件内
//!   聚簇排序（L445 §3），可选 hash 桶（`bucket=`，桶数远小于股票数）。
//!
//! 全模块纯函数、零 IO：物理写盘与 ListingTable 注册归 lake writer 落地项
//! （L445 §9 边界）。

use chrono::TimeZone;
use serde::Serialize;

/// symbol 维度策略：聚簇排序（默认）或 hash 桶目录
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolPartition {
    /// 不做目录分区——文件内按 symbol 排序供谓词下推（L445 §3 既定）
    ClusterSorted,
    /// symbol hash 桶目录：`bucket={n:04}`；桶数应远小于股票总数
    HashBuckets(u32),
}

/// 分区方案（时间维固定 trade_date；交易所/symbol 维按需叠加）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartitionScheme {
    /// `{table}/trade_date=YYYY-MM-DD/`（L445 §3 骨架原样）
    DateOnly,
    /// `{table}/exchange={ex}/trade_date=YYYY-MM-DD/`
    DateExchange,
    /// `{table}/exchange={ex}/trade_date=YYYY-MM-DD/bucket={n}/`
    DateExchangeSymbolBuckets(u32),
}

impl PartitionScheme {
    pub fn symbol_partition(&self) -> SymbolPartition {
        match self {
            PartitionScheme::DateOnly | PartitionScheme::DateExchange => {
                SymbolPartition::ClusterSorted
            }
            PartitionScheme::DateExchangeSymbolBuckets(n) => SymbolPartition::HashBuckets(*n),
        }
    }
}

/// 行路由键：参与分区决策的最小字段集（不绑死具体 schema）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordKey<'a> {
    pub symbol: &'a str,
    pub exchange: &'a str,
    /// Unix 毫秒（与 ClickHouse `market_data.timestamp` 口径一致）
    pub timestamp_ms: i64,
}

/// A 股交易日（Asia/Shanghai 日切）：时间戳 +8h 落到 UTC 日期即交易日。
/// 纯函数无日历依赖——非交易日无行情数据，自然无分区目录。
pub fn trade_date_of(timestamp_ms: i64) -> String {
    let sh = chrono::Utc
        .timestamp_opt((timestamp_ms / 1000) + 8 * 3600, 0)
        .single()
        .unwrap_or_else(|| chrono::Utc.timestamp_opt(0, 0).single().unwrap());
    sh.format("%Y-%m-%d").to_string()
}

/// FNV-1a 32 位：无依赖、跨平台稳定（桶号不得随版本漂移——旧数据在新版本
/// writer 下必须仍落同一桶）
fn fnv1a(text: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in text.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// symbol → 桶号（稳定 hash 取模）
pub fn symbol_bucket(symbol: &str, buckets: u32) -> u32 {
    debug_assert!(buckets > 0, "bucket 数必须为正");
    if buckets == 0 {
        return 0;
    }
    fnv1a(symbol) % buckets
}

/// 行 → 湖内相对目录（不含文件名；`{table}` 由调用方与 layer 拼在最前）
pub fn partition_dir(scheme: &PartitionScheme, table: &str, record: &RecordKey) -> String {
    let date = trade_date_of(record.timestamp_ms);
    match scheme {
        PartitionScheme::DateOnly => format!("{table}/trade_date={date}"),
        PartitionScheme::DateExchange => {
            format!("{table}/exchange={}/trade_date={date}", record.exchange)
        }
        PartitionScheme::DateExchangeSymbolBuckets(buckets) => {
            let bucket = symbol_bucket(record.symbol, *buckets);
            format!(
                "{table}/exchange={}/trade_date={date}/bucket={bucket:04}",
                record.exchange
            )
        }
    }
}

// ---------------------------------------------------------------------------
// compaction 计划（L445 §5 小文件治理的策略侧：哪些文件该合并、怎么分组）
// ---------------------------------------------------------------------------

/// 湖内一个已落盘分区文件（清单输入；物理扫描归 lake writer）
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PartitionFile {
    /// 相对湖根的路径（含父分区目录）
    pub relative_path: String,
    pub bytes: u64,
    pub rows: u64,
}

/// 合并计划：`merge_groups` 内每组同分区目录、按文件名升序（seq 单调）拼接；
/// `untouched` 为维持原样的文件（已是目标尺寸或分组不足）
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompactionPlan {
    pub merge_groups: Vec<Vec<PartitionFile>>,
    pub untouched: Vec<PartitionFile>,
}

impl CompactionPlan {
    /// 计划覆盖的文件总数（调度侧用于估算重写量）
    pub fn files_in_groups(&self) -> usize {
        self.merge_groups.iter().map(Vec::len).sum()
    }
}

fn parent_dir(relative_path: &str) -> &str {
    match relative_path.rsplit_once('/') {
        Some((dir, _)) => dir,
        None => "",
    }
}

/// compaction 装箱：同分区目录内，小于目标尺寸的文件数达到 `min_files`
/// 才触发合并；组内按文件名升序贪心累加、超过 `target_bytes` 即封箱开新组
/// （最后一组可能仍小于目标——接受，避免二次重写）。
pub fn plan_compaction(
    files: &[PartitionFile],
    target_bytes: u64,
    min_files: usize,
) -> CompactionPlan {
    let mut by_dir: std::collections::BTreeMap<&str, Vec<&PartitionFile>> = Default::default();
    for file in files {
        by_dir
            .entry(parent_dir(&file.relative_path))
            .or_default()
            .push(file);
    }

    let mut plan = CompactionPlan {
        merge_groups: Vec::new(),
        untouched: Vec::new(),
    };

    for (_, mut group) in by_dir {
        group.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        let small: Vec<&PartitionFile> = group
            .iter()
            .copied()
            .filter(|f| f.bytes < target_bytes)
            .collect();
        if small.len() < min_files {
            plan.untouched.extend(group.into_iter().cloned());
            continue;
        }

        let mut current: Vec<PartitionFile> = Vec::new();
        let mut current_bytes = 0u64;
        for file in small {
            let packed = current_bytes + file.bytes > target_bytes;
            if packed && !current.is_empty() {
                plan.merge_groups.push(std::mem::take(&mut current));
                current_bytes = 0;
            }
            current_bytes += file.bytes;
            current.push(file.clone());
        }
        if !current.is_empty() {
            plan.merge_groups.push(current);
        }
        // 单文件「组」= 无效重写（1 个文件变 1 个文件），降级回 untouched
        let groups = std::mem::take(&mut plan.merge_groups);
        for group in groups {
            if group.len() >= 2 {
                plan.merge_groups.push(group);
            } else {
                plan.untouched.extend(group);
            }
        }
        // 大文件（>= target）维持原样
        plan.untouched.extend(
            group
                .into_iter()
                .filter(|f| f.bytes >= target_bytes)
                .cloned(),
        );
    }

    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trade_date_follows_shanghai_midnight_boundary() {
        // UTC 2026-10-01T15:59:59.999Z = 北京 23:59:59.999 → 10-01
        assert_eq!(trade_date_of(1_790_870_399_999), "2026-10-01");
        // UTC 2026-10-01T16:00:00Z = 北京 10-02 00:00:00 → 10-02（日切）
        assert_eq!(trade_date_of(1_790_870_400_000), "2026-10-02");
        // 上午盘 09:31 北京 = 01:31 UTC
        assert_eq!(trade_date_of(1_790_818_260_000), "2026-10-01");
    }

    #[test]
    fn symbol_bucket_is_stable_and_in_range() {
        // 稳定性：FNV-1a 常量锁定——旧数据在新版本 writer 下同桶
        assert_eq!(symbol_bucket("600519", 64), fnv1a("600519") % 64);
        assert_eq!(symbol_bucket("000001", 64), fnv1a("000001") % 64);
        for symbol in ["600519", "000001", "300750", "688981"] {
            assert!(symbol_bucket(symbol, 64) < 64);
        }
    }

    #[test]
    fn partition_dir_maps_three_schemes() {
        let record = RecordKey {
            symbol: "600519",
            exchange: "SSE",
            timestamp_ms: 1_790_870_400_000,
        };

        assert_eq!(
            partition_dir(&PartitionScheme::DateOnly, "market_data", &record),
            "market_data/trade_date=2026-10-02"
        );
        assert_eq!(
            partition_dir(&PartitionScheme::DateExchange, "market_data", &record),
            "market_data/exchange=SSE/trade_date=2026-10-02"
        );
        assert_eq!(
            partition_dir(
                &PartitionScheme::DateExchangeSymbolBuckets(64),
                "market_data",
                &record
            ),
            format!(
                "market_data/exchange=SSE/trade_date=2026-10-02/bucket={:04}",
                symbol_bucket("600519", 64)
            )
        );
    }

    fn file(dir: &str, seq: usize, bytes: u64) -> PartitionFile {
        PartitionFile {
            relative_path: format!("{dir}/part-{seq:05}.parquet"),
            bytes,
            rows: bytes / 100,
        }
    }

    #[test]
    fn compaction_merges_small_files_within_partition_only() {
        let files = vec![
            file("t/trade_date=2026-10-01", 1, 10),
            file("t/trade_date=2026-10-01", 2, 20),
            file("t/trade_date=2026-10-01", 3, 30),
            file("t/trade_date=2026-10-02", 1, 10),
            file("t/trade_date=2026-10-02", 2, 10),
        ];
        let plan = plan_compaction(&files, 100, 3);

        // 10-01 有 3 个小文件（>= min_files=3）→ 合并一组；10-02 只有 2 个 → 不动
        assert_eq!(plan.merge_groups.len(), 1);
        assert_eq!(plan.merge_groups[0].len(), 3);
        assert!(plan.merge_groups[0]
            .windows(2)
            .all(|w| w[0].relative_path < w[1].relative_path));
        assert_eq!(plan.untouched.len(), 2);
        assert!(plan
            .untouched
            .iter()
            .all(|f| f.relative_path.contains("2026-10-02")));
    }

    #[test]
    fn compaction_respects_target_size_and_skips_large_files() {
        let files = vec![
            file("d", 1, 30),
            file("d", 2, 30),
            file("d", 3, 60),
            file("d", 4, 200), // 已超目标 → untouched
        ];
        let plan = plan_compaction(&files, 100, 2);

        // 30+30=60≤100 同组；再装 60 会到 120 超目标 → 60 单独成组但只有
        // 1 个文件（60+30 已封箱）→ 无效重写降级 untouched
        assert_eq!(plan.merge_groups.len(), 1);
        assert_eq!(plan.merge_groups[0].len(), 2);
        assert_eq!(
            plan.merge_groups[0][0].bytes + plan.merge_groups[0][1].bytes,
            60
        );
        assert_eq!(plan.untouched.len(), 2);
        assert_eq!(plan.untouched.iter().map(|f| f.bytes).sum::<u64>(), 260);
        assert_eq!(plan.files_in_groups(), 2);
    }
}
