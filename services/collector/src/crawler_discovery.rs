//! 自动爬虫发现和集成模块
//!
//! 提供智能的爬虫项目发现、注册、配置和集成功能
//! 支持多种编程语言和框架的开源爬虫项目

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::time::sleep;

use crate::distributed_crawler::{OpenSourceCrawler, CrawlerType};

/// 爬虫发现配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlerDiscoveryConfig {
    /// 搜索路径列表
    pub search_paths: Vec<PathBuf>,
    /// 搜索深度限制
    pub max_depth: usize,
    /// 是否启用自动发现
    pub auto_discovery: bool,
    /// 扫描间隔（秒）
    pub scan_interval: u64,
    /// 支持的文件类型
    pub supported_extensions: Vec<String>,
    /// 忽略的目录模式
    pub ignore_patterns: Vec<String>,
}

impl Default for CrawlerDiscoveryConfig {
    fn default() -> Self {
        Self {
            search_paths: vec![
                PathBuf::from("crawlers"),
                PathBuf::from("scrapy_projects"),
                PathBuf::from("web_scrapers"),
                PathBuf::from("data_collectors"),
                PathBuf::from(".local/share/crawlers"),
                PathBuf::from("/opt/crawlers"),
            ],
            max_depth: 3,
            auto_discovery: true,
            scan_interval: 300, // 5分钟
            supported_extensions: vec![
                "py".to_string(), "js".to_string(), "go".to_string(),
                "rs".to_string(), "java".to_string(), "php".to_string(),
                "rb".to_string(), "cpp".to_string(), "c".to_string(),
                "json".to_string(), "yaml".to_string(), "yml".to_string(),
                "toml".to_string(), "ini".to_string(), "cfg".to_string(),
            ],
            ignore_patterns: vec![
                "node_modules".to_string(),
                "target".to_string(),
                "build".to_string(),
                "dist".to_string(),
                "__pycache__".to_string(),
                ".git".to_string(),
                ".svn".to_string(),
                "venv".to_string(),
                "env".to_string(),
            ],
        }
    }
}

/// 爬虫项目信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlerProject {
    /// 项目名称
    pub name: String,
    /// 显示名称
    pub display_name: String,
    /// 项目类型
    pub crawler_type: CrawlerType,
    /// 项目路径
    pub path: PathBuf,
    /// 主要编程语言
    pub language: String,
    /// 框架
    pub framework: String,
    /// 版本
    pub version: Option<String>,
    /// 描述
    pub description: Option<String>,
    /// 作者
    pub author: Option<String>,
    /// 仓库URL
    pub repository_url: Option<String>,
    /// 许可证
    pub license: Option<String>,
    /// 支持的数据源
    pub supported_sources: Vec<String>,
    /// 依赖关系
    pub dependencies: Vec<String>,
    /// 配置文件
    pub config_files: Vec<PathBuf>,
    /// 启动命令
    pub start_command: Option<String>,
    /// 命令模板
    pub command_template: Option<String>,
    /// 是否需要Python
    pub requires_python: bool,
    /// 是否需要Node.js
    pub requires_nodejs: bool,
    /// 是否需要Go
    pub requires_go: bool,
    /// 是否需要Rust
    pub requires_rust: bool,
    /// 健康状态
    pub health_status: CrawlerHealthStatus,
    /// 最后扫描时间
    pub last_scanned: chrono::DateTime<chrono::Utc>,
}

/// 爬虫健康状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlerHealthStatus {
    /// 是否健康
    pub is_healthy: bool,
    /// 最后检查时间
    pub last_check: chrono::DateTime<chrono::Utc>,
    /// 错误信息
    pub errors: Vec<String>,
    /// 警告信息
    pub warnings: Vec<String>,
    /// 性能指标
    pub metrics: HashMap<String, serde_json::Value>,
}

