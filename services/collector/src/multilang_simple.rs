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

    pub async fn execute_crawler(
        &self,
        task: &TaskDefinition,
        config: &CrawlerConfig,
    ) -> Result<TaskResult> {
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
                (
                    "script_path".to_string(),
                    serde_json::json!(script_file.display().to_string()),
                ),
            ]),
        })
    }

    async fn prepare_script_file(
        &self,
        task: &TaskDefinition,
        config: &CrawlerConfig,
    ) -> Result<PathBuf> {
        if let Some(inline_code) = &config.inline_code {
            let path = self.temp_dir.join(format!(
                "{}_crawler.{}",
                sanitize_filename(&task.id),
                config.language.extension()
            ));
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
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AShareDataSource, TaskSource};

    fn a_share_task(id: &str) -> TaskDefinition {
        TaskDefinition::new(
            id,
            TaskSource::AShare {
                source: AShareDataSource::EastMoney,
                symbols: vec!["000001".to_string()],
            },
            "多语言爬虫任务",
        )
    }

    /// 临时工作目录（RAII）：panic 触发 unwinding 时 Drop 仍清场，
    /// 不在 /tmp 留陈旧目录（Codex 深审 2026-11-12 [m] 收敛）。
    struct TempWorkspace(PathBuf);

    impl TempWorkspace {
        fn new() -> Self {
            let dir =
                std::env::temp_dir().join(format!("alpha-crawler-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).expect("创建临时工作目录");
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempWorkspace {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    #[test]
    fn crawler_language_maps_command_and_extension() {
        assert_eq!(CrawlerLanguage::Python.command(), "python3");
        assert_eq!(CrawlerLanguage::Python.extension(), "py");
        assert_eq!(CrawlerLanguage::NodeJs.command(), "node");
        assert_eq!(CrawlerLanguage::NodeJs.extension(), "js");
        assert_eq!(CrawlerLanguage::Go.command(), "go");
        assert_eq!(CrawlerLanguage::Go.extension(), "go");
        assert_eq!(CrawlerLanguage::Rust.command(), "cargo");
        assert_eq!(CrawlerLanguage::Rust.extension(), "rs");
        assert_eq!(CrawlerLanguage::Shell.command(), "bash");
        assert_eq!(CrawlerLanguage::Shell.extension(), "sh");
    }

    #[test]
    fn default_config_uses_python_with_hour_timeout() {
        let config = CrawlerConfig::default();
        assert_eq!(config.language, CrawlerLanguage::Python);
        assert_eq!(config.timeout, Some(3600));
        assert_eq!(config.arguments, Vec::<String>::new());
        assert!(config.script_path.is_none());
        assert!(config.inline_code.is_none());
    }

    #[test]
    fn sanitize_filename_keeps_only_safe_chars() {
        assert_eq!(sanitize_filename("task-01_a"), "task-01_a");
        assert_eq!(sanitize_filename("a/b\\c:d"), "a_b_c_d");
        assert_eq!(sanitize_filename("中文 task"), "___task");
        assert_eq!(sanitize_filename(""), "");
    }

    #[test]
    fn parse_stdout_handles_json_object_array_and_fallback() {
        assert_eq!(
            parse_stdout_as_json_or_text(r#"{"price": 1.5}"#),
            serde_json::json!({"price": 1.5})
        );
        assert_eq!(
            parse_stdout_as_json_or_text("[1, 2]"),
            serde_json::json!([1, 2])
        );
        // 伪 JSON 开头但解析失败 → 退回 text 包装
        assert_eq!(
            parse_stdout_as_json_or_text("{not json"),
            serde_json::json!({"text": "{not json"})
        );
        assert_eq!(
            parse_stdout_as_json_or_text("plain text output"),
            serde_json::json!({"text": "plain text output"})
        );
        assert_eq!(
            parse_stdout_as_json_or_text("  \n\t "),
            serde_json::Value::Null
        );
    }

    #[tokio::test]
    async fn inline_bash_script_completes_with_json_data() {
        let ws = TempWorkspace::new();
        let crawler = MultilangCrawler::new(ws.path());
        crawler.initialize().await.expect("初始化临时目录");

        let mut config = CrawlerConfig {
            language: CrawlerLanguage::Shell,
            inline_code: Some(r#"echo '{"price": 10.5, "symbol": "sh600000"}'"#.to_string()),
            ..CrawlerConfig::default()
        };
        config.timeout = Some(30);
        let result = crawler
            .execute_crawler(&a_share_task("bash-ok"), &config)
            .await
            .expect("执行成功");

        assert_eq!(result.status, TaskStatus::Completed);
        assert!(result.error.is_none());
        assert_eq!(
            result.data,
            Some(serde_json::json!({"price": 10.5, "symbol": "sh600000"}))
        );
        assert_eq!(
            result.metadata.get("exit_code"),
            Some(&serde_json::json!(0))
        );
        assert_eq!(
            result.metadata.get("language"),
            Some(&serde_json::json!(CrawlerLanguage::Shell))
        );
        let script_path = result
            .metadata
            .get("script_path")
            .and_then(|v| v.as_str())
            .expect("script_path 元数据存在");
        // 任务 id "bash-ok" 的连字符是保留字符：文件名保持 bash-ok_crawler.sh
        assert!(script_path.ends_with("bash-ok_crawler.sh"));
        assert!(ws.path().join("temp").join("bash-ok_crawler.sh").exists());
    }

    #[tokio::test]
    async fn failing_script_returns_failed_status_with_stderr() {
        let ws = TempWorkspace::new();
        let crawler = MultilangCrawler::new(ws.path());
        crawler.initialize().await.expect("初始化临时目录");

        let config = CrawlerConfig {
            language: CrawlerLanguage::Shell,
            inline_code: Some("echo boom-stderr >&2; exit 3".to_string()),
            timeout: Some(30),
            ..CrawlerConfig::default()
        };
        let result = crawler
            .execute_crawler(&a_share_task("bash-fail"), &config)
            .await
            .expect("执行器本身不报错");

        assert_eq!(result.status, TaskStatus::Failed);
        assert!(result.data.is_none());
        let error = result.error.expect("stderr 进入 error 字段");
        assert!(error.contains("boom-stderr"));
        assert_eq!(
            result.metadata.get("exit_code"),
            Some(&serde_json::json!(3))
        );
    }

    #[tokio::test]
    async fn timeout_aborts_long_running_script() {
        let ws = TempWorkspace::new();
        let crawler = MultilangCrawler::new(ws.path());
        crawler.initialize().await.expect("初始化临时目录");

        let config = CrawlerConfig {
            language: CrawlerLanguage::Shell,
            inline_code: Some("sleep 30".to_string()),
            timeout: Some(1),
            ..CrawlerConfig::default()
        };
        let err = crawler
            .execute_crawler(&a_share_task("bash-slow"), &config)
            .await
            .expect_err("超时应产生错误");
        assert!(
            err.to_string().contains("elapsed") || err.to_string().contains("deadline"),
            "超时错误应指明 deadline：{}",
            err
        );
    }

    #[tokio::test]
    async fn missing_script_and_inline_code_is_rejected() {
        let ws = TempWorkspace::new();
        let crawler = MultilangCrawler::new(ws.path());
        crawler.initialize().await.expect("初始化临时目录");

        let config = CrawlerConfig {
            language: CrawlerLanguage::Shell,
            script_path: None,
            inline_code: None,
            timeout: Some(30),
            ..CrawlerConfig::default()
        };
        let err = crawler
            .execute_crawler(&a_share_task("no-script"), &config)
            .await
            .expect_err("缺脚本应报错");
        assert!(err
            .to_string()
            .contains("missing script_path or inline_code"));
    }

    #[tokio::test]
    async fn rust_language_is_not_supported_in_simple_runner() {
        let ws = TempWorkspace::new();
        let crawler = MultilangCrawler::new(ws.path());
        crawler.initialize().await.expect("初始化临时目录");

        let config = CrawlerConfig {
            language: CrawlerLanguage::Rust,
            inline_code: Some("fn main() {}".to_string()),
            timeout: Some(30),
            ..CrawlerConfig::default()
        };
        let err = crawler
            .execute_crawler(&a_share_task("rust-run"), &config)
            .await
            .expect_err("Rust 应显式拒绝");
        assert!(err.to_string().contains("not supported"));
    }

    #[tokio::test]
    async fn script_path_resolves_relative_to_workspace_root() {
        let ws = TempWorkspace::new();
        let crawler = MultilangCrawler::new(ws.path());
        crawler.initialize().await.expect("初始化临时目录");
        let script_dir = ws.path().join("scripts");
        std::fs::create_dir_all(&script_dir).expect("创建 scripts 目录");
        std::fs::write(script_dir.join("echo_task.sh"), "echo path-script").expect("写脚本文件");

        let config = CrawlerConfig {
            language: CrawlerLanguage::Shell,
            script_path: Some(PathBuf::from("scripts/echo_task.sh")),
            timeout: Some(30),
            ..CrawlerConfig::default()
        };
        let result = crawler
            .execute_crawler(&a_share_task("path-script"), &config)
            .await
            .expect("相对路径脚本执行成功");

        assert_eq!(result.status, TaskStatus::Completed);
        assert_eq!(
            result.data,
            Some(serde_json::json!({"text": "path-script"}))
        );
        // script_path 分支不落临时目录（复用仓库内脚本）
        assert!(!ws
            .path()
            .join("temp")
            .join("path_script_crawler.sh")
            .exists());
    }
}
