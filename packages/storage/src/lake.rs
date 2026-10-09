//! Parquet 数据湖写路径（docs/data-lake-parquet.md §3–§5 落地）。
//!
//! 职责边界：本模块把一批 [`MarketBar`] 按交易日分区、编码为 Parquet、经
//! `.tmp` 临时文件 + 原子 rename 落盘，并提供单分区（`read_partition`）与
//! 时间窗（`read_range`）读回。**不**做：
//! 分区策略决策（复用 [`crate::partition`] 的 L447 策略层）、DataFusion
//! ListingTable 注册（归 data-engine 后置项）、compaction 调度（归 L447）、
//! 对象存储适配（`lake_root` 骨架期为本机目录，§10.1）。
//!
//! 写路径契约（§5）：同 `(trade_date, seq)` 重写内容一致（writer 确定性），
//! 覆盖即重放；`seq` 缺省由 writer 按分区扫描分配（§3「由 writer 原子分配」，
//! 单写者假设下无并发竞态），显式传入则覆盖重放。

use std::fs;
use std::path::{Path, PathBuf};

use alpha_core::{AlphaError, AlphaResult};
use arrow::array::{ArrayRef, Float64Array, TimestampMillisecondArray};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

use crate::partition::{partition_dir, trade_date_of, PartitionScheme};

/// 湖 Silver 层单条标准 OHLCV 记录（列名与 ClickHouse `market_data` 对齐，§4）。
#[derive(Debug, Clone, PartialEq)]
pub struct MarketBar {
    /// Unix 毫秒（UTC），与 ClickHouse `market_data.timestamp` 口径一致。
    pub timestamp_ms: i64,
    /// 六位证券代码。
    pub symbol: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// 落盘结果清单（供调用方登记 / 返回）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LakeWriteReport {
    /// 湖根下分区相对目录（如 `silver/market_data/trade_date=2026-10-02`）。
    pub partition_dir: String,
    /// 相对湖根的完整文件路径（如 `.../part-00042.parquet`）。
    pub relative_path: String,
    /// 本批写出记录数。
    pub rows: usize,
}

/// Parquet 湖 writer：绑定一个湖根目录与分区档位。
pub struct LakeWriter {
    lake_root: PathBuf,
    scheme: PartitionScheme,
}

impl LakeWriter {
    /// 新建 writer；`lake_root` 不存在时在首次写入时按需创建。
    pub fn new(lake_root: impl Into<PathBuf>, scheme: PartitionScheme) -> Self {
        Self {
            lake_root: lake_root.into(),
            scheme,
        }
    }

    pub fn lake_root(&self) -> &Path {
        &self.lake_root
    }

    /// 按交易日把一批 bar 分组写入各自分区（§5：写 `.tmp` → fsync → 原子 rename）。
    ///
    /// `layer`（如 `"silver"`）与 `table`（如 `"market_data"`）拼在一级目录前；
    /// `seq`：`None` = 每分区扫描现有 `part-*.parquet` 自动分配下一个号（跨调用
    /// 单调递增）；`Some(n)` = 全部分区用 n（重放覆盖语义，§5）。返回按分区
    /// （日期升序）排序的写入清单。
    pub fn write_bars(
        &self,
        layer: &str,
        table: &str,
        bars: &[MarketBar],
        seq: Option<u64>,
    ) -> AlphaResult<Vec<LakeWriteReport>> {
        if bars.is_empty() {
            return Ok(Vec::new());
        }

        // 按交易日分组（BTreeMap 保日期升序，输出确定性）。
        let mut by_date: std::collections::BTreeMap<String, Vec<&MarketBar>> =
            std::collections::BTreeMap::new();
        for record in bars {
            by_date
                .entry(trade_date_of(record.timestamp_ms))
                .or_default()
                .push(record);
        }

        let mut reports = Vec::with_capacity(by_date.len());
        for (date, group) in by_date {
            let rel_dir = self.partition_dir_for(
                layer,
                table,
                &group[0].symbol,
                group[0].timestamp_ms,
                &date,
            );
            let abs_dir = self.lake_root.join(&rel_dir);
            let seq = seq.unwrap_or_else(|| next_seq(&abs_dir));
            let file_name = format!("part-{seq:05}.parquet");
            let rel_path = format!("{rel_dir}/{file_name}");

            let batch = bars_to_record_batch(&group)?;
            write_parquet_atomic(&abs_dir, &file_name, &batch)?;

            reports.push(LakeWriteReport {
                partition_dir: rel_dir,
                relative_path: rel_path,
                rows: group.len(),
            });
        }
        Ok(reports)
    }