impl Default for CrawlerHealthStatus {
    fn default() -> Self {
        Self {
            is_healthy: true,
            last_check: chrono::Utc::now(),
            errors: Vec::new(),
            warnings: Vec::new(),
            metrics: HashMap::new(),
        }
    }
}

/// 爬虫发现器接口
#[async_trait]
pub trait CrawlerDiscoverer {
    /// 发现爬虫项目
    async fn discover_crawlers(&self) -> Result<Vec<CrawlerProject>>;

    /// 验证爬虫项目
    async fn validate_crawler(&self, project: &CrawlerProject) -> Result<bool>;

    /// 获取爬虫项目信息
    async fn get_crawler_info(&self, path: &Path) -> Result<Option<CrawlerProject>>;

    /// 注册爬虫项目
    async fn register_crawler(&self, project: CrawlerProject) -> Result<()>;

    /// 取消注册爬虫项目
    async fn unregister_crawler(&self, name: &str) -> Result<()>;
}

/// 文件系统爬虫发现器
pub struct FilesystemCrawlerDiscoverer {
    config: CrawlerDiscoveryConfig,
    detectors: Vec<Box<dyn CrawlerDetector>>,
}

impl FilesystemCrawlerDiscoverer {
    pub fn new(config: CrawlerDiscoveryConfig) -> Self {
        let detectors: Vec<Box<dyn CrawlerDetector>> = vec![
            Box::new(ScrapyDetector::new()),
            Box::new(PuppeteerDetector::new()),
            Box::new(SeleniumDetector::new()),
            Box::new(NodeDetector::new()),
            Box::new(GoDetector::new()),
            Box::new(RustDetector::new()),
            Box::new(PythonDetector::new()),
            Box::new(GenericDetector::new()),
        ];

        Self {
            config,
            detectors,
        }
    }

    async fn scan_directory(&self, path: &Path, depth: usize) -> Result<Vec<PathBuf>> {
        if depth > self.config.max_depth {
            return Ok(Vec::new());
        }

        let mut projects = Vec::new();

        if !path.exists() {
            return Ok(projects);
        }

        let mut entries = fs::read_dir(path).await?;

        while let Some(entry) = entries.next_entry().await? {
            let entry_path = entry.path();

            // 跳过忽略的目录
            if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
                if self.config.ignore_patterns.iter().any(|pattern| name.contains(pattern)) {
                    continue;
                }
            }

            if entry_path.is_dir() {
                // 递归扫描子目录
                let sub_projects = self.scan_directory(&entry_path, depth + 1).await?;
                projects.extend(sub_projects);
            } else if entry_path.is_file() {
                // 检查文件扩展名
                if let Some(extension) = entry_path.extension().and_then(|e| e.to_str()) {
                    if self.config.supported_extensions.contains(&extension.to_string()) {
                        projects.push(entry_path);
                    }
                }
            }
        }

        Ok(projects)
    }
}

#[async_trait]
impl CrawlerDiscoverer for FilesystemCrawlerDiscoverer {
    async fn discover_crawlers(&self) -> Result<Vec<CrawlerProject>> {
        let mut all_projects = Vec::new();

        for search_path in &self.config.search_paths {
            println!("🔍 搜索爬虫项目: {:?}", search_path);

            let files = self.scan_directory(search_path, 0).await?;

            for file_path in files {
                // 尝试每个检测器
                for detector in &self.detectors {
                    if let Some(project) = detector.detect(&file_path).await? {
                        // 验证项目
                        if self.validate_crawler(&project).await? {
                            all_projects.push(project);
                        }
                        break; // 找到一个匹配的检测器就停止
                    }
                }
            }
        }

        // 去重
        let mut unique_projects = Vec::new();
        let mut seen_names = std::collections::HashSet::new();

        for project in all_projects {
            if seen_names.insert(project.name.clone()) {
                unique_projects.push(project);
            }
        }

        println!("📊 发现 {} 个爬虫项目", unique_projects.len());
        Ok(unique_projects)
    }

