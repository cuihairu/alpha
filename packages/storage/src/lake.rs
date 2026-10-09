//! Parquet 数据湖写路径（docs/data-lake-parquet.md §3–§5 落地）。
//!
//! 职责边界：本模块把一批 [`MarketBar`] 按交易日分区、编码为 Parquet、经
//! `.tmp` 临时文件 + 原子 rename 落盘，并提供单分区（`read_partition`）与
//! 时间窗（`read_range`）读回、小文件 compaction 执行（`compact_table`，消费
//! L447 计划器）与表级 manifest 统计（`rebuild_manifest`，§7 catalog v1）。
//! **不**做：分区策略决策（复用 [`crate::partition`] 的 L447 策略层）、
//! DataFusion ListingTable 注册（归 data-engine 后置项）、compaction 内置
//! 调度器（触发时机由调用方定，§5）、对象存储适配（`lake_root` 骨架期为本机
//! 目录，§10.1）。
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

use crate::partition::{partition_dir, trade_date_of, PartitionFile, PartitionScheme};

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

/// compaction 触发参数（§5：阈值与调度归 L447 侧拍板，这里只承载配置）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionOptions {
    /// 分区目标文件尺寸：小于该值的文件才算「小文件」可被装箱。
    pub target_bytes: u64,
    /// 同分区小文件数达到该值才触发合并（低于则整分区 untouched）。
    pub min_files: usize,
}

impl Default for CompactionOptions {
    fn default() -> Self {
        // §3 单文件目标 128MB~512MB：骨架期取下沿；min_files=4（避免
        // 两三个导出就触发重写，又能在例行导出后把碎片收干净）。
        Self {
            target_bytes: 128 * 1024 * 1024,
            min_files: 4,
        }
    }
}

/// compaction 结果：合并写出的新文件与被删除的旧文件清单。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionReport {
    /// 每个合并组一个重写文件（seq 取该分区现有最大号之后）。
    pub written: Vec<LakeWriteReport>,
    /// 合并后删除的旧文件（相对湖根）。
    pub removed: Vec<String>,
    /// 维持原样的文件数（小文件不足触发 + 已达目标尺寸的大文件）。
    pub untouched_files: usize,
}

/// manifest 文件名（§7 catalog v1）。
pub const MANIFEST_NAME: &str = "_manifest.json";

/// 表级统计清单（§7：分区→文件→行数/字节），serde 可序列化落
/// `{layer}/{table}/_manifest.json`。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LakeManifest {
    /// 生成时刻（Unix 毫秒）——清单是时点快照，落湖后须重建。
    pub generated_at_ms: i64,
    /// 分区相对目录 → 统计。
    pub partitions: std::collections::BTreeMap<String, LakePartitionStats>,
    pub total_files: u64,
    pub total_rows: u64,
    pub total_bytes: u64,
}

/// 单分区统计。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LakePartitionStats {
    /// 文件级明细（路径为相对湖根）。
    pub files: Vec<LakeFileStats>,
    pub total_rows: u64,
    pub total_bytes: u64,
}

/// 单文件统计。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LakeFileStats {
    pub path: String,
    pub bytes: u64,
    pub rows: u64,
}

