//! 本地数据导出：CSV / JSON
//!
//! 骨架期导出到应用数据目录下的 `exports/`（TODO L113 会接原生「另存为」对话框与
//! 任意路径：届时只需在导出成功后追加一次目录选择）。导出时刻由调用方注入，
//! 使文件名可断言（否则只能靠正则匹配）。

use crate::error::{DesktopError, DesktopResult};
use alpha_core::models::MarketData;
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

/// CSV 表头（与 web 前端导入口径一致）
pub const CSV_HEADER: [&str; 7] = [
    "symbol",
    "timestamp",
    "price",
    "volume",
    "open",
    "high",
    "low",
];

/// 支持的导出格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// 逗号分隔值
    Csv,
    /// JSON 数组
    Json,
}

impl ExportFormat {
    /// 解析前端传入的格式串（大小写不敏感）
    pub fn parse(raw: &str) -> DesktopResult<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "csv" => Ok(Self::Csv),
            "json" => Ok(Self::Json),
            other => Err(DesktopError::InvalidInput(format!(
                "不支持的导出格式: {other}（支持 csv/json）"
            ))),
        }
    }

    /// 格式标签（文件名后缀）
    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
        }
    }
}

/// 导出结果：落盘路径 + 文件名（供命令返回给前端）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportOutcome {
    /// 落盘绝对/相对路径
    pub path: PathBuf,
    /// 文件名（含标的与导出时刻）
    pub filename: String,
    /// 导出行数（K 线根数）
    pub rows: usize,
}

/// 文件名：`<symbol>_<YYYYmmdd_HHMMSS>.<ext>`（与既有实现口径一致）
pub fn export_filename(symbol: &str, format: ExportFormat, at: DateTime<Utc>) -> String {
    format!(
        "{}_{}.{}",
        symbol,
        at.format("%Y%m%d_%H%M%S"),
        format.extension()
    )
}

/// 导出一段序列到 `dir`
pub fn export(
    data: &[MarketData],
    dir: &Path,
    symbol: &str,
    format: ExportFormat,
    at: DateTime<Utc>,
) -> DesktopResult<ExportOutcome> {
    if data.is_empty() {
        return Err(DesktopError::InvalidInput("没有可导出的数据".to_string()));
    }
    std::fs::create_dir_all(dir)?;
    let filename = export_filename(symbol, format, at);
    let path = dir.join(&filename);
    match format {
        ExportFormat::Csv => write_csv(data, &path)?,
        ExportFormat::Json => write_json(data, &path)?,
    }
    Ok(ExportOutcome {
        path,
        filename,
        rows: data.len(),
    })
}

/// 按 IPC 请求批量导出（`export_data` 命令的实现体）
///
/// 格式串解析、取数、逐标的导出都在框架层；返回导出的文件名列表（前端提示用）。
/// 空标的列表在这里被拒——早前是在接线层判的（`symbols.is_empty()`），那类判断
/// 属于业务口径，放在只能靠 macOS CI 编译的接线层等于没有测试覆盖。
pub fn export_request(
    request: &crate::ipc::ExportRequest,
    dir: &Path,
    at: DateTime<Utc>,
) -> DesktopResult<Vec<String>> {
    let format = ExportFormat::parse(&request.format)?;
    if request.symbols.is_empty() {
        return Err(DesktopError::InvalidInput(
            "symbols 至少需要一个标的".to_string(),
        ));
    }
    let mut files = Vec::with_capacity(request.symbols.len());
    for symbol in &request.symbols {
        let series = crate::market::synthetic_series(symbol, crate::market::DEFAULT_BARS);
        files.push(export(&series, dir, symbol, format, at)?.filename);
    }
    Ok(files)
}