    async fn validate_crawler(&self, project: &CrawlerProject) -> Result<bool> {
        // 检查项目目录是否存在
        if !project.path.exists() {
            return Ok(false);
        }

        // 检查必要的配置文件是否存在
        if !project.config_files.iter().all(|f| f.exists()) {
            return Ok(false);
        }

        // 检查依赖是否满足
        if project.requires_python && !self.check_python_available().await? {
            return Ok(false);
        }

        if project.requires_nodejs && !self.check_nodejs_available().await? {
            return Ok(false);
        }

        if project.requires_go && !self.check_go_available().await? {
            return Ok(false);
        }

        if project.requires_rust && !self.check_rust_available().await? {
            return Ok(false);
        }

        // 尝试解析配置
        for config_file in &project.config_files {
            if let Err(e) = self.parse_config_file(config_file).await {
                eprintln!("配置文件解析失败 {:?}: {}", config_file, e);
                return Ok(false);
            }
        }

        Ok(true)
    }

    async fn get_crawler_info(&self, path: &Path) -> Result<Option<CrawlerProject>> {
        for detector in &self.detectors {
            if let Some(project) = detector.detect(path).await? {
                if self.validate_crawler(&project).await? {
                    return Ok(Some(project));
                }
            }
        }
        Ok(None)
    }

    async fn register_crawler(&self, project: CrawlerProject) -> Result<()> {
        // 这里可以将爬虫项目注册到中央存储
        println!("📝 注册爬虫项目: {}", project.name);

        // 保存到配置文件
        let config_path = PathBuf::from("discovered_crawlers.json");
        let mut existing_crawlers: Vec<CrawlerProject> = if config_path.exists() {
            let content = fs::read_to_string(&config_path).await?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Vec::new()
        };

        // 检查是否已存在
        if let Some(pos) = existing_crawlers.iter().position(|c| c.name == project.name) {
            existing_crawlers[pos] = project;
        } else {
            existing_crawlers.push(project);
        }

        let content = serde_json::to_string_pretty(&existing_crawlers)?;
        fs::write(&config_path, content).await?;

        Ok(())
    }

    async fn unregister_crawler(&self, name: &str) -> Result<()> {
        println!("🗑️ 取消注册爬虫项目: {}", name);

        let config_path = PathBuf::from("discovered_crawlers.json");
        if config_path.exists() {
            let content = fs::read_to_string(&config_path).await?;
            let mut existing_crawlers: Vec<CrawlerProject> = serde_json::from_str(&content)?;

            existing_crawlers.retain(|c| c.name != name);

            let content = serde_json::to_string_pretty(&existing_crawlers)?;
            fs::write(&config_path, content).await?;
        }

        Ok(())
    }
}

impl FilesystemCrawlerDiscoverer {
    async fn check_python_available(&self) -> Result<bool> {
        tokio::task::spawn_blocking(|| {
            Command::new("python3")
                .arg("--version")
                .output()
                .map(|_| true)
                .unwrap_or(false)
        }).await
    }

    async fn check_nodejs_available(&self) -> Result<bool> {
        tokio::task::spawn_blocking(|| {
            Command::new("node")
                .arg("--version")
                .output()
                .map(|_| true)
                .unwrap_or(false)
        }).await
    }

    async fn check_go_available(&self) -> Result<bool> {
        tokio::task::spawn_blocking(|| {
            Command::new("go")
                .arg("version")
                .output()
                .map(|_| true)
                .unwrap_or(false)
        }).await
    }

    async fn check_rust_available(&self) -> Result<bool> {
        tokio::task::spawn_blocking(|| {
            Command::new("rustc")
                .arg("--version")
                .output()
                .map(|_| true)
                .unwrap_or(false)
        }).await
    }