    /// 读回某分区目录下全部 `.parquet` 文件的记录（按文件名排序拼接）。
    /// 分区不存在返回空向量（与「无分区目录即无数据」语义一致，§3）。
    pub fn read_partition(&self, relative_dir: &str) -> AlphaResult<Vec<MarketBar>> {
        let dir = self.lake_root.join(relative_dir);
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut files: Vec<PathBuf> = fs::read_dir(&dir)
            .map_err(|e| AlphaError::StorageError(format!("read_dir {dir:?}: {e}")))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().map(|x| x == "parquet").unwrap_or(false))
            .collect();
        files.sort();

        let mut out = Vec::new();
        for path in files {
            out.extend(read_parquet_file(&path)?);
        }
        Ok(out)
    }

    /// 读回 `[start_ms, end_ms]` 内某 symbol 的记录（读旁路用）：枚举区间覆盖的
    /// 交易日分区逐个读，按 `timestamp` 升序返回。重复导出落湖会在同分区产生
    /// 多个 part 文件（compaction 归 L447），同 `(symbol, timestamp)` 取文件序
    /// 靠后者，读侧不暴露重复行。
    pub fn read_range(
        &self,
        layer: &str,
        table: &str,
        symbol: &str,
        start_ms: i64,
        end_ms: i64,
    ) -> AlphaResult<Vec<MarketBar>> {
        const DAY_MS: i64 = 24 * 3600 * 1000;
        let mut dates: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
        let mut t = start_ms;
        while t <= end_ms {
            dates.entry(trade_date_of(t)).or_insert(t);
            t += DAY_MS;
        }
        if start_ms <= end_ms {
            dates.entry(trade_date_of(end_ms)).or_insert(end_ms);
        }

        let mut latest: std::collections::BTreeMap<i64, MarketBar> =
            std::collections::BTreeMap::new();
        for (date, ts) in dates {
            let rel_dir = self.partition_dir_for(layer, table, symbol, ts, &date);
            for record in self.read_partition(&rel_dir)? {
                if record.symbol == symbol
                    && record.timestamp_ms >= start_ms
                    && record.timestamp_ms <= end_ms
                {
                    latest.insert(record.timestamp_ms, record);
                }
            }
        }
        Ok(latest.into_values().collect())
    }

    /// 分区相对目录：`{layer}/{partition_dir(scheme, table, record)}`。
    fn partition_dir_for(
        &self,
        layer: &str,
        table: &str,
        symbol: &str,
        timestamp_ms: i64,
        date: &str,
    ) -> String {
        // 复用 L447 策略层（DateOnly 只依赖交易日；多维档走同一函数）。
        let key = crate::partition::RecordKey {
            symbol,
            exchange: "",
            timestamp_ms,
        };
        let base = partition_dir(&self.scheme, table, &key);
        // 用已算好的交易日覆盖（避免对时间戳再算一次日期产生不一致）。
        let base = match base.find("trade_date=") {
            Some(idx) => {
                let head = &base[..idx];
                format!("{head}trade_date={date}")
            }
            None => base,
        };
        format!("{layer}/{base}")
    }
}

/// Arrow schema：七列对齐 §4（timestamp/symbol/OHLC/volume）。
pub fn market_bar_schema() -> Schema {
    Schema::new(vec![
        Field::new(
            "timestamp",
            DataType::Timestamp(TimeUnit::Millisecond, Some("UTC".into())),
            false,
        ),
        Field::new("symbol", DataType::Utf8, false),
        Field::new("open_price", DataType::Float64, false),
        Field::new("high_price", DataType::Float64, false),
        Field::new("low_price", DataType::Float64, false),
        Field::new("close_price", DataType::Float64, false),
        Field::new("volume", DataType::Float64, false),
    ])
}

