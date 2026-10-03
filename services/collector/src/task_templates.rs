//! 采集任务模板（architecture §24「任务描述：以 YAML/JSON 定义每个数据源
//! ——URL、请求参数、解析策略、刷新频率」的落地面）
//!
//! - 形态：单文件或目录（非递归）下的 `*.yaml` / `*.yml` / `*.json`，顶层
//!   `version` + `tasks:` 列表；目录按文件名序装载，跨文件任务 ID 不得重复。
//! - 校验分两段：**装载期**（[`load_path`]——纯格式/取值/ID 唯一性，disabled
//!   任务同样校验）与**提交期**（[`TaskTemplate::into_task_definition`]——
//!   symbols 覆盖仅对 ashare/hkshare/usshare 生效这类与 `TaskSource` 相关的约束）。
//! - 接线：`ALPHA_COLLECTOR_TASKS` 指向模板文件或目录即装载登记（见 main.rs），
//!   未设置时零行为变化；示例见 `config/collector.tasks.yaml`。
//!
//! 解析复用既有 `config` crate（yaml-rust/serde_json 均已入锁，零新增依赖），
//! 经 `serde_json::Value` 两跳反序列化，保证 `deny_unknown_fields` 等严格
//! serde 语义生效（config 自带反序列化器对未知字段静默忽略，不适合配置面）。

use crate::types::{
    CompressionType, DataFormat, ParseRule, ParserConfig, ParserType, RequestConfig, StorageConfig,
    StorageType, TaskConfig, TaskDefinition, TaskPriority, TaskSource,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// 支持的模板扩展名（architecture 口径 YAML/JSON，不含 config crate 能读的
/// 其余格式——任务定义走声明面，刻意收窄）
const SUPPORTED_EXTS: [&str; 3] = ["yaml", "yml", "json"];

/// 与 main_simple::parse_task_source 支持的 source_type 同集（那边是权威，
/// 这里提前拦给出更清晰的文件级报错）
const SUPPORTED_SOURCE_TYPES: [&str; 8] = [
    "ashare", "a-share", "hkshare", "hk-share", "usshare", "us-share", "news", "custom",
];

/// 模板装载错误
#[derive(Debug, Error)]
pub enum TemplateError {
    #[error("任务模板路径不可读 {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("任务模板解析失败 {path}: {msg}")]
    Parse { path: String, msg: String },
    #[error("任务模板校验失败 {path} · {task}: {msg}")]
    Invalid {
        path: String,
        task: String,
        msg: String,
    },
    #[error("任务ID重复: {ids}")]
    DuplicateId { ids: String },
}

/// 模板文件顶层结构
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskTemplateFile {
    /// 模板格式版本（当前仅 1；缺省视为 1）
    pub version: Option<u32>,
    /// 任务列表（至少一项）
    pub tasks: Vec<TaskTemplate>,
}

/// 单个采集任务模板（架构口径四要素：URL / 请求参数 / 解析策略 / 刷新频率）
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskTemplate {
    /// 任务唯一 ID：`[a-z0-9][a-z0-9_-]{0,63}`，跨文件不得重复
    pub id: String,
    /// 任务名称（非空）
    pub name: String,
    /// 任务类型：ashare / hkshare / usshare / news / custom（大小写不敏感）
    pub source_type: String,
    /// 数据源 URL（必须 http/https）
    pub url: String,
    /// false 时装载校验但不提交
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub description: Option<String>,
    /// HTTP 方法（缺省 GET）
    #[serde(default)]
    pub method: Option<String>,
    /// 请求参数（进 RequestConfig.params）
    #[serde(default)]
    pub params: HashMap<String, String>,
    /// 请求头
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// 请求体
    #[serde(default)]
    pub body: Option<String>,
    /// 标的列表（仅 ashare/hkshare/usshare 支持覆盖，替换默认标的）
    #[serde(default)]
    pub symbols: Option<Vec<String>>,
    /// 刷新频率：5/6 段 cron 表达式（6 段含秒位，如 `0 */5 * * * *`）
    #[serde(default)]
    pub schedule: Option<String>,
    /// critical / high / medium / low / background（缺省 medium）
    #[serde(default)]
    pub priority: Option<String>,
    /// 超时秒数（>=1）
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// 最大重试次数（缺省 3）
    #[serde(default)]
    pub max_retries: Option<u32>,
    /// 同源请求间隔毫秒（>=1，缺省 1000）
    #[serde(default)]
    pub request_interval_ms: Option<u64>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 解析策略（缺省 JSON 解析 / JSON 格式）
    #[serde(default)]
    pub parser: TemplateParser,
    /// 存储策略（缺省内存存储）
    #[serde(default)]
    pub storage: TemplateStorage,
}

fn default_true() -> bool {
    true
}

/// 解析策略模板段
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateParser {
    /// html / json / xml / csv / regex / javascript / python（大小写不敏感）
    #[serde(rename = "type")]
    parser_type: Option<String>,
    /// json / xml / csv / tsv / parquet / avro / text / binary
    format: Option<String>,
    field_mapping: Option<HashMap<String, String>>,
    rules: Option<Vec<ParseRule>>,
}

/// 存储策略模板段
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateStorage {
    /// memory / file / database / message_queue / object_storage
    #[serde(rename = "type")]
    storage_type: Option<String>,
    /// 存储目标标识（非 memory 时必填非空）
    target: Option<String>,
    table: Option<String>,
    /// 批量大小（>=1）
    batch_size: Option<usize>,
    /// gzip / brotli / lz4 / snappy / zstd
    compression: Option<String>,
}

/// 装载模板路径（文件或目录），完成解析 + 装载期校验 + 跨文件 ID 去重。
/// 返回含 disabled 任务在内的全部模板（提交期再过滤）。
pub fn load_path(path: &Path) -> Result<Vec<TaskTemplate>, TemplateError> {
    let meta = std::fs::metadata(path).map_err(|e| TemplateError::Io {
        path: path.display().to_string(),
        source: e,
    })?;

    let files: Vec<PathBuf> = if meta.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
            .map_err(|e| TemplateError::Io {
                path: path.display().to_string(),
                source: e,
            })?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .and_then(|e| e.to_str())
                        .map(|e| SUPPORTED_EXTS.contains(&e.to_ascii_lowercase().as_str()))
                        .unwrap_or(false)
            })
            .collect();
        // 目录装载按文件名序，提交顺序确定可复现
        entries.sort();
        if entries.is_empty() {
            return Err(TemplateError::Invalid {
                path: path.display().to_string(),
                task: "(目录)".to_string(),
                msg: "目录内没有 *.yaml / *.yml / *.json 模板文件".to_string(),
            });
        }
        entries
    } else {
        vec![path.to_path_buf()]
    };

    let mut templates = Vec::new();
    let mut seen_ids: HashMap<String, String> = HashMap::new();
    let mut duplicates: Vec<String> = Vec::new();
    for file in &files {
        for template in load_file(file)? {
            let origin = file.display().to_string();
            if let Some(first) = seen_ids.get(&template.id) {
                duplicates.push(format!("{}（{} 与 {}）", template.id, first, origin));
            } else {
                seen_ids.insert(template.id.clone(), origin);
            }
            templates.push(template);
        }
    }
    if !duplicates.is_empty() {
        return Err(TemplateError::DuplicateId {
            ids: duplicates.join("、"),
        });
    }
    Ok(templates)
}

