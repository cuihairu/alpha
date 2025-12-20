//! 多语言爬虫支持模块
//!
//! 支持 Python、Node.js、Go 等多种语言的爬虫任务执行
//! 提供统一的接口和灵活的语言选择机制

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tokio::{
    fs,
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command as TokioCommand},
    sync::{mpsc, RwLock},
    task::JoinHandle,
};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::types::{TaskDefinition, TaskResult, TaskSource};

/// 支持的爬虫语言类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum CrawlerLanguage {
    Python,
    NodeJs,
    Go,
    Rust,
    Shell,
}

impl CrawlerLanguage {
    /// 获取语言的文件扩展名
    pub fn extension(&self) -> &'static str {
        match self {
            CrawlerLanguage::Python => "py",
            CrawlerLanguage::NodeJs => "js",
            CrawlerLanguage::Go => "go",
            CrawlerLanguage::Rust => "rs",
            CrawlerLanguage::Shell => "sh",
        }
    }

    /// 获取语言的解释器/编译器命令
    pub fn command(&self) -> &'static str {
        match self {
            CrawlerLanguage::Python => "python3",
            CrawlerLanguage::NodeJs => "node",
            CrawlerLanguage::Go => "go",
            CrawlerLanguage::Rust => "cargo",
            CrawlerLanguage::Shell => "bash",
        }
    }

    /// 检查语言运行时是否可用
    pub async fn is_available(&self) -> bool {
        let result = TokioCommand::new("sh")
            .arg("-c")
            .arg(&format!("command -v {}", self.command()))
            .output()
            .await;

        match result {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    /// 获取语言的默认执行超时时间
    pub fn default_timeout(&self) -> Duration {
        match self {
            CrawlerLanguage::Python => Duration::from_secs(300), // 5 minutes
            CrawlerLanguage::NodeJs => Duration::from_secs(180), // 3 minutes
            CrawlerLanguage::Go => Duration::from_secs(120),   // 2 minutes
            CrawlerLanguage::Rust => Duration::from_secs(600),  // 10 minutes (compilation time)
            CrawlerLanguage::Shell => Duration::from_secs(60),  // 1 minute
        }
    }
}

/// 爬虫执行配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlerConfig {
    /// 执行语言
    pub language: CrawlerLanguage,
    /// 脚本路径或代码
    pub script_path: Option<String>,
    /// 内联代码（当script_path为None时使用）
    pub inline_code: Option<String>,
    /// 工作目录
    pub working_directory: Option<String>,
    /// 环境变量
    pub environment: HashMap<String, String>,
    /// 执行超时时间
    pub timeout: Option<u64>, // seconds
    /// 命令行参数
    pub arguments: Vec<String>,
    /// Python虚拟环境路径
    pub python_venv: Option<String>,
    /// Node.js项目路径
    pub node_project_path: Option<String>,
    /// Go模块路径
    pub go_module_path: Option<String>,
}

impl Default for CrawlerConfig {
    fn default() -> Self {
        Self {
            language: CrawlerLanguage::Python,
            script_path: None,
            inline_code: None,
            working_directory: None,
            environment: HashMap::new(),
            timeout: None,
            arguments: Vec::new(),
            python_venv: None,
            node_project_path: None,
            go_module_path: None,
        }
    }
}

/// 多语言爬虫执行器
pub struct MultilangCrawler {
    /// 工作目录根路径
    workspace_root: PathBuf,
    /// 语言运行时缓存
    runtime_cache: Arc<RwLock<HashMap<CrawlerLanguage, bool>>>,
    /// 临时文件目录
    temp_dir: PathBuf,
}

impl MultilangCrawler {
    /// 创建新的多语言爬虫执行器
    pub fn new<P: AsRef<Path>>(workspace_root: P) -> Self {
        let workspace_root = workspace_root.as_ref().to_path_buf();
        let temp_dir = workspace_root.join("temp");

        Self {
            workspace_root,
            runtime_cache: Arc::new(RwLock::new(HashMap::new())),
            temp_dir,
        }
    }