/// 把一批 bar 编成单列组 batch（§3：文件内按 symbol, timestamp 排序）。
pub fn bars_to_record_batch(bars: &[&MarketBar]) -> AlphaResult<RecordBatch> {
    let schema = std::sync::Arc::new(market_bar_schema());

    let mut sorted: Vec<&MarketBar> = bars.to_vec();
    sorted.sort_by(|a, b| {
        a.symbol
            .cmp(&b.symbol)
            .then(a.timestamp_ms.cmp(&b.timestamp_ms))
    });

    // §4 类型落面：Timestamp(ms, UTC)，毫秒值直存（Arrow Timestamp 数组即 i64 底座）。
    let ts: ArrayRef = std::sync::Arc::new(
        TimestampMillisecondArray::from(sorted.iter().map(|b| b.timestamp_ms).collect::<Vec<_>>())
            .with_timezone("UTC"),
    );
    let symbol: ArrayRef = std::sync::Arc::new(arrow::array::StringArray::from(
        sorted.iter().map(|b| b.symbol.clone()).collect::<Vec<_>>(),
    ));
    let open: ArrayRef = std::sync::Arc::new(Float64Array::from(
        sorted.iter().map(|b| b.open).collect::<Vec<_>>(),
    ));
    let high: ArrayRef = std::sync::Arc::new(Float64Array::from(
        sorted.iter().map(|b| b.high).collect::<Vec<_>>(),
    ));
    let low: ArrayRef = std::sync::Arc::new(Float64Array::from(
        sorted.iter().map(|b| b.low).collect::<Vec<_>>(),
    ));
    let close: ArrayRef = std::sync::Arc::new(Float64Array::from(
        sorted.iter().map(|b| b.close).collect::<Vec<_>>(),
    ));
    let volume: ArrayRef = std::sync::Arc::new(Float64Array::from(
        sorted.iter().map(|b| b.volume).collect::<Vec<_>>(),
    ));

    RecordBatch::try_new(schema, vec![ts, symbol, open, high, low, close, volume])
        .map_err(|e| AlphaError::StorageError(format!("record batch: {e}")))
}

/// 扫描分区目录现有 `part-{n}.parquet`，返回下一个可用序号（无文件为 0）。
/// 单写者假设（§10.2）下无需加锁；解析失败的文件名忽略（不阻碍后续写入）。
fn next_seq(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let n = name.strip_prefix("part-")?.strip_suffix(".parquet")?;
            n.parse::<u64>().ok()
        })
        .max()
        .map(|m| m + 1)
        .unwrap_or(0)
}

/// 把一批 bar 编码为内存 Parquet（snappy、湖 schema），供读旁路响应体直出。
pub fn bars_to_parquet_bytes(bars: &[MarketBar]) -> AlphaResult<Vec<u8>> {
    let refs: Vec<&MarketBar> = bars.iter().collect();
    let batch = bars_to_record_batch(&refs)?;
    let mut buf = Vec::new();
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(&mut buf, batch.schema(), Some(props))
        .map_err(|e| AlphaError::StorageError(format!("arrow writer: {e}")))?;
    writer
        .write(&batch)
        .map_err(|e| AlphaError::StorageError(format!("parquet write: {e}")))?;
    writer
        .into_inner()
        .map_err(|e| AlphaError::StorageError(format!("parquet close: {e}")))?;
    Ok(buf)
}

/// 原子写：写 `.tmp` → fsync → rename（§5）。
fn write_parquet_atomic(dir: &Path, file_name: &str, batch: &RecordBatch) -> AlphaResult<()> {
    fs::create_dir_all(dir)
        .map_err(|e| AlphaError::StorageError(format!("create_dir_all {dir:?}: {e}")))?;
    let tmp = dir.join(format!(".{file_name}.tmp"));
    let final_path = dir.join(file_name);

    {
        let file = fs::File::create(&tmp)
            .map_err(|e| AlphaError::StorageError(format!("create {tmp:?}: {e}")))?;
        // Silver 起步 snappy（§3）。
        let props = WriterProperties::builder()
            .set_compression(Compression::SNAPPY)
            .build();
        let mut writer = ArrowWriter::try_new(file, batch.schema(), Some(props))
            .map_err(|e| AlphaError::StorageError(format!("arrow writer: {e}")))?;
        writer
            .write(batch)
            .map_err(|e| AlphaError::StorageError(format!("parquet write: {e}")))?;
        let file = writer
            .into_inner()
            .map_err(|e| AlphaError::StorageError(format!("parquet close: {e}")))?;
        file.sync_all()
            .map_err(|e| AlphaError::StorageError(format!("fsync {tmp:?}: {e}")))?;
    }

    fs::rename(&tmp, &final_path)
        .map_err(|e| AlphaError::StorageError(format!("rename {tmp:?} -> {final_path:?}: {e}")))?;
    Ok(())
}

