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
mod convert;
mod loader;
mod schema;
#[cfg(test)]
mod tests;

pub use loader::load_path;
pub use schema::{TaskTemplate, TaskTemplateFile, TemplateError, TemplateParser, TemplateStorage};
