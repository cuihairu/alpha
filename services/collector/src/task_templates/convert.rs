use super::schema::{TaskTemplate, TemplateParser, TemplateStorage};
use crate::types::{
    CompressionType, DataFormat, ParserConfig, ParserType, RequestConfig, StorageConfig,
    StorageType, TaskConfig, TaskDefinition, TaskPriority, TaskSource,
};

/// 与 main_simple::parse_task_source 支持的 source_type 同集（那边是权威，
/// 这里提前拦给出更清晰的文件级报错）
const SUPPORTED_SOURCE_TYPES: [&str; 8] = [
    "ashare", "a-share", "hkshare", "hk-share", "usshare", "us-share", "news", "custom",
];

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

/// cron 校验复用 cron_scheduler::parse（5/6 段、域取值、步进、区间约束全收口
/// 一处——模板面与调度执行面语义永不脱节）
fn validate_schedule(schedule: &str) -> Result<(), String> {
    crate::cron_scheduler::parse(schedule)
        .map(|_| ())
        .map_err(|e| format!("schedule 「{schedule}」非法：{e}"))
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