    async fn parse_config_file(&self, path: &Path) -> Result<()> {
        let content = fs::read_to_string(path).await?;

        // 尝试解析为JSON
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            serde_json::from_str::<serde_json::Value>(&content)?;
        }
        // 尝试解析为YAML
        else if path.extension().and_then(|s| s.to_str()) == Some("yaml") ||
                path.extension().and_then(|s| s.to_str()) == Some("yml") {
            // 这里需要添加yaml解析库
            // serde_yaml::from_str::<serde_json::Value>(&content)?;
        }
        // 尝试解析为TOML
        else if path.extension().and_then(|s| s.to_str()) == Some("toml") {
            // 这里需要添加toml解析库
            // toml::from_str::<serde_json::Value>(&content)?;
        }

        Ok(())
    }
}

/// 爬虫检测器接口
#[async_trait]
pub trait CrawlerDetector {
    /// 检测文件是否为爬虫项目
    async fn detect(&self, path: &Path) -> Result<Option<CrawlerProject>>;

    /// 支持的文件模式
    fn supported_patterns(&self) -> Vec<String>;

    /// 检测器名称
    fn name(&self) -> &str;
}

/// Scrapy爬虫检测器
pub struct ScrapyDetector;

impl ScrapyDetector {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CrawlerDetector for ScrapyDetector {
    async fn detect(&self, path: &Path) -> Result<Option<CrawlerProject>> {
        // 检查是否为scrapy.cfg文件
        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
            if filename == "scrapy.cfg" {
                let project_dir = path.parent().unwrap_or(path);

                // 检查是否有settings.py和spiders目录
                let settings_path = project_dir.join("settings.py");
                let spiders_dir = project_dir.join("spiders");

                if settings_path.exists() || spiders_dir.exists() {
                    return Ok(Some(CrawlerProject {
                        name: project_dir.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown_scrapy_project")
                            .to_string(),
                        display_name: format!("Scrapy Project - {}",
                            project_dir.file_name().and_then(|n| n.to_str()).unwrap_or("Unknown")),
                        crawler_type: CrawlerType::Scrapy,
                        path: project_dir.to_path_buf(),
                        language: "python".to_string(),
                        framework: "scrapy".to_string(),
                        version: None,
                        description: None,
                        author: None,
                        repository_url: None,
                        license: None,
                        supported_sources: vec![
                            "ashare".to_string(),
                            "cryptocurrency".to_string(),
                            "forex".to_string(),
                            "news".to_string()
                        ],
                        dependencies: vec!["scrapy".to_string(), "redis".to_string()],
                        config_files: vec![
                            settings_path,
                            path.to_path_buf()
                        ],
                        start_command: Some("scrapy crawl".to_string()),
                        command_template: Some("scrapy crawl {spider_name} -s LOG_LEVEL=INFO".to_string()),
                        requires_python: true,
                        requires_nodejs: false,
                        requires_go: false,
                        requires_rust: false,
                        health_status: CrawlerHealthStatus::default(),
                        last_scanned: chrono::Utc::now(),
                    }));
                }
            }
        }
        Ok(None)
    }

    fn supported_patterns(&self) -> Vec<String> {
        vec!["scrapy.cfg".to_string(), "settings.py".to_string()]
    }

    fn name(&self) -> &str {
        "scrapy"
    }
}

/// Puppeteer爬虫检测器
pub struct PuppeteerDetector;