/// 读单个 Parquet 文件为 bar 列表。
fn read_parquet_file(path: &Path) -> AlphaResult<Vec<MarketBar>> {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    let file = fs::File::open(path)
        .map_err(|e| AlphaError::StorageError(format!("open {path:?}: {e}")))?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)
        .map_err(|e| AlphaError::StorageError(format!("parquet reader {path:?}: {e}")))?;
    let reader = builder
        .build()
        .map_err(|e| AlphaError::StorageError(format!("parquet reader build: {e}")))?;

    let mut out = Vec::new();
    for batch in reader {
        let batch = batch.map_err(|e| AlphaError::StorageError(format!("parquet read: {e}")))?;
        let ts = batch
            .column(0)
            .as_any()
            .downcast_ref::<TimestampMillisecondArray>()
            .ok_or_else(|| AlphaError::StorageError("timestamp column not Timestamp(ms)".into()))?;
        let symbol = batch
            .column(1)
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .ok_or_else(|| AlphaError::StorageError("symbol column not Utf8".into()))?;
        let open = f64_col(&batch, 2)?;
        let high = f64_col(&batch, 3)?;
        let low = f64_col(&batch, 4)?;
        let close = f64_col(&batch, 5)?;
        let volume = f64_col(&batch, 6)?;

        for i in 0..batch.num_rows() {
            out.push(MarketBar {
                timestamp_ms: ts.value(i),
                symbol: symbol.value(i).to_string(),
                open: open.value(i),
                high: high.value(i),
                low: low.value(i),
                close: close.value(i),
                volume: volume.value(i),
            });
        }
    }
    Ok(out)
}