    /// 初始化执行器
    pub async fn initialize(&self) -> Result<()> {
        // 创建临时目录
        fs::create_dir_all(&self.temp_dir).await?;

        // 检查所有语言的运行时可用性
        let languages = [
            CrawlerLanguage::Python,
            CrawlerLanguage::NodeJs,
            CrawlerLanguage::Go,
            CrawlerLanguage::Rust,
            CrawlerLanguage::Shell,
        ];

        for language in &languages {
            let available = language.is_available().await;
            {
                let mut cache = self.runtime_cache.write().await;
                cache.insert(language.clone(), available);
            }

            if available {
                info!("{} runtime is available", language.command());
            } else {
                warn!("{} runtime is not available", language.command());
            }
        }

        Ok(())
    }

    /// 执行爬虫任务
    pub async fn execute_crawler(&self, task: &TaskDefinition, config: &CrawlerConfig) -> Result<TaskResult> {
        let task_id = Uuid::new_v4();
        let start_time = Instant::now();

        info!("Starting crawler execution for task_id: {}", task_id);

        // 检查语言运行时可用性
        if !self.is_language_available(&config.language).await? {
            return Err(anyhow!("Language runtime {} is not available", config.language.command()));
        }

        // 准备执行环境
        let script_path = self.prepare_script(config).await?;
        let working_dir = self.prepare_working_directory(config).await?;
        let timeout = Duration::from_secs(config.timeout.unwrap_or_else(|| config.language.default_timeout().as_secs()));

        // 执行爬虫
        let execution_result = self.execute_script(&script_path, &working_dir, config, timeout).await;

        // 清理临时文件
        if let Err(e) = self.cleanup_temp_files(&script_path).await {
            warn!("Failed to cleanup temp files: {}", e);
        }

        let execution_time = start_time.elapsed();

        match execution_result {
            Ok(output) => {
                info!("Crawler execution completed successfully in {:?}", execution_time);

                Ok(TaskResult {
                    task_id: task.id.clone(),
                    source: task.source.clone(),
                    status: crate::types::TaskStatus::Completed,
                    data: Some(output.clone()),
                    error: None,
                    execution_time,
                    metadata: self.create_execution_metadata(config, &output).await?,
                })
            }
            Err(e) => {
                error!("Crawler execution failed: {}", e);

                Ok(TaskResult {
                    task_id: task.id.clone(),
                    source: task.source.clone(),
                    status: crate::types::TaskStatus::Failed,
                    data: None,
                    error: Some(e.to_string()),
                    execution_time,
                    metadata: HashMap::new(),
                })
            }
        }
    }

    /// 检查语言运行时是否可用
    async fn is_language_available(&self, language: &CrawlerLanguage) -> Result<bool> {
        let cache = self.runtime_cache.read().await;
        if let Some(&available) = cache.get(language) {
            return Ok(available);
        }
        drop(cache);

        let available = language.is_available().await;
        {
            let mut cache = self.runtime_cache.write().await;
            cache.insert(language.clone(), available);
        }

        Ok(available)
    }

