//! 多语言爬虫执行器（简化版）
//!
//! 负责把内联脚本（或脚本路径）写入临时文件并执行，返回结构化任务结果。

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    time::Instant,
};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tokio::process::Command as TokioCommand;

use crate::types::{TaskDefinition, TaskResult, TaskStatus};

/// 爬虫执行配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlerConfig {
    /// 编程语言
    pub language: CrawlerLanguage,
    /// 脚本路径（可选，如果提供 `inline_code` 则忽略此字段）
    pub script_path: Option<PathBuf>,
    /// 内联代码（可选）
    pub inline_code: Option<String>,
    /// 命令行参数
    pub arguments: Vec<String>,
    /// 超时时间（秒）
    pub timeout: Option<u64>,
    /// 环境变量
    pub environment: HashMap<String, String>,
    /// 工作目录（相对 workspace_root）
    pub working_directory: Option<PathBuf>,
}

impl Default for CrawlerConfig {
    fn default() -> Self {
        Self {
            language: CrawlerLanguage::Python,
            script_path: None,
            inline_code: None,
            arguments: Vec::new(),
            timeout: Some(3600),
            environment: HashMap::new(),
            working_directory: None,
        }
    }
}

/// 支持的执行语言
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum CrawlerLanguage {
    Python,
    NodeJs,
    Go,
    Rust,
    Shell,
}

impl CrawlerLanguage {
    pub fn command(&self) -> &'static str {
        match self {
            Self::Python => "python3",
            Self::NodeJs => "node",
            Self::Go => "go",
            Self::Rust => "cargo",
            Self::Shell => "bash",
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            Self::Python => "py",
            Self::NodeJs => "js",
            Self::Go => "go",
            Self::Rust => "rs",
            Self::Shell => "sh",
        }
    }
}

/// 多语言爬虫执行器
pub struct MultilangCrawler {
    workspace_root: PathBuf,
    temp_dir: PathBuf,
}

impl MultilangCrawler {
    pub fn new<P: AsRef<Path>>(workspace_root: P) -> Self {
        let workspace_root = workspace_root.as_ref().to_path_buf();
        let temp_dir = workspace_root.join("temp");
        Self {
            workspace_root,
            temp_dir,
        }
    }

    pub async fn initialize(&self) -> Result<()> {
        tokio::fs::create_dir_all(&self.temp_dir).await?;
        Ok(())
    }

    pub async fn execute_crawler(&self, task: &TaskDefinition, config: &CrawlerConfig) -> Result<TaskResult> {
        let now = chrono::Utc::now();

        let script_file = self.prepare_script_file(task, config).await?;
        let (status, stdout, stderr, exit_code, execution_secs) =
            self.execute_script(&script_file, config).await?;

        let (data, error) = if status == TaskStatus::Completed {
            (Some(parse_stdout_as_json_or_text(&stdout)), None)
        } else {
            (None, Some(stderr))
        };

        Ok(TaskResult {
            task_id: task.id.clone(),
            task_name: task.name.clone(),
            source: task.source.clone(),
            status,
            data,
            error,
            start_time: now,
            end_time: chrono::Utc::now(),
            execution_time: Some(execution_secs),
            metadata: HashMap::from([
                ("language".to_string(), serde_json::json!(config.language)),
                ("exit_code".to_string(), serde_json::json!(exit_code)),
                ("script_path".to_string(), serde_json::json!(script_file.display().to_string())),
            ]),
        })
    }

    async fn prepare_script_file(&self, task: &TaskDefinition, config: &CrawlerConfig) -> Result<PathBuf> {
        if let Some(inline_code) = &config.inline_code {
            let path = self
                .temp_dir
                .join(format!("{}_crawler.{}", sanitize_filename(&task.id), config.language.extension()));
            tokio::fs::write(&path, inline_code).await?;
            Ok(path)
        } else if let Some(script_path) = &config.script_path {
            Ok(self.workspace_root.join(script_path))
        } else {
            Err(anyhow!("missing script_path or inline_code"))
        }
    }

    async fn execute_script(
        &self,
        script_file: &Path,
        config: &CrawlerConfig,
    ) -> Result<(TaskStatus, String, String, Option<i32>, u64)> {
        let mut cmd = TokioCommand::new(config.language.command());

        match config.language {
            CrawlerLanguage::Go => {
                cmd.arg("run");
                cmd.arg(script_file);
            }
            CrawlerLanguage::Rust => {
                return Err(anyhow!("Rust execution is not supported in simple runner"));
            }
            _ => {
                cmd.arg(script_file);
            }
        }

        for arg in &config.arguments {
            cmd.arg(arg);
        }

        if let Some(working_dir) = &config.working_directory {
            cmd.current_dir(self.workspace_root.join(working_dir));
        } else {
            cmd.current_dir(&self.workspace_root);
        }

        for (key, value) in &config.environment {
            cmd.env(key, value);
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let start = Instant::now();
        let output = if let Some(timeout) = config.timeout {
            tokio::time::timeout(std::time::Duration::from_secs(timeout), cmd.output()).await??
        } else {
            cmd.output().await?
        };
        let execution_secs = start.elapsed().as_secs();

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code();
        let status = if output.status.success() {
            TaskStatus::Completed
        } else {
            TaskStatus::Failed
        };

        Ok((status, stdout, stderr, exit_code, execution_secs))
    }
}

fn parse_stdout_as_json_or_text(stdout: &str) -> serde_json::Value {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return serde_json::Value::Null;
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            return value;
        }
    }
    serde_json::json!({ "text": trimmed })
}

fn sanitize_filename(input: &str) -> String {
    input
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