/// 解析单个模板文件（扩展名限定 yaml/yml/json）
fn load_file(path: &Path) -> Result<Vec<TaskTemplate>, TemplateError> {
    let display = path.display().to_string();
    let ext_ok = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| SUPPORTED_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false);
    if !ext_ok {
        return Err(TemplateError::Parse {
            path: display,
            msg: format!("不支持的扩展名（仅 {}）", SUPPORTED_EXTS.join("/")),
        });
    }

    // config 负责按扩展名选 yaml/json 解析；两跳经 serde_json::Value 走
    // 严格 serde 语义（deny_unknown_fields / 缺字段报错）
    let parsed: TaskTemplateFile = config::Config::builder()
        .add_source(config::File::from(path.to_path_buf()))
        .build()
        .map_err(|e| TemplateError::Parse {
            path: display.clone(),
            msg: e.to_string(),
        })
        .and_then(|cfg| {
            let value: serde_json::Value =
                cfg.try_deserialize().map_err(|e| TemplateError::Parse {
                    path: display.clone(),
                    msg: e.to_string(),
                })?;
            serde_json::from_value(value).map_err(|e| TemplateError::Parse {
                path: display.clone(),
                msg: e.to_string(),
            })
        })?;

    match parsed.version {
        None | Some(1) => {}
        Some(v) => {
            return Err(TemplateError::Invalid {
                path: display,
                task: "(文件)".to_string(),
                msg: format!("不支持的模板版本 {v}（当前仅 1）"),
            })
        }
    }
    if parsed.tasks.is_empty() {
        return Err(TemplateError::Invalid {
            path: display,
            task: "(文件)".to_string(),
            msg: "tasks 列表为空——未定义任何任务".to_string(),
        });
    }

    for (idx, template) in parsed.tasks.iter().enumerate() {
        let label = if template.id.is_empty() {
            format!("第{}项", idx + 1)
        } else {
            template.id.clone()
        };
        template.validate().map_err(|msg| TemplateError::Invalid {
            path: display.clone(),
            task: label,
            msg,
        })?;
    }
    Ok(parsed.tasks)
}