impl PuppeteerDetector {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CrawlerDetector for PuppeteerDetector {
    async fn detect(&self, path: &Path) -> Result<Option<CrawlerProject>> {
        // 检查是否为包含puppeteer的JavaScript文件
        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
            if filename.ends_with(".js") {
                let content = fs::read_to_string(path).await?;

                // 检查文件内容是否包含puppeteer关键词
                if content.contains("puppeteer") ||
                   content.contains("const puppeteer") ||
                   content.contains("require('puppeteer')") ||
                   content.contains("import puppeteer") {

                    let project_dir = path.parent().unwrap_or(path);

                    return Ok(Some(CrawlerProject {
                        name: project_dir.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown_puppeteer_project")
                            .to_string(),
                        display_name: format!("Puppeteer Project - {}",
                            project_dir.file_name().and_then(|n| n.to_str()).unwrap_or("Unknown")),
                        crawler_type: CrawlerType::Puppeteer,
                        path: project_dir.to_path_buf(),
                        language: "javascript".to_string(),
                        framework: "puppeteer".to_string(),
                        version: None,
                        description: None,
                        author: None,
                        repository_url: None,
                        license: None,
                        supported_sources: vec![
                            "news".to_string(),
                            "social_media".to_string(),
                            "ashare".to_string(),
                            "hkshare".to_string()
                        ],
                        dependencies: vec!["puppeteer".to_string()],
                        config_files: vec![
                            path.to_path_buf()
                        ],
                        start_command: Some("node".to_string()),
                        command_template: Some("node {} --headless".to_string()),
                        requires_python: false,
                        requires_nodejs: true,
                        requires_go: false,
                        requires_rust: false,
                        health_status: CrawlerHealthStatus::default(),
                        last_scanned: chrono::Utc::now(),
                    }));
                }
            }
        }
        Ok(None)
    }

    fn supported_patterns(&self) -> Vec<String> {
        vec!["*.js".to_string(), "package.json".to_string()]
    }

    fn name(&self) -> &str {
        "puppeteer"
    }
}

/// Selenium爬虫检测器
pub struct SeleniumDetector;

impl SeleniumDetector {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CrawlerDetector for SeleniumDetector {
    async fn detect(&self, path: &Path) -> Result<Option<CrawlerProject>> {
        // 检查是否为包含selenium的Python文件
        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
            if filename.ends_with(".py") {
                let content = fs::read_to_string(path).await?;

                // 检查文件内容是否包含selenium关键词
                if content.contains("selenium") ||
                   content.contains("from selenium") ||
                   content.contains("import selenium") ||
                   content.contains("webdriver") {

                    let project_dir = path.parent().unwrap_or(path);

                    return Ok(Some(CrawlerProject {
                        name: project_dir.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown_selenium_project")
                            .to_string(),
                        display_name: format!("Selenium Project - {}",
                            project_dir.file_name().and_then(|n| n.to_str()).unwrap_or("Unknown")),
                        crawler_type: CrawlerType::Selenium,
                        path: project_dir.to_path_buf(),
                        language: "python".to_string(),
                        framework: "selenium".to_string(),
                        version: None,
                        description: None,
                        author: None,
                        repository_url: None,
                        license: None,
                        supported_sources: vec![
                            "ashare".to_string(),
                            "hkshare".to_string(),
                            "forex".to_string(),
                            "news".to_string()
                        ],
                        dependencies: vec!["selenium".to_string(), "beautifulsoup4".to_string()],
                        config_files: vec![
                            path.to_path_buf()
                        ],
                        start_command: Some("python".to_string()),
                        command_template: Some("python {} --headless".to_string()),
                        requires_python: true,
                        requires_nodejs: false,
                        requires_go: false,
                        requires_rust: false,
                        health_status: CrawlerHealthStatus::default(),
                        last_scanned: chrono::Utc::now(),
                    }));
                }
            }
        }
        Ok(None)
    }

    fn supported_patterns(&self) -> Vec<String> {
        vec!["*.py".to_string(), "requirements.txt".to_string()]
    }

    fn name(&self) -> &str {
        "selenium"
    }
}

/// Node.js爬虫检测器
pub struct NodeDetector;