fn f64_col(batch: &RecordBatch, idx: usize) -> AlphaResult<&Float64Array> {
    batch
        .column(idx)
        .as_any()
        .downcast_ref::<Float64Array>()
        .ok_or_else(|| AlphaError::StorageError(format!("column {idx} not Float64")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::partition::PartitionScheme;

    fn bar(symbol: &str, ts: i64, close: f64) -> MarketBar {
        MarketBar {
            timestamp_ms: ts,
            symbol: symbol.to_string(),
            open: close - 0.5,
            high: close + 1.0,
            low: close - 1.0,
            close,
            volume: 1000.0,
        }
    }

    // 2026-10-02 09:30 CST = 01:30 UTC = 1780363... 用确定值：2026-10-02T01:30:00Z
    const T1: i64 = 1_790_914_200_000; // 2026-10-01T01:30:00Z 附近，下方按实际日切断言
    const T2: i64 = T1 + 24 * 3600 * 1000;

    #[test]
    fn writes_and_reads_single_partition_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let writer = LakeWriter::new(dir.path(), PartitionScheme::DateOnly);
        let bars = vec![bar("600519", T1, 1800.5), bar("000001", T1, 12.25)];
        let reports = writer
            .write_bars("silver", "market_data", &bars, Some(42))
            .unwrap();
        assert_eq!(reports.len(), 1, "同交易日应落一个分区");
        assert!(reports[0]
            .relative_path
            .starts_with("silver/market_data/trade_date="));
        assert!(reports[0].relative_path.ends_with("/part-00042.parquet"));

        let back = writer.read_partition(&reports[0].partition_dir).unwrap();
        assert_eq!(back.len(), 2);
        // 文件内按 symbol 排序：000001 在前。
        assert_eq!(back[0].symbol, "000001");
        assert_eq!(back[0].close, 12.25);
        assert_eq!(back[1].symbol, "600519");
        assert_eq!(back[1].close, 1800.5);
    }

    #[test]
    fn splits_bars_into_trade_date_partitions() {
        let dir = tempfile::tempdir().unwrap();
        let writer = LakeWriter::new(dir.path(), PartitionScheme::DateOnly);
        let bars = vec![bar("600519", T1, 1.0), bar("600519", T2, 2.0)];
        let reports = writer
            .write_bars("silver", "market_data", &bars, Some(1))
            .unwrap();
        assert_eq!(reports.len(), 2, "跨交易日应落两个分区");
        // 日期升序。
        assert!(reports[0].partition_dir < reports[1].partition_dir);
    }

    #[test]
    fn explicit_seq_replays_overwrite_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let writer = LakeWriter::new(dir.path(), PartitionScheme::DateOnly);
        let bars = vec![bar("600519", T1, 9.9)];
        let a = writer
            .write_bars("silver", "market_data", &bars, Some(7))
            .unwrap();
        let b = writer
            .write_bars("silver", "market_data", &bars, Some(7))
            .unwrap();
        assert_eq!(a[0].relative_path, b[0].relative_path, "同 seq 覆盖即重放");
        let files: Vec<_> = fs::read_dir(dir.path().join(&a[0].partition_dir))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|x| x == "parquet")
                    .unwrap_or(false)
            })
            .collect();
        assert_eq!(files.len(), 1, "重放不产生第二个文件");
    }

    #[test]
    fn auto_allocates_monotonic_seq_when_unspecified() {
        let dir = tempfile::tempdir().unwrap();
        let writer = LakeWriter::new(dir.path(), PartitionScheme::DateOnly);
        let bars = vec![bar("600519", T1, 1.0)];
        let a = writer
            .write_bars("silver", "market_data", &bars, None)
            .unwrap();
        let b = writer
            .write_bars("silver", "market_data", &bars, None)
            .unwrap();
        assert!(a[0].relative_path.ends_with("/part-00000.parquet"));
        assert!(b[0].relative_path.ends_with("/part-00001.parquet"));
        let back = writer.read_partition(&a[0].partition_dir).unwrap();
        assert_eq!(back.len(), 2, "两文件按序拼接读回");
    }

    #[test]
    fn read_range_filters_symbol_window_and_dedups_replays() {
        let dir = tempfile::tempdir().unwrap();
        let writer = LakeWriter::new(dir.path(), PartitionScheme::DateOnly);
        let bars = vec![
            bar("600519", T1, 1.0),
            bar("600519", T2, 2.0),
            bar("000001", T1, 9.0),
        ];
        writer
            .write_bars("silver", "market_data", &bars, None)
            .unwrap();
        // 重复导出：同数据再落一个 part，读侧不应出现重复行。
        writer
            .write_bars("silver", "market_data", &bars, None)
            .unwrap();

        let all = writer
            .read_range("silver", "market_data", "600519", T1, T2)
            .unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].close, 1.0);
        assert_eq!(all[1].close, 2.0);

        let first_only = writer
            .read_range("silver", "market_data", "600519", T1, T1)
            .unwrap();
        assert_eq!(first_only.len(), 1);

        let other = writer
            .read_range("silver", "market_data", "000001", T1, T2)
            .unwrap();
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].close, 9.0);

        assert!(writer
            .read_range("silver", "market_data", "600519", T2, T1)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn empty_batch_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let writer = LakeWriter::new(dir.path(), PartitionScheme::DateOnly);
        assert!(writer
            .write_bars("silver", "market_data", &[], None)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn missing_partition_reads_empty() {
        let dir = tempfile::tempdir().unwrap();
        let writer = LakeWriter::new(dir.path(), PartitionScheme::DateOnly);
        let back = writer
            .read_partition("silver/market_data/trade_date=1999-01-01")
            .unwrap();
        assert!(back.is_empty());
    }

    #[test]
    fn schema_has_seven_aligned_columns() {
        let s = market_bar_schema();
        let names: Vec<_> = s.fields().iter().map(|f| f.name().as_str()).collect();
        assert_eq!(
            names,
            vec![
                "timestamp",
                "symbol",
                "open_price",
                "high_price",
                "low_price",
                "close_price",
                "volume"
            ]
        );
    }
}