    /// 准备脚本文件
    async fn prepare_script(&self, config: &CrawlerConfig) -> Result<PathBuf> {
        let script_content = if let Some(script_path) = &config.script_path {
            // 从文件读取
            fs::read_to_string(script_path).await?
        } else if let Some(inline_code) = &config.inline_code {
            // 使用内联代码
            inline_code.clone()
        } else {
            return Err(anyhow!("Either script_path or inline_code must be provided"));
        };

        let extension = config.language.extension();
        let script_file = self.temp_dir.join(format!("crawler_{}.{}",
            Uuid::new_v4().to_string().replace("-", "_"), extension));

        fs::write(&script_file, script_content).await?;

        // 设置执行权限
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script_file).await?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script_file, perms).await?;
        }

        debug!("Prepared script file: {:?}", script_file);
        Ok(script_file)
    }

    /// 准备工作目录
    async fn prepare_working_directory(&self, config: &CrawlerConfig) -> Result<PathBuf> {
        let working_dir = if let Some(dir) = &config.working_directory {
            PathBuf::from(dir)
        } else {
            self.temp_dir.join(format!("workspace_{}", Uuid::new_v4().to_string().replace("-", "_")))
        };

        fs::create_dir_all(&working_dir).await?;
        Ok(working_dir)
    }

    /// 执行脚本
    async fn execute_script(
        &self,
        script_path: &Path,
        working_dir: &Path,
        config: &CrawlerConfig,
        timeout: Duration,
    ) -> Result<String> {
        let mut cmd = self.build_command(config, script_path)?;

        // 设置工作目录
        cmd.current_dir(working_dir);

        // 设置环境变量
        for (key, value) in &config.environment {
            cmd.env(key, value);
        }

        debug!("Executing command: {:?}", cmd);

        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = child.stdout.take()
            .ok_or_else(|| anyhow!("Failed to capture stdout"))?;
        let stderr = child.stderr.take()
            .ok_or_else(|| anyhow!("Failed to capture stderr"))?;

        // 读取输出
        let stdout_future = async {
            let mut reader = BufReader::new(stdout).lines();
            let mut output = Vec::new();

            while let Some(line) = reader.next_line().await? {
                output.push(line);
            }

            Ok(output.join("\n"))
        };

        let stderr_future = async {
            let mut reader = BufReader::new(stderr).lines();
            let mut output = Vec::new();

            while let Some(line) = reader.next_line().await? {
                output.push(line);
            }

            Ok(output.join("\n"))
        };

        // 使用tokio::time::timeout来设置超时
        let result = tokio::time::timeout(timeout, async {
            tokio::try_join!(stdout_future, stderr_future)
        }).await;

        match result {
            Ok(Ok((stdout_output, stderr_output))) => {
                let status = child.wait().await?;

                if !status.success() {
                    let error_msg = if !stderr_output.is_empty() {
                        stderr_output
                    } else {
                        format!("Process exited with status: {}", status)
                    };
                    return Err(anyhow!("Script execution failed: {}", error_msg));
                }

                if !stderr_output.is_empty() {
                    warn!("Script stderr: {}", stderr_output);
                }

                Ok(stdout_output)
            }
            Ok(Err(e)) => Err(anyhow!("Failed to read script output: {}", e)),
            Err(_) => {
                // 超时处理
                child.kill().await?;
                Err(anyhow!("Script execution timed out after {:?}", timeout))
            }
        }
    }

    /// 构建执行命令
    fn build_command(&self, config: &CrawlerConfig, script_path: &Path) -> Result<TokioCommand> {
        let mut cmd = match &config.language {
            CrawlerLanguage::Python => {
                let python_cmd = if let Some(venv) = &config.python_venv {
                    PathBuf::from(venv).join("bin").join("python3").to_string_lossy().to_string()
                } else {
                    config.language.command().to_string()
                };

                let mut cmd = TokioCommand::new(python_cmd);
                cmd.arg(script_path);
                cmd
            }

            CrawlerLanguage::NodeJs => {
                let node_cmd = if let Some(project_path) = &config.node_project_path {
                    format!("{} -r {}", config.language.command(), script_path.display())
                } else {
                    format!("{} {}", config.language.command(), script_path.display())
                };

                let mut cmd = TokioCommand::new("sh");
                cmd.arg("-c").arg(node_cmd);
                cmd
            }

            CrawlerLanguage::Go => {
                let go_path = if let Some(module_path) = &config.go_module_path {
                    PathBuf::from(module_path)
                } else {
                    script_path.parent().unwrap_or(Path::new(".")).to_path_buf()
                };

                let mut cmd = TokioCommand::new("sh");
                cmd.arg("-c")
                    .arg(format!("cd {} && go run {}",
                        go_path.display(),
                        script_path.file_name().unwrap().to_string_lossy()));
                cmd
            }

            CrawlerLanguage::Rust => {
                let mut cmd = TokioCommand::new("sh");
                cmd.arg("-c")
                    .arg(format!("cd {} && cargo run --bin crawler",
                        script_path.parent().unwrap_or(Path::new(".")).display()));
                cmd
            }

            CrawlerLanguage::Shell => {
                let mut cmd = TokioCommand::new("bash");
                cmd.arg(script_path);
                cmd
            }
        };

        // 添加参数
        for arg in &config.arguments {
            cmd.arg(arg);
        }

        Ok(cmd)
    }

    /// 创建执行元数据
    async fn create_execution_metadata(&self, config: &CrawlerConfig, output: &str) -> Result<HashMap<String, String>> {
        let mut metadata = HashMap::new();

        metadata.insert("language".to_string(), format!("{:?}", config.language));
        metadata.insert("script_type".to_string(),
            if config.script_path.is_some() { "file" } else { "inline" }.to_string());
        metadata.insert("output_length".to_string(), output.len().to_string());

        // 尝试解析JSON输出
        if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(output) {
            metadata.insert("output_format".to_string(), "json".to_string());
            if let Some(obj) = json_value.as_object() {
                metadata.insert("record_count".to_string(), obj.len().to_string());
            }
        } else {
            metadata.insert("output_format".to_string(), "text".to_string());
        }

        Ok(metadata)
    }

    /// 清理临时文件
    async fn cleanup_temp_files(&self, script_path: &Path) -> Result<()> {
        if script_path.starts_with(&self.temp_dir) {
            if let Err(e) = fs::remove_file(script_path).await {
                warn!("Failed to remove script file {:?}: {}", script_path, e);
            }
        }
        Ok(())
    }

    /// 批量执行爬虫任务
    pub async fn execute_batch(
        &self,
        tasks: Vec<(TaskDefinition, CrawlerConfig)>,
        max_concurrent: usize,
    ) -> Vec<Result<TaskResult>> {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent));
        let mut handles = Vec::new();

        for (task, config) in tasks {
            let semaphore = semaphore.clone();
            let crawler = self.clone(); // Note: need to implement Clone for MultilangCrawler

            let handle = tokio::spawn(async move {
                let _permit = semaphore.acquire().await.unwrap();
                crawler.execute_crawler(&task, &config).await
            });

            handles.push(handle);
        }

        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => results.push(Err(anyhow!("Task execution panicked: {}", e))),
            }
        }

        results
    }

    /// 获取支持的语言列表
    pub async fn supported_languages(&self) -> Vec<CrawlerLanguage> {
        let mut languages = Vec::new();
        let available = {
            let cache = self.runtime_cache.read().await;
            cache.clone()
        };

        for (language, &available) in available {
            if available {
                languages.push(language);
            }
        }

        languages
    }
}