impl TaskTemplate {
    /// 装载期校验（与 TaskSource 无关的取值约束；disabled 任务同样过检）
    pub fn validate(&self) -> Result<(), String> {
        validate_id(&self.id)?;
        if self.name.trim().is_empty() {
            return Err("name 不能为空".to_string());
        }
        if !SUPPORTED_SOURCE_TYPES.contains(&self.source_type.to_ascii_lowercase().as_str()) {
            return Err(format!(
                "source_type 「{}」无效，可选 {}",
                self.source_type,
                SUPPORTED_SOURCE_TYPES.join("/")
            ));
        }
        validate_url(&self.url)?;
        if let Some(method) = &self.method {
            if method.trim().is_empty() {
                return Err("method 不能为空".to_string());
            }
        }
        if let Some(symbols) = &self.symbols {
            if symbols.is_empty() {
                return Err("symbols 不能为空列表（不需要就省略该字段）".to_string());
            }
            if symbols.iter().any(|s| s.trim().is_empty()) {
                return Err("symbols 内不能有空标的".to_string());
            }
        }
        if let Some(schedule) = &self.schedule {
            validate_schedule(schedule)?;
        }
        if let Some(priority) = &self.priority {
            parse_priority(Some(priority)).map_err(|_| {
                format!("priority 「{priority}」无效，可选 critical/high/medium/low/background")
            })?;
        }
        if let Some(timeout) = self.timeout_secs {
            if timeout == 0 {
                return Err("timeout_secs 必须 >= 1".to_string());
            }
        }
        if let Some(interval) = self.request_interval_ms {
            if interval == 0 {
                return Err("request_interval_ms 必须 >= 1".to_string());
            }
        }
        self.parser.validate()?;
        self.storage.validate()?;
        Ok(())
    }

    /// 转为调度面任务定义（提交期：source 相关约束在此收口）。
    /// 未覆盖字段沿用与 HTTP API 提交路径一致的缺省（RequestConfig 等 Default）。
    pub fn into_task_definition(self, mut source: TaskSource) -> Result<TaskDefinition, String> {
        if let Some(symbols) = &self.symbols {
            match &mut source {
                TaskSource::AShare { symbols: slot, .. }
                | TaskSource::HKShare { symbols: slot, .. }
                | TaskSource::USShare { symbols: slot, .. } => *slot = symbols.clone(),
                _ => {
                    return Err(format!(
                    "任务 {}：source_type 「{}」不支持 symbols 覆盖（仅 ashare/hkshare/usshare）",
                    self.id, self.source_type
                ))
                }
            }
        }
        let priority = parse_priority(self.priority.as_deref())?;

        let request = RequestConfig {
            url: self.url,
            method: self.method.unwrap_or_else(|| "GET".to_string()),
            params: self.params,
            headers: self.headers,
            body: self.body,
            request_interval: self.request_interval_ms.unwrap_or(1000),
            ..Default::default()
        };

        let parser = ParserConfig {
            parser_type: match self.parser.parser_type.as_deref() {
                Some(t) => parse_parser_type(t)?,
                None => ParserType::JSON,
            },
            data_format: match self.parser.format.as_deref() {
                Some(f) => parse_data_format(f)?,
                None => DataFormat::JSON,
            },
            field_mapping: self.parser.field_mapping.unwrap_or_default(),
            rules: self.parser.rules.unwrap_or_default(),
        };

        // memory 存储未指明目标时缺省 "default"，与 HTTP API 提交路径对齐
        // （非 memory 类型在 validate 已强制要求非空 target，此处只补 Memory）
        let storage = StorageConfig {
            storage_type: match self.storage.storage_type.as_deref() {
                Some(t) => parse_storage_type(t)?,
                None => StorageType::Memory,
            },
            target: self.storage.target.unwrap_or_else(|| "default".to_string()),
            table: self.storage.table,
            batch_size: Some(self.storage.batch_size.unwrap_or(1000)),
            compression: match self.storage.compression.as_deref() {
                Some(c) => Some(parse_compression(c)?),
                None => None,
            },
        };

        let mut task = TaskDefinition::new(self.id, source, self.name);
        task.description = self.description;
        task.schedule = self.schedule;
        task.priority = priority;
        task.config = TaskConfig {
            request,
            parser,
            storage,
            notification: None,
        };
        task.timeout = self.timeout_secs;
        task.tags = self.tags;
        if let Some(max) = self.max_retries {
            task.retry_policy.max_retries = max;
        }
        Ok(task)
    }
}