impl NodeDetector {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CrawlerDetector for NodeDetector {
    async fn detect(&self, path: &Path) -> Result<Option<CrawlerProject>> {
        // 检查是否为package.json文件
        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
            if filename == "package.json" {
                let content = fs::read_to_string(path).await?;

                if let Ok(package_json) = serde_json::from_str::<serde_json::Value>(&content) {
                    // 检查是否有爬虫相关的依赖
                    let dependencies = package_json.get("dependencies")
                        .and_then(|d| d.as_object())
                        .map(|obj| obj.keys().collect::<Vec<_>>())
                        .unwrap_or_default();

                    let crawler_deps = ["cheerio", "axios", "puppeteer", "playwright", "request"];
                    if dependencies.iter().any(|dep| crawler_deps.contains(&dep.as_str())) {
                        let project_dir = path.parent().unwrap_or(path);

                        return Ok(Some(CrawlerProject {
                            name: project_dir.file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("unknown_node_project")
                                .to_string(),
                            display_name: format!("Node.js Project - {}",
                                project_dir.file_name().and_then(|n| n.to_str()).unwrap_or("Unknown")),
                            crawler_type: CrawlerType::NodeJs,
                            path: project_dir.to_path_buf(),
                            language: "javascript".to_string(),
                            framework: "node".to_string(),
                            version: package_json.get("version")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                            description: package_json.get("description")
                                .and_then(|d| d.as_str())
                                .map(|s| s.to_string()),
                            author: package_json.get("author")
                                .and_then(|a| a.as_str())
                                .map(|s| s.to_string()),
                            repository_url: package_json.get("repository")
                                .and_then(|r| r.as_str())
                                .map(|s| s.to_string()),
                            license: package_json.get("license")
                                .and_then(|l| l.as_str())
                                .map(|s| s.to_string()),
                            supported_sources: vec![
                                "cryptocurrency".to_string(),
                                "forex".to_string(),
                                "news".to_string()
                            ],
                            dependencies: dependencies.into_iter().map(|s| s.to_string()).collect(),
                            config_files: vec![path.to_path_buf()],
                            start_command: Some("node".to_string()),
                            command_template: Some("node index.js".to_string()),
                            requires_python: false,
                            requires_nodejs: true,
                            requires_go: false,
                            requires_rust: false,
                            health_status: CrawlerHealthStatus::default(),
                            last_scanned: chrono::Utc::now(),
                        }));
                    }
                }
            }
        }
        Ok(None)
    }

    fn supported_patterns(&self) -> Vec<String> {
        vec!["package.json".to_string(), "*.js".to_string()]
    }

    fn name(&self) -> &str {
        "node"
    }
}

/// Go爬虫检测器
pub struct GoDetector;

impl GoDetector {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CrawlerDetector for GoDetector {
    async fn detect(&self, path: &Path) -> Result<Option<CrawlerProject>> {
        // 检查是否为go.mod文件
        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
            if filename == "go.mod" {
                let content = fs::read_to_string(path).await?;

                // 检查是否有爬虫相关的依赖
                if content.contains("colly") || content.contains("chromedp") || content.contains("goquery") {
                    let project_dir = path.parent().unwrap_or(path);

                    return Ok(Some(CrawlerProject {
                        name: project_dir.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown_go_project")
                            .to_string(),
                        display_name: format!("Go Project - {}",
                            project_dir.file_name().and_then(|n| n.to_str()).unwrap_or("Unknown")),
                        crawler_type: CrawlerType::Go,
                        path: project_dir.to_path_buf(),
                        language: "go".to_string(),
                        framework: "go".to_string(),
                        version: None,
                        description: None,
                        author: None,
                        repository_url: None,
                        license: None,
                        supported_sources: vec![
                            "commodities".to_string(),
                            "forex".to_string(),
                            "economic_indicators".to_string()
                        ],
                        dependencies: vec!["colly".to_string()],
                        config_files: vec![path.to_path_buf()],
                        start_command: Some("go run".to_string()),
                        command_template: Some("go run main.go".to_string()),
                        requires_python: false,
                        requires_nodejs: false,
                        requires_go: true,
                        requires_rust: false,
                        health_status: CrawlerHealthStatus::default(),
                        last_scanned: chrono::Utc::now(),
                    }));
                }
            }
        }
        Ok(None)
    }

