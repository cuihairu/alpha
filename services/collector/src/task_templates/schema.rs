use crate::types::ParseRule;
use serde::Deserialize;
use std::collections::HashMap;
use thiserror::Error;

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
    /// 最大重试次数（缺省 3）。登记为任务元数据（写入
    /// `TaskDefinition.retry_policy`）；执行链当前单发、不消费该值
    /// （任务级重试不立项，见 docs/architecture.md 采集调度拍板），
    /// 实际失败补偿走请求级重试与 cron 周期性重跑
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
    pub(crate) parser_type: Option<String>,
    /// json / xml / csv / tsv / parquet / avro / text / binary
    pub(crate) format: Option<String>,
    pub(crate) field_mapping: Option<HashMap<String, String>>,
    pub(crate) rules: Option<Vec<ParseRule>>,
}

/// 存储策略模板段
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateStorage {
    /// memory / file / database / message_queue / object_storage
    #[serde(rename = "type")]
    pub(crate) storage_type: Option<String>,
    /// 存储目标标识（非 memory 时必填非空）
    pub(crate) target: Option<String>,
    pub(crate) table: Option<String>,
    /// 批量大小（>=1）
    pub(crate) batch_size: Option<usize>,
    /// gzip / brotli / lz4 / snappy / zstd
    pub(crate) compression: Option<String>,
}