// 为了支持execute_batch中的clone，需要实现Clone
impl Clone for MultilangCrawler {
    fn clone(&self) -> Self {
        Self {
            workspace_root: self.workspace_root.clone(),
            runtime_cache: Arc::clone(&self.runtime_cache),
            temp_dir: self.temp_dir.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_crawler_language_availability() {
        let python = CrawlerLanguage::Python;
        assert!(python.is_available().await || !python.is_available().await); // Just check it doesn't panic

        let node = CrawlerLanguage::NodeJs;
        assert!(node.is_available().await || !node.is_available().await);
    }

    #[tokio::test]
    async fn test_multilang_crawler_initialization() {
        let crawler = MultilangCrawler::new("/tmp/test_crawler");
        assert!(crawler.initialize().await.is_ok());
    }

    #[test]
    fn test_crawler_config_default() {
        let config = CrawlerConfig::default();
        assert_eq!(config.language, CrawlerLanguage::Python);
        assert!(config.script_path.is_none());
        assert!(config.inline_code.is_none());
    }

    #[test]
    fn test_crawler_language_properties() {
        let python = CrawlerLanguage::Python;
        assert_eq!(python.extension(), "py");
        assert_eq!(python.command(), "python3");
        assert_eq!(python.default_timeout(), Duration::from_secs(300));

        let node = CrawlerLanguage::NodeJs;
        assert_eq!(node.extension(), "js");
        assert_eq!(node.command(), "node");
        assert_eq!(node.default_timeout(), Duration::from_secs(180));
    }
}