    fn supported_patterns(&self) -> Vec<String> {
        vec!["go.mod".to_string(), "*.go".to_string()]
    }

    fn name(&self) -> &str {
        "go"
    }
}

/// Rust爬虫检测器
pub struct RustDetector;

impl RustDetector {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CrawlerDetector for RustDetector {
    async fn detect(&self, path: &Path) -> Result<Option<CrawlerProject>> {
        // 检查是否为Cargo.toml文件
        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
            if filename == "Cargo.toml" {
                let content = fs::read_to_string(path).await?;

                // 检查是否有爬虫相关的依赖
                if content.contains("scraper") || content.contains("reqwest") || content.contains("tokio") {
                    let project_dir = path.parent().unwrap_or(path);

                    return Ok(Some(CrawlerProject {
                        name: project_dir.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown_rust_project")
                            .to_string(),
                        display_name: format!("Rust Project - {}",
                            project_dir.file_name().and_then(|n| n.to_str()).unwrap_or("Unknown")),
                        crawler_type: CrawlerType::Rust,
                        path: project_dir.to_path_buf(),
                        language: "rust".to_string(),
                        framework: "rust".to_string(),
                        version: None,
                        description: None,
                        author: None,
                        repository_url: None,
                        license: None,
                        supported_sources: vec![
                            "news".to_string(),
                            "research_reports".to_string(),
                            "economic_indicators".to_string()
                        ],
                        dependencies: vec!["scraper".to_string(), "reqwest".to_string()],
                        config_files: vec![path.to_path_buf()],
                        start_command: Some("cargo run".to_string()),
                        command_template: Some("cargo run --release".to_string()),
                        requires_python: false,
                        requires_nodejs: false,
                        requires_go: false,
                        requires_rust: true,
                        health_status: CrawlerHealthStatus::default(),
                        last_scanned: chrono::Utc::now(),
                    }));
                }
            }
        }
        Ok(None)
    }

    fn supported_patterns(&self) -> Vec<String> {
        vec!["Cargo.toml".to_string(), "*.rs".to_string()]
    }

    fn name(&self) -> &str {
        "rust"
    }
}

/// Python通用检测器
pub struct PythonDetector;

impl PythonDetector {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CrawlerDetector for PythonDetector {
    async fn detect(&self, path: &Path) -> Result<Option<CrawlerProject>> {
        // 检查是否为Python文件
        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
            if filename.ends_with(".py") && (filename.contains("crawler") || filename.contains("scraper")) {
                let content = fs::read_to_string(path).await?;

                // 检查是否包含爬虫相关的导入
                if content.contains("requests") || content.contains("beautifulsoup") || content.contains("lxml") {
                    let project_dir = path.parent().unwrap_or(path);

                    return Ok(Some(CrawlerProject {
                        name: project_dir.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown_python_project")
                            .to_string(),
                        display_name: format!("Python Project - {}",
                            project_dir.file_name().and_then(|n| n.to_str()).unwrap_or("Unknown")),
                        crawler_type: CrawlerType::BeautifulSoup,
                        path: project_dir.to_path_buf(),
                        language: "python".to_string(),
                        framework: "python".to_string(),
                        version: None,
                        description: None,
                        author: None,
                        repository_url: None,
                        license: None,
                        supported_sources: vec![
                            "forex".to_string(),
                            "economic_indicators".to_string(),
                            "commodities".to_string()
                        ],
                        dependencies: vec!["requests".to_string(), "beautifulsoup4".to_string()],
                        config_files: vec![path.to_path_buf()],
                        start_command: Some("python".to_string()),
                        command_template: Some("python {}".to_string()),
                        requires_python: true,
                        requires_nodejs: false,
                        requires_go: false,
                        requires_rust: false,
                        health_status: CrawlerHealthStatus::default(),
                        last_scanned: chrono::Utc::now(),
                    }));
                }
            }
        }
        Ok(None)
    }