impl TemplateParser {
    fn validate(&self) -> Result<(), String> {
        if let Some(t) = &self.parser_type {
            parse_parser_type(t)?;
        }
        if let Some(f) = &self.format {
            parse_data_format(f)?;
        }
        if let Some(rules) = &self.rules {
            if rules.iter().any(|r| r.name.trim().is_empty()) {
                return Err("parser.rules 内每条规则必须有非空 name".to_string());
            }
        }
        Ok(())
    }
}

impl TemplateStorage {
    fn validate(&self) -> Result<(), String> {
        let storage_type = match &self.storage_type {
            Some(t) => {
                let parsed = parse_storage_type(t)?;
                Some(parsed)
            }
            None => None,
        };
        if let Some(size) = self.batch_size {
            if size == 0 {
                return Err("storage.batch_size 必须 >= 1".to_string());
            }
        }
        if let Some(target) = &self.target {
            if target.trim().is_empty() {
                return Err("storage.target 不能为空字符串".to_string());
            }
        }
        // 非 memory 存储必须指明目标，否则运行期无处可写
        if let Some(t) = storage_type {
            if t != StorageType::Memory
                && self
                    .target
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or("")
                    .is_empty()
            {
                return Err(format!(
                    "storage.type 为 {} 时 storage.target 必填",
                    self.storage_type.as_deref().unwrap_or_default()
                ));
            }
        }
        if let Some(c) = &self.compression {
            parse_compression(c)?;
        }
        Ok(())
    }
}

/// ID 规则：小写字母或数字开头，仅 `[a-z0-9_-]`，长度 1..=64
fn validate_id(id: &str) -> Result<(), String> {
    let valid = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-';
    let mut chars = id.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit() => {}
        _ => {
            return Err(format!(
                "id 「{id}」非法：须以小写字母或数字开头（仅小写字母/数字/_/-，长度 1..=64）"
            ))
        }
    }
    if id.chars().count() > 64 || !id.chars().all(valid) {
        return Err(format!("id 「{id}」非法：仅小写字母/数字/_/-，长度 1..=64"));
    }
    Ok(())
}

fn validate_url(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("url 非法（{e}）：{url}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(()),
        scheme => Err(format!("url 协议必须为 http/https，当前为 {scheme}")),
    }
}

/// 轻量 cron 形检（非完整 cron 语义解析）：5/6 段、每段非空、字符集限
/// `[A-Za-z0-9*/?,\-#LW]`——目的是拦住手误而非替代调度器解析
fn validate_schedule(schedule: &str) -> Result<(), String> {
    let fields: Vec<&str> = schedule.split_whitespace().collect();
    if fields.len() != 5 && fields.len() != 6 {
        return Err(format!(
            "schedule 「{schedule}」须为 5/6 段 cron 表达式（6 段含秒位，如 `0 */5 * * * *`）"
        ));
    }
    let allowed = |c: char| c.is_ascii_alphanumeric() || "*/?,-#LW".contains(c);
    if let Some(bad) = schedule
        .chars()
        .find(|c| !allowed(*c) && !c.is_whitespace())
    {
        return Err(format!("schedule 「{schedule}」含非法字符 {bad:?}"));
    }
    Ok(())
}

/// 与 main_simple::parse_priority 同值域，但未知值显式报错而非静默降级
/// （配置面手误不应悄悄变成 medium）
fn parse_priority(priority: Option<&str>) -> Result<TaskPriority, String> {
    match priority.map(str::to_ascii_lowercase).as_deref() {
        None => Ok(TaskPriority::Medium),
        Some("critical") => Ok(TaskPriority::Critical),
        Some("high") => Ok(TaskPriority::High),
        Some("medium") => Ok(TaskPriority::Medium),
        Some("low") => Ok(TaskPriority::Low),
        Some("background") => Ok(TaskPriority::Background),
        Some(other) => Err(format!(
            "priority 「{other}」无效，可选 critical/high/medium/low/background"
        )),
    }
}

fn parse_parser_type(input: &str) -> Result<ParserType, String> {
    match input.to_ascii_lowercase().as_str() {
        "html" => Ok(ParserType::HTML),
        "json" => Ok(ParserType::JSON),
        "xml" => Ok(ParserType::XML),
        "csv" => Ok(ParserType::CSV),
        "regex" => Ok(ParserType::Regex),
        "javascript" => Ok(ParserType::JavaScript),
        "python" => Ok(ParserType::Python),
        other => Err(format!(
            "parser.type 「{other}」无效，可选 html/json/xml/csv/regex/javascript/python"
        )),
    }
}

