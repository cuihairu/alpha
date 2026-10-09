//! 本地数据导出：CSV / JSON
//!
//! 两条写路径（TODO L112 骨架 + L113 原生文件集成）：
//! * [`export`]：写到应用数据目录下的 `exports/`（快速导出，无需用户选路径）；
//! * [`export_to_file`]：写到调用方给定的显式文件路径（L113：前端经原生
//!   「另存为」对话框拿到路径后传入，见 `gui::export_symbol_to_file`）。
//!   两条路径写盘都走「临时文件 + rename」原子替换（与 `config::save` 同口径），
//!   导出时刻由调用方注入，使文件名可断言（否则只能靠正则匹配）。

use crate::error::{DesktopError, DesktopResult};
use alpha_core::models::MarketData;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// 与目标同目录的临时文件路径（`a.csv` → `a.csv.tmp`，无后缀 → `name.tmp`）
fn tmp_sibling(path: &Path) -> PathBuf {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let tmp_ext = if ext.is_empty() {
        "tmp".to_string()
    } else {
        format!("{ext}.tmp")
    };
    path.with_extension(tmp_ext)
}

/// 导出一段序列到调用方给定的显式文件路径（L113 原生「另存为」链路）
///
/// 口径（均为框架层断言，接线层只透传，见 `export_symbol_request`）：
/// * 路径后缀必须与格式一致（`csv`/`json`，大小写不敏感），否则拒绝且不落任何文件；
/// * 父目录不存在则自动创建；写盘走临时文件 + rename 原子替换（与 `config::save` 同口径）；
/// * 空序列拒绝（与 [`export`] 一致）。
pub fn export_to_file(
    data: &[MarketData],
    path: &Path,
    format: ExportFormat,
) -> DesktopResult<ExportOutcome> {
    if data.is_empty() {
        return Err(DesktopError::InvalidInput("没有可导出的数据".to_string()));
    }
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            DesktopError::InvalidInput(format!("导出路径缺少文件名: {}", path.display()))
        })?
        .to_string();
    let actual = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if actual != format.extension() {
        return Err(DesktopError::InvalidInput(format!(
            "导出路径后缀应为 .{}（与格式一致），实际: {}",
            format.extension(),
            path.display()
        )));
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let tmp = tmp_sibling(path);
    match format {
        ExportFormat::Csv => write_csv(data, &tmp)?,
        ExportFormat::Json => write_json(data, &tmp)?,
    }
    std::fs::rename(&tmp, path)?;
    Ok(ExportOutcome {
        path: path.to_path_buf(),
        filename,
        rows: data.len(),
    })
}

/// 按「另存为」请求导出单个标的（`export_symbol_to_file` 命令的实现体）
///
/// 格式串解析、标的校验、取数、落盘都在框架层；`dest` 是前端经原生对话框拿到的
/// 用户自选路径（含文件名与后缀）。未知格式/空标的在写盘前拒绝，不留残留文件。
pub fn export_symbol_request(
    symbol: &str,
    format_raw: &str,
    dest: &Path,
) -> DesktopResult<ExportOutcome> {
    let format = ExportFormat::parse(format_raw)?;
    if symbol.trim().is_empty() {
        return Err(DesktopError::InvalidInput("symbol 不能为空".to_string()));
    }
    let series = crate::market::synthetic_series(symbol, crate::market::DEFAULT_BARS);
    export_to_file(&series, dest, format)
}

/// 覆盖确认对话框标题（框架层定稿，接线层只透传给平台对话框）
pub const OVERWRITE_TITLE: &str = "覆盖确认";

/// 目标路径是否已有文件需要确认覆盖（纯判定：存在且为普通文件）。
///
/// 目录/不存在都不算冲突——不存在直接写，目录会在后续写盘时报错（与既有口径一致）。
pub fn overwrite_required(path: &Path) -> bool {
    path.is_file()
}

/// 覆盖确认提示文案（接线层只透传，不自造文案）
pub fn overwrite_prompt(path: &Path) -> String {
    format!("{} 已存在，是否覆盖？", path.display())
}