    fn supported_patterns(&self) -> Vec<String> {
        vec!["*.py".to_string(), "requirements.txt".to_string()]
    }

    fn name(&self) -> &str {
        "python"
    }
}

/// 通用爬虫检测器
pub struct GenericDetector;

impl GenericDetector {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CrawlerDetector for GenericDetector {
    async fn detect(&self, path: &Path) -> Result<Option<CrawlerProject>> {
        // 检查是否为配置文件
        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
            if filename.contains("config") || filename.contains("setting") {
                let content = fs::read_to_string(path).await?;

                // 检查内容是否包含爬虫关键词
                let crawler_keywords = ["crawler", "spider", "scraper", "scrapy", "crawl", "puppeteer"];
                if crawler_keywords.iter().any(|keyword| content.to_lowercase().contains(keyword)) {
                    let project_dir = path.parent().unwrap_or(path);

                    return Ok(Some(CrawlerProject {
                        name: project_dir.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown_project")
                            .to_string(),
                        display_name: format!("Generic Project - {}",
                            project_dir.file_name().and_then(|n| n.to_str()).unwrap_or("Unknown")),
                        crawler_type: CrawlerType::Custom,
                        path: project_dir.to_path_buf(),
                        language: "unknown".to_string(),
                        framework: "custom".to_string(),
                        version: None,
                        description: Some("Auto-detected crawler project".to_string()),
                        author: None,
                        repository_url: None,
                        license: None,
                        supported_sources: vec!["unknown".to_string()],
                        dependencies: vec![],
                        config_files: vec![path.to_path_buf()],
                        start_command: None,
                        command_template: None,
                        requires_python: false,
                        requires_nodejs: false,
                        requires_go: false,
                        requires_rust: false,
                        health_status: CrawlerHealthStatus::default(),
                        last_scanned: chrono::Utc::now(),
                    }));
                }
            }
        }
        Ok(None)
    }

    fn supported_patterns(&self) -> Vec<String> {
        vec!["config.json".to_string(), "settings.json".to_string(), "*.toml".to_string()]
    }

    fn name(&self) -> &str {
        "generic"
    }
}

/// 自动爬虫发现和管理服务
pub struct CrawlerDiscoveryService {
    discoverer: FilesystemCrawlerDiscoverer,
    is_running: std::sync::atomic::AtomicBool,
}

impl CrawlerDiscoveryService {
    pub fn new(config: CrawlerDiscoveryConfig) -> Self {
        Self {
            discoverer: FilesystemCrawlerDiscoverer::new(config),
            is_running: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub async fn start_continuous_discovery(&self) -> Result<()> {
        self.is_running.store(true, std::sync::atomic::Ordering::SeqCst);

        println!("🚀 启动自动爬虫发现服务...");

        while self.is_running.load(std::sync::atomic::Ordering::SeqCst) {
            // 发现爬虫
            match self.discoverer.discover_crawlers().await {
                Ok(crawlers) => {
                    println!("📊 发现 {} 个爬虫项目", crawlers.len());

                    // 注册所有发现的爬虫
                    for crawler in crawlers {
                        if let Err(e) = self.discoverer.register_crawler(crawler).await {
                            eprintln!("注册爬虫失败: {}", e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("爬虫发现失败: {}", e);
                }
            }

            // 等待下次扫描
            sleep(Duration::from_secs(self.discoverer.config.scan_interval)).await;
        }

        println!("⏹️ 自动爬虫发现服务已停止");
        Ok(())
    }

    pub fn stop(&self) {
        self.is_running.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    pub async fn manual_discovery(&self) -> Result<Vec<CrawlerProject>> {
        println!("🔍 手动触发爬虫发现...");
        self.discoverer.discover_crawlers().await
    }
}