fn parse_data_format(input: &str) -> Result<DataFormat, String> {
    match input.to_ascii_lowercase().as_str() {
        "json" => Ok(DataFormat::JSON),
        "xml" => Ok(DataFormat::XML),
        "csv" => Ok(DataFormat::CSV),
        "tsv" => Ok(DataFormat::TSV),
        "parquet" => Ok(DataFormat::Parquet),
        "avro" => Ok(DataFormat::Avro),
        "text" => Ok(DataFormat::Text),
        "binary" => Ok(DataFormat::Binary),
        other => Err(format!(
            "parser.format 「{other}」无效，可选 json/xml/csv/tsv/parquet/avro/text/binary"
        )),
    }
}

fn parse_storage_type(input: &str) -> Result<StorageType, String> {
    match input.to_ascii_lowercase().as_str() {
        "memory" => Ok(StorageType::Memory),
        "file" => Ok(StorageType::File),
        "database" => Ok(StorageType::Database),
        "message_queue" => Ok(StorageType::MessageQueue),
        "object_storage" => Ok(StorageType::ObjectStorage),
        other => Err(format!(
            "storage.type 「{other}」无效，可选 memory/file/database/message_queue/object_storage"
        )),
    }
}

fn parse_compression(input: &str) -> Result<CompressionType, String> {
    match input.to_ascii_lowercase().as_str() {
        "gzip" => Ok(CompressionType::Gzip),
        "brotli" => Ok(CompressionType::Brotli),
        "lz4" => Ok(CompressionType::LZ4),
        "snappy" => Ok(CompressionType::Snappy),
        "zstd" => Ok(CompressionType::Zstd),
        other => Err(format!(
            "storage.compression 「{other}」无效，可选 gzip/brotli/lz4/snappy/zstd"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::main_simple::SimpleCollector;
    use crate::types::{AShareDataSource, NewsDataSource};

    const VALID_YAML: &str = r#"
version: 1
tasks:
  - id: ashare-kline
    name: A股日K线
    source_type: ashare
    url: "https://push2his.eastmoney.com/api/qt/stock/kline/get"
    symbols: ["600519", "000001"]
    params:
      klt: "101"
      fqt: "1"
    schedule: "0 30 9 * * 1-5"
    priority: high
    timeout_secs: 30
    max_retries: 5
    request_interval_ms: 2000
    tags: [kline, daily]
    parser:
      type: json
      format: json
      field_mapping:
        close: close
      rules:
        - name: price
          regex: '"close":([0-9.]+)'
    storage:
      type: database
      target: timescale
      table: kline_daily
      batch_size: 500
  - id: sina-news
    name: 新浪要闻
    source_type: news
    url: "https://feed.mix.sina.com.cn/api/roll/get"
    schedule: "0 */10 * * * *"
"#;

    /// 与 VALID_YAML 首任务等价的 JSON 形态（跨格式等价性锚）
    const EQUIV_JSON_TASK: &str = r#"{
  "version": 1,
  "tasks": [{
    "id": "ashare-kline",
    "name": "A股日K线",
    "source_type": "ashare",
    "url": "https://push2his.eastmoney.com/api/qt/stock/kline/get",
    "symbols": ["600519", "000001"],
    "params": {"klt": "101", "fqt": "1"},
    "schedule": "0 30 9 * * 1-5",
    "priority": "high",
    "timeout_secs": 30,
    "max_retries": 5,
    "request_interval_ms": 2000,
    "tags": ["kline", "daily"],
    "parser": {"type": "json", "format": "json", "field_mapping": {"close": "close"},
               "rules": [{"name": "price", "regex": "\"close\":([0-9.]+)"}]},
    "storage": {"type": "database", "target": "timescale", "table": "kline_daily", "batch_size": 500}
  }]
}"#;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "alpha-collector-templates-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("创建临时目录");
        dir
    }

    fn write(path: &Path, content: &str) {
        std::fs::write(path, content).expect("写模板文件");
    }

    fn a_share_source() -> TaskSource {
        TaskSource::AShare {
            source: AShareDataSource::EastMoney,
            symbols: vec!["000001".to_string()],
        }
    }

    #[test]
    fn loads_yaml_file_with_full_fields() {
        let dir = temp_dir();
        let file = dir.join("tasks.yaml");
        write(&file, VALID_YAML);
        let templates = load_path(&file).expect("装载成功");
        assert_eq!(templates.len(), 2);

        let first = &templates[0];
        assert_eq!(first.id, "ashare-kline");
        assert_eq!(
            first.symbols,
            Some(vec!["600519".to_string(), "000001".to_string()])
        );
        assert_eq!(first.params.get("klt").map(String::as_str), Some("101"));
        assert_eq!(first.schedule.as_deref(), Some("0 30 9 * * 1-5"));
        assert_eq!(first.priority.as_deref(), Some("high"));
        assert_eq!(first.storage.target.as_deref(), Some("timescale"));
        assert_eq!(first.storage.batch_size, Some(500));
        assert_eq!(first.parser.parser_type.as_deref(), Some("json"));
        assert!(first.enabled);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn json_and_yaml_produce_equivalent_tasks() {
        let source = a_share_source();
        let yaml_task = load_path_str(VALID_YAML, "t.yaml")
            .expect("yaml 装载")
            .remove(0)
            .into_task_definition(source.clone())
            .expect("yaml 转换");
        let json_task = load_path_str(EQUIV_JSON_TASK, "t.json")
            .expect("json 装载")
            .remove(0)
            .into_task_definition(source)
            .expect("json 转换");

        assert_eq!(yaml_task.id, json_task.id);
        assert_eq!(yaml_task.name, json_task.name);
        assert_eq!(yaml_task.config.request.url, json_task.config.request.url);
        assert_eq!(
            yaml_task.config.request.params,
            json_task.config.request.params
        );
        assert_eq!(
            yaml_task.config.request.request_interval,
            json_task.config.request.request_interval
        );
        assert_eq!(yaml_task.config.parser, json_task.config.parser);
        assert_eq!(yaml_task.config.storage, json_task.config.storage);
        assert_eq!(yaml_task.priority, json_task.priority);
        assert_eq!(yaml_task.schedule, json_task.schedule);
        assert_eq!(yaml_task.source, json_task.source);
    }

    /// 直接从字符串内容装载（落盘后走同一条 load_path 路径）
    fn load_path_str(content: &str, file_name: &str) -> Result<Vec<TaskTemplate>, TemplateError> {
        let dir = temp_dir();
        let file = dir.join(file_name);
        write(&file, content);
        let result = load_path(&file);
        std::fs::remove_dir_all(&dir).ok();
        result
    }

    #[test]
    fn rejects_unknown_source_type() {
        let err = load_path_str(
            "tasks:\n  - id: a\n    name: a\n    source_type: martian\n    url: https://x.io\n",
            "bad.yaml",
        )
        .expect_err("应拒绝未知 source_type");
        assert!(
            err.to_string().contains("source_type"),
            "报错应指明字段：{err}"
        );
    }

    #[test]
    fn rejects_duplicate_ids_within_file() {
        let content = VALID_YAML.replace("sina-news", "ashare-kline");
        let err = load_path_str(&content, "dup.yaml").expect_err("应拒绝文件内重复 ID");
        assert!(
            matches!(err, TemplateError::DuplicateId { .. }),
            "实际：{err}"
        );
    }

    #[test]
    fn rejects_duplicate_ids_across_files() {
        let dir = temp_dir();
        write(&dir.join("a.yaml"), VALID_YAML);
        write(
            &dir.join("b.json"),
            r#"{"tasks": [{"id": "sina-news", "name": "x", "source_type": "news",
                 "url": "https://x.io"}]}"#,
        );
        let err = load_path(&dir).expect_err("应拒绝跨文件重复 ID");
        match err {
            TemplateError::DuplicateId { ids } => {
                assert!(ids.contains("sina-news"));
                assert!(ids.contains("a.yaml"));
                assert!(ids.contains("b.json"));
            }
            other => panic!("应为 DuplicateId，实际 {other:?}"),
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rejects_invalid_url() {
        for url in ["ftp://x.io", "not-a-url", "https://"] {
            let content = format!(
                "tasks:\n  - id: a\n    name: a\n    source_type: custom\n    url: \"{url}\"\n"
            );
            let err = load_path_str(&content, "u.yaml").expect_err("应拒绝非法 url");
            assert!(
                err.to_string().contains("url"),
                "url={url} 的报错应指明字段：{err}"
            );
        }
    }

    #[test]
    fn rejects_invalid_priority_parser_and_storage() {
        let base = "tasks:\n  - id: a\n    name: a\n    source_type: custom\n    url: https://x.io";
        for extra in [
            "\n    priority: urgent",
            "\n    parser:\n      type: yaml",
            "\n    storage:\n      type: tape",
            "\n    storage:\n      type: database",
            "\n    storage:\n      compression: zip",
            "\n    request_interval_ms: 0",
            "\n    timeout_secs: 0",
            "\n    symbols: []",
        ] {
            let Err(err) = load_path_str(&format!("{base}{extra}\n"), "v.yaml") else {
                panic!("应拒绝 {extra:?}")
            };
            assert!(!err.to_string().is_empty());
        }
    }

    #[test]
    fn schedule_takes_five_or_six_cron_fields() {
        for ok in ["0 9 * * 1-5", "0 */10 * * * *", "30 8-16/2 * * 1-5"] {
            let content = format!(
                "tasks:\n  - id: a\n    name: a\n    source_type: custom\n    url: https://x.io\n    schedule: \"{ok}\"\n"
            );
            let Ok(loaded) = load_path_str(&content, "s.yaml") else {
                panic!("合法 schedule {ok} 应装载")
            };
            assert_eq!(loaded.len(), 1);
        }
        for bad in ["0 9 *", "0 9 * * * * *", "0 9 * * * ;", "每五分钟"] {
            let content = format!(
                "tasks:\n  - id: a\n    name: a\n    source_type: custom\n    url: https://x.io\n    schedule: \"{bad}\"\n"
            );
            let Err(err) = load_path_str(&content, "s.yaml") else {
                panic!("非法 schedule {bad} 应被拒")
            };
            assert!(err.to_string().contains("schedule"), "{bad} 报错：{err}");
        }
    }

    #[test]
    fn rejects_unknown_top_or_task_field() {
        let err = load_path_str(
            "version: 1\nshedule: x\ntasks:\n  - id: a\n    name: a\n    source_type: custom\n    url: https://x.io\n",
            "f.yaml",
        )
        .expect_err("未知顶层字段应被拒");
        assert!(err.to_string().contains("unknown field"), "实际：{err}");

        let err = load_path_str(
            "tasks:\n  - id: a\n    name: a\n    source_type: custom\n    url: https://x.io\n    shedule: x\n",
            "f.yaml",
        )
        .expect_err("未知任务字段应被拒");
        assert!(err.to_string().contains("unknown field"), "实际：{err}");
    }

    #[test]
    fn rejects_invalid_id_name_and_version() {
        for bad_id in ["UpperCase", "-leading", "带中文", ""] {
            let content = format!(
                "tasks:\n  - id: \"{bad_id}\"\n    name: a\n    source_type: custom\n    url: https://x.io\n"
            );
            assert!(
                load_path_str(&content, "i.yaml").is_err(),
                "id={bad_id:?} 应被拒"
            );
        }
        assert!(load_path_str(
            "tasks:\n  - id: a\n    name: \" \"\n    source_type: custom\n    url: https://x.io\n",
            "n.yaml"
        )
        .is_err());
        assert!(load_path_str(
            "version: 2\ntasks:\n  - id: a\n    name: a\n    source_type: custom\n    url: https://x.io\n",
            "v.yaml"
        )
        .is_err());
    }

    #[test]
    fn empty_and_missing_paths_error() {
        assert!(load_path_str("tasks: []\n", "e.yaml").is_err());
        assert!(matches!(
            load_path(Path::new("/nonexistent/alpha-templates.yaml")),
            Err(TemplateError::Io { .. })
        ));
        assert!(load_path_str("", "empty.yaml").is_err());
        assert!(matches!(
            load_path_str("tasks:\n  - id: a\n", "x.toml"),
            Err(TemplateError::Parse { .. })
        ));
    }

    #[test]
    fn directory_loading_is_sorted_and_ext_filtered() {
        let dir = temp_dir();
        write(&dir.join("b-second.yaml"), VALID_YAML);
        write(
            &dir.join("a-first.json"),
            r#"{"tasks": [{"id": "aa-first", "name": "x", "source_type": "custom",
                 "url": "https://x.io"}]}"#,
        );
        write(&dir.join("notes.txt"), "不是模板");
        let templates = load_path(&dir).expect("目录装载");
        // txt 不入装载：仅 2 个模板文件、共 3 个任务（b-second.yaml 含 2 个）
        assert_eq!(templates.len(), 3);
        assert_eq!(templates[0].id, "aa-first", "按文件名序：a-first.json 在前");
        assert_eq!(templates[1].id, "ashare-kline");
        assert_eq!(templates[2].id, "sina-news", "文件内保持声明顺序");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn minimal_task_maps_to_api_path_defaults() {
        let templates = load_path_str(
            "tasks:\n  - id: bare\n    name: 最小任务\n    source_type: custom\n    url: https://x.io/api\n",
            "m.yaml",
        )
        .expect("最小模板装载");
        let task = templates[0]
            .clone()
            .into_task_definition(TaskSource::Custom {
                source_type: "custom".to_string(),
                endpoint: "https://x.io/api".to_string(),
                params: HashMap::new(),
            })
            .expect("转换");
        assert_eq!(task.config.request.method, "GET");
        assert_eq!(task.config.request.request_interval, 1000);
        assert_eq!(task.config.parser.parser_type, ParserType::JSON);
        assert_eq!(task.config.storage.storage_type, StorageType::Memory);
        assert_eq!(
            task.config.storage.target, "default",
            "memory 缺省目标与 API 路径一致"
        );
        assert_eq!(task.retry_policy.max_retries, 3);
        assert_eq!(task.priority, TaskPriority::Medium);
        assert_eq!(task.status, crate::types::TaskStatus::Pending);
    }

    #[test]
    fn symbols_replace_default_and_reject_unsupported_source() {
        let templates = load_path_str(VALID_YAML, "s.yaml").expect("装载");
        let task = templates[0]
            .clone()
            .into_task_definition(a_share_source())
            .expect("ashare 支持 symbols");
        match task.source {
            TaskSource::AShare { symbols, .. } => {
                assert_eq!(symbols, vec!["600519".to_string(), "000001".to_string()]);
            }
            other => panic!("source 应保持 AShare，实际 {other:?}"),
        }

        let news = templates[1].clone();
        let mut news_with_symbols = news;
        news_with_symbols.symbols = Some(vec!["600519".to_string()]);
        let news_source = TaskSource::News {
            sources: vec![NewsDataSource::Sina],
            keywords: vec![],
            languages: vec![],
        };
        assert!(news_with_symbols.into_task_definition(news_source).is_err());
    }

    #[test]
    fn parse_rules_default_required_to_false() {
        let templates = load_path_str(VALID_YAML, "r.yaml").expect("装载");
        let task = templates[0]
            .clone()
            .into_task_definition(a_share_source())
            .expect("转换");
        assert_eq!(task.config.parser.rules.len(), 1);
        assert_eq!(task.config.parser.rules[0].name, "price");
        assert!(!task.config.parser.rules[0].required, "required 缺省 false");
    }

    #[tokio::test]
    async fn submit_registers_enabled_and_skips_disabled() {
        let workspace = temp_dir();
        let templates_dir = workspace.join("templates");
        std::fs::create_dir_all(&templates_dir).expect("建模板目录");
        write(
            &templates_dir.join("tasks.yaml"),
            &VALID_YAML.replace("  - id: sina-news", "  - id: sina-news\n    enabled: false"),
        );

        let collector = SimpleCollector::new(&workspace);
        let mut events = collector.subscribe_events();
        let (submitted, skipped) = collector
            .submit_task_templates(&templates_dir)
            .await
            .expect("提交模板");
        assert_eq!((submitted, skipped), (1, 1));

        let stats = collector.get_task_stats().await;
        assert_eq!(stats.total, 1, "disabled 任务不入登记");
        assert_eq!(stats.running, 0);

        // 事件面：仅登记的任务发 TaskSubmitted
        match events.try_recv() {
            Ok(crate::main_simple::CollectorEvent::TaskSubmitted { task_id, .. }) => {
                assert_eq!(task_id, "ashare-kline");
            }
            other => panic!("应收到 TaskSubmitted，实际 {other:?}"),
        }
        assert!(events.try_recv().is_err(), "disabled 任务不发事件");
        std::fs::remove_dir_all(&workspace).ok();
    }

    #[tokio::test]
    async fn submit_surfaces_template_errors_as_string() {
        let workspace = temp_dir();
        let file = workspace.join("tasks.yaml");
        write(
            &file,
            "tasks:\n  - id: a\n    name: a\n    source_type: martian\n    url: https://x.io\n",
        );
        let collector = SimpleCollector::new(&workspace);
        let err = collector
            .submit_task_templates(&file)
            .await
            .expect_err("未知 source_type 应报错");
        assert!(err.contains("source_type"), "报错信息：{err}");
        std::fs::remove_dir_all(&workspace).ok();
    }

    /// 优先级大小写不敏感（HIGH == high）
    #[test]
    fn priority_matching_is_case_insensitive() {
        let templates = load_path_str(
            "tasks:\n  - id: a\n    name: a\n    source_type: custom\n    url: https://x.io\n    priority: HIGH\n",
            "p.yaml",
        )
        .expect("装载");
        let task = templates[0]
            .clone()
            .into_task_definition(TaskSource::Custom {
                source_type: "custom".to_string(),
                endpoint: "https://x.io".to_string(),
                params: HashMap::new(),
            })
            .expect("转换");
        assert_eq!(task.priority, TaskPriority::High);
    }

    /// 兜底：validate 侧 HashSet/HashMap 路径无 panic（全量字段合法场景回归）
    #[test]
    fn validate_covers_all_branches_without_panic() {
        let templates = load_path_str(VALID_YAML, "c.yaml").expect("装载");
        for t in &templates {
            assert!(t.validate().is_ok());
        }
    }
}