/// 带覆盖确认的「另存为」导出（L113 收尾：写前确认未做的补上）。
///
/// `confirm` 是接线层注入的平台确认回调（真实环境为原生 yes/no 阻塞对话框，
/// 见 `platform::confirm_overwrite`；测试为闭包）。口径：目标已存在且 `confirm`
/// 返回 `false` → `Ok(None)`（用户取消，不落盘、不留残留）；否则走
/// [`export_symbol_request`] 返回 `Ok(Some(outcome))`。判定与流程全在本层，
/// 接线层只做 `ask(...)` 的机械透传（与既有薄度契约一致）。
pub fn export_symbol_request_confirm<F>(
    symbol: &str,
    format_raw: &str,
    dest: &Path,
    confirm: F,
) -> DesktopResult<Option<ExportOutcome>>
where
    F: FnOnce(&Path) -> bool,
{
    if overwrite_required(dest) && !confirm(dest) {
        return Ok(None);
    }
    export_symbol_request(symbol, format_raw, dest).map(Some)
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

    // —— L113 原生「另存为」链路（显式路径导出） ——

    #[test]
    fn to_file_csv_writes_parseable_file_at_explicit_path() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let dest = tmp.path().join("picked-600519.csv");
        let outcome = export_to_file(&series(6), &dest, ExportFormat::Csv).expect("显式路径导出");

        assert_eq!(outcome.filename, "picked-600519.csv");
        assert_eq!(outcome.path, dest);
        assert_eq!(outcome.rows, 6);
        let mut reader = csv::Reader::from_path(&dest).expect("读取 CSV");
        let rows: Vec<_> = reader.records().map(|r| r.expect("记录")).collect();
        assert_eq!(rows.len(), 6);
        assert_eq!(&rows[0][0], "600519");
    }

    #[test]
    fn to_file_accepts_uppercase_extension() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let dest = tmp.path().join("picked.CSV");
        // 后缀大小写不敏感
        let outcome = export_to_file(&series(3), &dest, ExportFormat::Csv).expect("大写后缀应接受");
        assert_eq!(outcome.rows, 3);
        assert!(dest.is_file());
    }

    #[test]
    fn to_file_rejects_extension_mismatch_and_writes_nothing() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let dest = tmp.path().join("picked.json");
        let err = export_to_file(&series(3), &dest, ExportFormat::Csv).expect_err("后缀不符");
        assert_eq!(err.kind(), "invalid_input");
        assert!(err.to_string().contains(".csv"), "实际: {err}");
        assert!(!dest.exists(), "拒绝时不应落文件");
        assert_eq!(
            std::fs::read_dir(tmp.path()).expect("列目录").count(),
            0,
            "拒绝时不应留临时文件"
        );
    }

    #[test]
    fn to_file_rejects_missing_filename() {
        let err =
            export_to_file(&series(1), Path::new(""), ExportFormat::Csv).expect_err("空路径应拒绝");
        assert_eq!(err.kind(), "invalid_input");
    }

    #[test]
    fn to_file_rejects_empty_series() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let dest = tmp.path().join("empty.csv");
        let err = export_to_file(&[], &dest, ExportFormat::Csv).expect_err("空序列应拒绝");
        assert_eq!(err.kind(), "invalid_input");
        assert!(!dest.exists());
    }

    #[test]
    fn to_file_creates_missing_parent_dirs_and_leaves_no_tmp() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let dest = tmp.path().join("nested/deeper/picked.json");
        let outcome = export_to_file(&series(2), &dest, ExportFormat::Json).expect("应自建父目录");
        assert_eq!(outcome.rows, 2);
        let parsed: Vec<MarketData> =
            serde_json::from_str(&std::fs::read_to_string(&dest).expect("读 JSON"))
                .expect("解析 JSON");
        assert_eq!(parsed.len(), 2);
        let leftovers: Vec<_> = std::fs::read_dir(dest.parent().expect("父目录"))
            .expect("列目录")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            leftovers,
            vec!["picked.json".to_string()],
            "临时文件应已被 rename 消耗，残留: {leftovers:?}"
        );
    }

    #[test]
    fn to_file_overwrites_previous_export() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let dest = tmp.path().join("picked.csv");
        export_to_file(&series(2), &dest, ExportFormat::Csv).expect("首次导出");
        export_to_file(&series(5), &dest, ExportFormat::Csv).expect("覆盖导出");

        let mut reader = csv::Reader::from_path(&dest).expect("读取 CSV");
        let rows: Vec<_> = reader.records().map(|r| r.expect("记录")).collect();
        assert_eq!(rows.len(), 5, "覆盖后应为新内容");
    }

    #[test]
    fn to_file_reports_io_error_for_unwritable_target() {
        let tmp = tempfile::tempdir().expect("临时目录");
        // 目标是已存在的目录（且后缀与格式一致，绕过前置校验）：
        // rename 会失败，错误应映射为 io
        let dest = tmp.path().join("picked.json");
        std::fs::create_dir(&dest).expect("建占位目录");
        let err = export_to_file(&series(1), &dest, ExportFormat::Json).expect_err("写目录应失败");
        assert_eq!(err.kind(), "io");
    }

    #[test]
    fn symbol_request_writes_single_symbol_file() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let dest = tmp.path().join("save-as.csv");
        let outcome = export_symbol_request("600519", "csv", &dest).expect("另存为请求");

        assert_eq!(outcome.filename, "save-as.csv");
        assert_eq!(outcome.rows, crate::market::DEFAULT_BARS);
        let mut reader = csv::Reader::from_path(&dest).expect("读取 CSV");
        let rows: Vec<_> = reader.records().map(|r| r.expect("记录")).collect();
        assert_eq!(rows.len(), crate::market::DEFAULT_BARS);
        assert!(rows.iter().all(|r| &r[0] == "600519"));
    }

    #[test]
    fn symbol_request_rejects_unknown_format_before_writing() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let dest = tmp.path().join("save-as.csv");
        let err = export_symbol_request("600519", "xlsx", &dest).expect_err("未知格式应拒绝");
        assert!(err.to_string().contains("xlsx"), "实际: {err}");
        assert!(!dest.exists(), "拒绝时不应落文件");
    }

    #[test]
    fn symbol_request_rejects_blank_symbol_before_writing() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let dest = tmp.path().join("save-as.csv");
        let err = export_symbol_request("  ", "csv", &dest).expect_err("空标的应拒绝");
        assert_eq!(err.kind(), "invalid_input");
        assert!(!dest.exists(), "拒绝时不应落文件");
    }

    #[test]
    fn export_outcome_serializes_for_command_boundary() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let dest = tmp.path().join("picked.json");
        let outcome = export_to_file(&series(1), &dest, ExportFormat::Json).expect("导出");
        // 命令返回值经 Tauri 序列化给前端：字段名即前端契约
        let json = serde_json::to_value(&outcome).expect("序列化");
        for key in ["path", "filename", "rows"] {
            assert!(json.get(key).is_some(), "应含字段 {key}: {json}");
        }
        assert_eq!(json["rows"], 1);
        let back: ExportOutcome = serde_json::from_value(json).expect("往返");
        assert_eq!(back, outcome);
    }

    #[test]
    fn overwrite_required_only_for_existing_files() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let missing = tmp.path().join("none.csv");
        assert!(!overwrite_required(&missing), "不存在不算冲突");
        assert!(!overwrite_required(tmp.path()), "目录不算冲突");
        std::fs::write(&missing, "x").expect("造文件");
        assert!(overwrite_required(&missing), "已存在文件应需确认");
    }

    #[test]
    fn confirm_declined_keeps_existing_file_untouched() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let dest = tmp.path().join("save-as.csv");
        std::fs::write(&dest, "旧内容").expect("造既有文件");
        // confirm 返回 false → 用户取消：不落盘、原文件不变、无临时残留
        let outcome =
            export_symbol_request_confirm("600519", "csv", &dest, |_| false).expect("取消不是错误");
        assert!(outcome.is_none(), "取消应返回 None");
        assert_eq!(std::fs::read_to_string(&dest).expect("读原文件"), "旧内容");
        assert!(!tmp_sibling(&dest).exists(), "取消不应留临时文件");
    }

    #[test]
    fn confirm_accepted_replaces_existing_file() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let dest = tmp.path().join("save-as.csv");
        std::fs::write(&dest, "旧内容").expect("造既有文件");
        let outcome = export_symbol_request_confirm("600519", "csv", &dest, |_| true)
            .expect("确认覆盖")
            .expect("应返回结果");
        assert_eq!(outcome.rows, crate::market::DEFAULT_BARS);
        let body = std::fs::read_to_string(&dest).expect("读新文件");
        assert_ne!(body, "旧内容", "应已覆盖");
        assert!(body.contains("600519"), "新内容应含标的: {body:.40}");
    }

    #[test]
    fn confirm_skipped_when_no_conflict() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let dest = tmp.path().join("fresh.csv");
        let called = std::cell::Cell::new(false);
        let outcome = export_symbol_request_confirm("600519", "csv", &dest, |_| {
            called.set(true);
            false
        })
        .expect("无冲突直接导出")
        .expect("应返回结果");
        assert!(!called.get(), "无冲突不应弹确认");
        assert!(dest.exists());
        assert_eq!(outcome.rows, crate::market::DEFAULT_BARS);
    }
}