/// 扫描一个分区目录下全部 `part-*.parquet` 为计划器输入（行数取自
/// Parquet footer，不解码数据页）。
fn list_partition_files(lake_root: &Path, dir: &Path) -> AlphaResult<Vec<PartitionFile>> {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    let rel_dir = dir
        .strip_prefix(lake_root)
        .map_err(|e| AlphaError::StorageError(format!("strip_prefix {dir:?}: {e}")))?
        .to_string_lossy()
        .into_owned();
    let mut files = Vec::new();
    for entry in
        fs::read_dir(dir).map_err(|e| AlphaError::StorageError(format!("read_dir {dir:?}: {e}")))?
    {
        let path = entry
            .map_err(|e| AlphaError::StorageError(format!("read_dir entry {dir:?}: {e}")))?
            .path();
        if path.extension().map(|x| x == "parquet").unwrap_or(false) {
            let file = fs::File::open(&path)
                .map_err(|e| AlphaError::StorageError(format!("open {path:?}: {e}")))?;
            let rows = ParquetRecordBatchReaderBuilder::try_new(file)
                .map_err(|e| AlphaError::StorageError(format!("footer {path:?}: {e}")))?
                .metadata()
                .file_metadata()
                .num_rows();
            files.push(PartitionFile {
                relative_path: format!("{rel_dir}/{}", path.file_name().unwrap().to_string_lossy()),
                bytes: fs::metadata(&path)
                    .map_err(|e| AlphaError::StorageError(format!("metadata {path:?}: {e}")))?
                    .len(),
                rows: rows.max(0) as u64,
            });
        }
    }
    Ok(files)
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

    /// 全表 compaction（§5 小文件治理的执行侧，策略消费 L447 `plan_compaction`）：
    /// 扫 `{layer}/{table}` 下全部分区的小文件 → 装箱计划 → 组内按文件名升序
    /// 拼接（同 `(symbol, timestamp)` 取后写者，重放语义与读侧一致）→ 以新
    /// seq 落合并文件（全部组写成功后才删旧文件：中途崩溃最多留重复数据，
    /// 读侧按文件序去重，不丢数）。
    ///
    /// 骨架期无调度器（§7/§10）：由调用方按需触发（写透后/定时均可），
    /// 多维分区档下本函数按目录逐分区自然生效。
    pub fn compact_table(
        &self,
        layer: &str,
        table: &str,
        opts: &CompactionOptions,
    ) -> AlphaResult<CompactionReport> {
        let table_dir = self.lake_root.join(layer).join(table);
        let mut files = Vec::new();
        if table_dir.exists() {
            let mut dirs: Vec<PathBuf> = fs::read_dir(&table_dir)
                .map_err(|e| AlphaError::StorageError(format!("read_dir {table_dir:?}: {e}")))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_dir())
                .collect();
            dirs.sort();
            for dir in dirs {
                files.extend(list_partition_files(&self.lake_root, &dir)?);
            }
        }

        let plan = crate::partition::plan_compaction(&files, opts.target_bytes, opts.min_files);
        let mut report = CompactionReport {
            written: Vec::with_capacity(plan.merge_groups.len()),
            removed: Vec::new(),
            untouched_files: plan.untouched.len(),
        };
        if plan.merge_groups.is_empty() {
            return Ok(report);
        }

        // 每分区分配新 seq：现有最大号之后连续递增（合并文件名同样单调，
        // 读侧文件序去重时合并结果天然靠后胜出）。
        let mut next_seq_by_dir: std::collections::HashMap<String, u64> =
            std::collections::HashMap::new();
        let mut written: Vec<LakeWriteReport> = Vec::new();
        for group in &plan.merge_groups {
            let rel_dir = group[0]
                .relative_path
                .rsplit_once('/')
                .map(|(dir, _)| dir.to_string())
                .unwrap_or_default();
            let seq = *next_seq_by_dir
                .entry(rel_dir.clone())
                .or_insert_with(|| next_seq(&self.lake_root.join(&rel_dir)));
            next_seq_by_dir.insert(rel_dir.clone(), seq + 1);

            // 文件名升序拼接 + 去重（后写者胜出，与 read_range 读语义一致）。
            let mut latest: std::collections::BTreeMap<(String, i64), MarketBar> =
                std::collections::BTreeMap::new();
            for file in group {
                let path = self.lake_root.join(&file.relative_path);
                for record in read_parquet_file(&path)? {
                    latest.insert((record.symbol.clone(), record.timestamp_ms), record);
                }
            }
            let rows = latest.len();
            let bars: Vec<MarketBar> = latest.into_values().collect();

            let file_name = format!("part-{seq:05}.parquet");
            let batch = bars_to_record_batch(&bars.iter().collect::<Vec<_>>())?;
            write_parquet_atomic(&self.lake_root.join(&rel_dir), &file_name, &batch)?;
            written.push(LakeWriteReport {
                relative_path: format!("{rel_dir}/{file_name}"),
                partition_dir: rel_dir,
                rows,
            });
        }

        // 全部组写成功后才删旧文件（§5：合并不可丢数）。
        for group in &plan.merge_groups {
            for file in group {
                fs::remove_file(self.lake_root.join(&file.relative_path)).map_err(|e| {
                    AlphaError::StorageError(format!("remove {}: {e}", file.relative_path))
                })?;
                report.removed.push(file.relative_path.clone());
            }
        }
        report.written = written;
        Ok(report)
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

    /// 全量重建表级 manifest（§7 catalog v1：`_manifest.json`，分区→文件→
    /// 行数/字节统计）。骨架期「目录即清单」，本函数按需重建（写透/compaction
    /// 后或调度侧定期），读侧不依赖它（读路径直接列目录，manifest 只作统计
    /// 面与下游 catalog 升级底座）。原子写（.tmp + rename），行数取 Parquet
    /// footer 不解码数据页。
    pub fn rebuild_manifest(&self, layer: &str, table: &str) -> AlphaResult<LakeManifest> {
        let table_dir = self.lake_root.join(layer).join(table);
        let mut manifest = LakeManifest::default();
        if table_dir.exists() {
            let mut dirs: Vec<PathBuf> = fs::read_dir(&table_dir)
                .map_err(|e| AlphaError::StorageError(format!("read_dir {table_dir:?}: {e}")))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_dir())
                .collect();
            dirs.sort();
            for dir in dirs {
                let files = list_partition_files(&self.lake_root, &dir)?;
                if files.is_empty() {
                    continue;
                }
                let rel_dir = dir
                    .strip_prefix(&self.lake_root)
                    .map_err(|e| AlphaError::StorageError(format!("strip_prefix {dir:?}: {e}")))?
                    .to_string_lossy()
                    .into_owned();
                let entry = LakePartitionStats {
                    files: files
                        .iter()
                        .map(|f| LakeFileStats {
                            path: f.relative_path.clone(),
                            bytes: f.bytes,
                            rows: f.rows,
                        })
                        .collect(),
                    total_rows: files.iter().map(|f| f.rows).sum(),
                    total_bytes: files.iter().map(|f| f.bytes).sum(),
                };
                manifest.total_files += entry.files.len() as u64;
                manifest.total_rows += entry.total_rows;
                manifest.total_bytes += entry.total_bytes;
                manifest.partitions.insert(rel_dir, entry);
            }
        }
        manifest.generated_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        let dir = self.lake_root.join(layer).join(table);
        fs::create_dir_all(&dir)
            .map_err(|e| AlphaError::StorageError(format!("create_dir_all {dir:?}: {e}")))?;
        let body = serde_json::to_vec_pretty(&manifest)
            .map_err(|e| AlphaError::StorageError(format!("manifest serialize: {e}")))?;
        let tmp = dir.join(".manifest.tmp");
        fs::write(&tmp, body)
            .map_err(|e| AlphaError::StorageError(format!("write {tmp:?}: {e}")))?;
        fs::rename(&tmp, dir.join(MANIFEST_NAME))
            .map_err(|e| AlphaError::StorageError(format!("rename manifest: {e}")))?;
        Ok(manifest)
    }

    /// 读回上次 `rebuild_manifest` 落的清单；未生成过返回 None。
    pub fn read_manifest(&self, layer: &str, table: &str) -> AlphaResult<Option<LakeManifest>> {
        let path = self.lake_root.join(layer).join(table).join(MANIFEST_NAME);
        if !path.exists() {
            return Ok(None);
        }
        let body =
            fs::read(&path).map_err(|e| AlphaError::StorageError(format!("read {path:?}: {e}")))?;
        serde_json::from_slice(&body)
            .map(Some)
            .map_err(|e| AlphaError::StorageError(format!("manifest parse: {e}")))
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
    fn compaction_merges_small_files_and_removes_sources() {
        let dir = tempfile::tempdir().unwrap();
        let writer = LakeWriter::new(dir.path(), PartitionScheme::DateOnly);
        let bars = vec![bar("600519", T1, 1.0), bar("000001", T1, 9.0)];
        for _ in 0..3 {
            writer
                .write_bars("silver", "market_data", &bars, None)
                .unwrap();
        }

        let report = writer
            .compact_table(
                "silver",
                "market_data",
                &CompactionOptions {
                    target_bytes: 1024 * 1024,
                    min_files: 2,
                },
            )
            .unwrap();
        assert_eq!(report.written.len(), 1);
        assert_eq!(report.removed.len(), 3);
        assert_eq!(report.untouched_files, 0);

        // 分区里只剩合并文件，读回行数 = 去重后的 2 条（3 份同数据重放）。
        let part_dir = &report.written[0].partition_dir;
        let parquet_count = fs::read_dir(dir.path().join(part_dir))
            .unwrap()
            .filter(|e| e.as_ref().unwrap().path().extension().unwrap() == "parquet")
            .count();
        assert_eq!(parquet_count, 1);
        let back = writer.read_partition(part_dir).unwrap();
        assert_eq!(back.len(), 2, "重放重复行合并时去重");
        assert_eq!(report.written[0].rows, 2);

        // 合并后 seq 单调：再次写透分配的序号仍在合并文件之后。
        let a = writer
            .write_bars("silver", "market_data", &bars, None)
            .unwrap();
        assert!(a[0].relative_path > report.written[0].relative_path);
    }

    #[test]
    fn compaction_skips_partition_below_min_files() {
        let dir = tempfile::tempdir().unwrap();
        let writer = LakeWriter::new(dir.path(), PartitionScheme::DateOnly);
        let bars = vec![bar("600519", T1, 1.0)];
        writer
            .write_bars("silver", "market_data", &bars, None)
            .unwrap();

        let report = writer
            .compact_table(
                "silver",
                "market_data",
                &CompactionOptions {
                    target_bytes: 1024 * 1024,
                    min_files: 2,
                },
            )
            .unwrap();
        assert!(report.written.is_empty());
        assert!(report.removed.is_empty());
        assert_eq!(report.untouched_files, 1, "小文件不足不触发重写");
    }

    #[test]
    fn compaction_on_missing_table_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let writer = LakeWriter::new(dir.path(), PartitionScheme::DateOnly);
        let report = writer
            .compact_table("silver", "market_data", &CompactionOptions::default())
            .unwrap();
        assert!(report.written.is_empty());
        assert!(report.removed.is_empty());
        assert_eq!(report.untouched_files, 0);
    }

    #[test]
    fn manifest_rebuilds_and_roundtrips_across_writes_and_compaction() {
        let dir = tempfile::tempdir().unwrap();
        let writer = LakeWriter::new(dir.path(), PartitionScheme::DateOnly);
        let bars = vec![bar("600519", T1, 1.0), bar("000001", T1, 9.0)];
        let bars2 = vec![bar("600519", T2, 2.0)];

        assert!(writer
            .read_manifest("silver", "market_data")
            .unwrap()
            .is_none());

        writer
            .write_bars("silver", "market_data", &bars, None)
            .unwrap();
        writer
            .write_bars("silver", "market_data", &bars, None)
            .unwrap();
        writer
            .write_bars("silver", "market_data", &bars2, None)
            .unwrap();

        let m1 = writer.rebuild_manifest("silver", "market_data").unwrap();
        assert_eq!(m1.total_files, 3);
        assert_eq!(m1.total_rows, 5);
        assert_eq!(m1.partitions.len(), 2, "两个交易日分区");
        let round = writer
            .read_manifest("silver", "market_data")
            .unwrap()
            .unwrap();
        assert_eq!(round, m1, "落盘清单与返回值一致");

        // 合并后再重建：文件数收敛、行数按去重口径统计。
        writer
            .compact_table(
                "silver",
                "market_data",
                &CompactionOptions {
                    target_bytes: 1024 * 1024,
                    min_files: 2,
                },
            )
            .unwrap();
        let m2 = writer.rebuild_manifest("silver", "market_data").unwrap();
        assert_eq!(m2.total_files, 2, "首日 3 文件并 1 + 次日 1 文件");
        assert_eq!(m2.total_rows, 3, "首日重放去重 2 行 + 次日 1 行");
    }

    #[test]
    fn manifest_on_missing_table_is_empty_not_error() {
        let dir = tempfile::tempdir().unwrap();
        let writer = LakeWriter::new(dir.path(), PartitionScheme::DateOnly);
        let m = writer.rebuild_manifest("silver", "market_data").unwrap();
        assert_eq!(m.total_files, 0);
        assert!(m.partitions.is_empty());
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