fn write_csv(data: &[MarketData], path: &Path) -> DesktopResult<()> {
    let mut writer = csv::Writer::from_path(path)?;
    writer.write_record(CSV_HEADER)?;
    for item in data {
        writer.write_record([
            item.symbol.as_str(),
            item.timestamp.to_rfc3339().as_str(),
            &item.price.to_string(),
            &item.volume.to_string(),
            &optional(item.open),
            &optional(item.high),
            &optional(item.low),
        ])?;
    }
    writer.flush()?;
    Ok(())
}

fn write_json(data: &[MarketData], path: &Path) -> DesktopResult<()> {
    let json = serde_json::to_string_pretty(data)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// 可选字段的空值表示（CSV 无 null，用空单元格）
fn optional(value: Option<f64>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("固定时间戳")
    }

    fn series(n: usize) -> Vec<MarketData> {
        crate::market::synthetic_series_at("600519", n, at())
    }

    #[test]
    fn format_parse_accepts_case_insensitive_known_formats() {
        assert_eq!(ExportFormat::parse("csv").expect("csv"), ExportFormat::Csv);
        assert_eq!(
            ExportFormat::parse(" JSON ").expect("json"),
            ExportFormat::Json
        );
        assert_eq!(ExportFormat::Csv.extension(), "csv");
        assert_eq!(ExportFormat::Json.extension(), "json");
    }

    #[test]
    fn format_parse_rejects_unknown_format() {
        let err = ExportFormat::parse("xlsx").expect_err("应判非法");
        assert_eq!(err.kind(), "invalid_input");
        assert!(err.to_string().contains("xlsx"), "实际: {err}");
    }

    #[test]
    fn filename_contains_symbol_timestamp_and_extension() {
        assert_eq!(
            export_filename("600519", ExportFormat::Csv, at()),
            "600519_20231114_221320.csv"
        );
        assert!(export_filename("AAPL", ExportFormat::Json, at()).ends_with(".json"));
    }

    #[test]
    fn export_request_writes_one_file_per_symbol_in_order() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let request =
            crate::ipc::ExportRequest::new(vec!["600519".to_string(), "000001".to_string()], "csv");
        let files = export_request(&request, tmp.path(), at()).expect("批量导出");

        assert_eq!(files.len(), 2, "每个标的一个文件");
        assert!(files[0].starts_with("600519"), "{files:?}");
        assert!(files[1].starts_with("000001"), "{files:?}");
        for name in &files {
            assert!(tmp.path().join(name).is_file(), "应落盘: {name}");
        }
    }

    #[test]
    fn export_request_rejects_empty_symbol_list() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let request = crate::ipc::ExportRequest::new(Vec::new(), "csv");
        let err = export_request(&request, tmp.path(), at()).expect_err("空列表应报错");
        assert_eq!(err.kind(), "invalid_input");
        assert!(err.to_string().contains("symbols"), "实际: {err}");
    }

    #[test]
    fn export_request_rejects_unknown_format_before_writing() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let request = crate::ipc::ExportRequest::new(vec!["600519".to_string()], "xlsx");
        let err = export_request(&request, tmp.path(), at()).expect_err("未知格式应报错");
        assert!(err.to_string().contains("xlsx"), "实际: {err}");
        assert_eq!(
            std::fs::read_dir(tmp.path()).expect("列目录").count(),
            0,
            "失败不应留下残留文件"
        );
    }

    #[test]
    fn export_request_honours_json_format() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let request = crate::ipc::ExportRequest::new(vec!["600519".to_string()], "json");
        let files = export_request(&request, tmp.path(), at()).expect("导出");
        assert!(files[0].ends_with(".json"), "{files:?}");
    }

    #[test]
    fn export_creates_dir_and_reports_outcome() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let dir = tmp.path().join("exports");
        let data = series(10);
        let outcome = export(&data, &dir, "600519", ExportFormat::Csv, at()).expect("导出");

        assert!(outcome.path.is_file());
        assert_eq!(outcome.filename, "600519_20231114_221320.csv");
        assert_eq!(outcome.rows, 10);
    }

    #[test]
    fn csv_export_has_header_and_one_row_per_bar() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let data = series(7);
        let outcome = export(&data, tmp.path(), "600519", ExportFormat::Csv, at()).expect("导出");

        let mut reader = csv::Reader::from_path(&outcome.path).expect("读取 CSV");
        assert_eq!(reader.headers().expect("表头").len(), CSV_HEADER.len());
        let rows: Vec<_> = reader.records().map(|r| r.expect("记录")).collect();
        assert_eq!(rows.len(), 7, "数据行数应等于 K 线根数");
        assert_eq!(&rows[0][0], "600519");
    }

    #[test]
    fn csv_roundtrip_preserves_prices() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let data = series(5);
        let outcome = export(&data, tmp.path(), "600519", ExportFormat::Csv, at()).expect("导出");

        let mut reader = csv::Reader::from_path(&outcome.path).expect("读取 CSV");
        let parsed: Vec<f64> = reader
            .records()
            .map(|r| {
                let r = r.expect("记录");
                r[2].parse::<f64>().expect("价格列可解析")
            })
            .collect();
        let original: Vec<f64> = data.iter().map(|d| d.price).collect();
        assert_eq!(parsed, original, "CSV 往返应保持价格");
    }

    #[test]
    fn csv_uses_empty_cell_for_missing_optionals() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let mut data = series(1);
        data[0].open = None;
        let outcome = export(&data, tmp.path(), "X", ExportFormat::Csv, at()).expect("导出");

        let mut reader = csv::Reader::from_path(&outcome.path).expect("读取 CSV");
        let mut records = reader.records();
        let row = records.next().expect("数据行").expect("记录");
        let open_col = CSV_HEADER
            .iter()
            .position(|h| *h == "open")
            .expect("open 列");
        assert_eq!(&row[open_col], "", "缺失字段应为空单元格: {row:?}");
        let high_col = CSV_HEADER
            .iter()
            .position(|h| *h == "high")
            .expect("high 列");
        assert!(!row[high_col].is_empty(), "有值字段不应为空");
    }

    #[test]
    fn json_export_roundtrips_exactly() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let data = series(4);
        let outcome = export(&data, tmp.path(), "600519", ExportFormat::Json, at()).expect("导出");

        let parsed: Vec<MarketData> =
            serde_json::from_str(&std::fs::read_to_string(&outcome.path).expect("读 JSON"))
                .expect("解析 JSON");
        assert_eq!(parsed.len(), data.len());
        for (a, b) in parsed.iter().zip(&data) {
            assert_eq!(a.symbol, b.symbol);
            assert_eq!(a.price, b.price);
            assert_eq!(a.timestamp, b.timestamp);
        }
    }

    #[test]
    fn export_rejects_empty_series() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let err = export(&[], tmp.path(), "X", ExportFormat::Csv, at()).expect_err("应判非法");
        assert_eq!(err.kind(), "invalid_input");
    }

    #[test]
    fn export_does_not_clobber_other_format() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let data = series(2);
        let csv = export(&data, tmp.path(), "A", ExportFormat::Csv, at()).expect("CSV 导出");
        let json = export(&data, tmp.path(), "A", ExportFormat::Json, at()).expect("JSON 导出");
        assert_ne!(csv.path, json.path, "同名不同时刻/格式应落不同文件");
        assert!(csv.path.is_file() && json.path.is_file());
    }

    #[test]
    fn export_reports_io_error_for_unwritable_dir() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let blocker = tmp.path().join("blocker");
        std::fs::write(&blocker, b"file").expect("写占位文件");
        // blocker 是文件，不是目录 → create_dir_all 失败
        let err = export(
            &series(1),
            &blocker.join("nested"),
            "A",
            ExportFormat::Csv,
            at(),
        )
        .expect_err("应失败");
        assert_eq!(err.kind(), "io");
    }
}
