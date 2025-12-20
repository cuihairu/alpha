//! 分布式爬虫集成系统
//!
//! 自动发现、整合和管理各种开源爬虫工具
//! 支持 Scrapy、Puppeteer、Selenium、BeautifulSoup 等主流爬虫框架

use std::{
    collections::HashMap,
    path::PathBuf,
    process::Command,
    time::Duration,
};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tokio::fs;
use tracing::{info, warn, debug};
use uuid::Uuid;

use super::{
    multilang_simple::{CrawlerConfig, CrawlerLanguage, MultilangCrawler},
    data_sources::*,
};

/// 开源爬虫项目配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenSourceCrawler {
    /// 项目名称
    pub name: String,
    /// 项目类型（Scrapy, Puppeteer, Selenium等）
    pub crawler_type: CrawlerType,
    /// 项目URL
    pub repository_url: String,
    /// 本地路径
    pub local_path: Option<PathBuf>,
    /// 配置文件路径
    pub config_path: Option<PathBuf>,
    /// 支持的数据源类型
    pub supported_sources: Vec<String>,
    /// 启动命令
    pub start_command: String,
    /// 参数模板
    pub command_template: String,
    /// 是否需要Python环境
    pub requires_python: bool,
    /// 是否需要Node.js环境
    pub requires_nodejs: bool,
    /// 默认配置
    pub default_config: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CrawlerType {
    /// Scrapy框架
    Scrapy,
    /// Puppeteer无头浏览器
    Puppeteer,
    /// Selenium浏览器自动化
    Selenium,
    /// BeautifulSoup解析
    BeautifulSoup,
    /// Cheerio解析
    Cheerio,
    /// Request直接请求
    DirectHttp,
    /// Playwright浏览器自动化
    Playwright,
}

/// 分布式爬虫管理器
pub struct DistributedCrawlerManager {
    /// 工作目录
    workspace_root: PathBuf,
    /// 爬虫项目缓存
    crawlers: HashMap<String, OpenSourceCrawler>,
    /// 运行中的爬虫实例
    running_instances: HashMap<String, CrawlerInstance>,
}

/// 运行中的爬虫实例
#[derive(Debug)]
pub struct CrawlerInstance {
    /// 项目名称
    pub project_name: String,
    /// 进程ID
    pub process_id: Option<u32>,
    /// 启动时间
    pub started_at: Option<std::time::Instant>,
    /// 配置
    pub config: CrawlerConfig,
    /// 数据源
    pub data_source: String,
    /// 输出文件路径
    pub output_file: PathBuf,
}

/// 任务分配结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAllocation {
    /// 任务ID
    pub task_id: String,
    /// 分配的爬虫
    pub crawler_name: String,
    /// 分配的数据源类型
    pub data_source_type: String,
    /// 估计执行时间
    pub estimated_duration: Duration,
    /// 分配原因
    pub allocation_reason: String,
}

impl DistributedCrawlerManager {
    pub fn new<P: AsRef<std::path::Path>>(workspace_root: P) -> Self {
        let workspace_root = workspace_root.as_ref().to_path_buf();

        Self {
            workspace_root,
            crawlers: HashMap::new(),
            running_instances: HashMap::new(),
        }
    }

    /// 自动发现爬虫项目
    pub async fn discover_crawlers(&mut self) -> Result<()> {
        info!("开始自动发现爬虫项目...");

        // 搜索常见的开源爬虫项目
        let search_paths = vec![
            self.workspace_root.join("crawlers"),
            self.workspace_root.join("scrapy_projects"),
            self.workspace_root.join("web_scrapers"),
            self.workspace_root.join("data_collectors"),
        ];

        for search_path in &search_paths {
            if search_path.exists() {
                info!("搜索目录: {:?}", search_path);

                let mut entries = match tokio::fs::read_dir(&search_path).await {
                    Ok(entries) => {
                        let mut found = false;
                        for entry in entries {
                            if let Ok(entry) = entry {
                                let path = entry.path();
                                if path.is_dir() {
                                    if let Some(crawler) = self.analyze_project_directory(&path).await? {
                                        info!("发现爬虫项目: {:?}", crawler.name);
                                        self.crawlers.insert(crawler.name.clone(), crawler);
                                        found = true;
                                    }
                                }
                            }
                        }

                        if found {
                            info!("在 {:?} 中发现了爬虫项目", search_path);
                        }
                    }
                    Err(e) => {
                        warn!("读取目录 {:?} 失败: {}", search_path, e);
                    }
                };
            }
        }

        info!("自动发现完成，共发现 {} 个爬虫项目", self.crawlers.len());
        Ok(())
    }

    /// 分析项目目录，判断是否为爬虫项目
    async fn analyze_project_directory(&self, project_path: &PathBuf) -> Result<Option<OpenSourceCrawler>> {
        let mut is_crawler = false;
        let mut crawler_name = None;
        let mut crawler_type = None;

        // 检查常见的爬虫项目文件
        let indicator_files = vec![
            "scrapy.cfg", "scrapy.py", "requirements.txt",
            "package.json", "Pipfile", "pyproject.toml",
            "puppeteer.config.js", "puppeteer.js",
            "selenium-webdriver.js", "nightwatch.js",
            "package-lock.json", "yarn.lock",
            "bs4.config.js", "cheerio.py", "app.js",
        ];

        // 检查是否包含爬虫特征文件
        for entry in tokio::fs::read_dir(project_path).await? {
            let entry_path = entry.path();
            if entry_path.is_file() {
                let file_name = entry_path.file_name()
                    .unwrap_or("")
                    .to_string_lossy();

                for indicator in &indicator_files {
                    if file_name.contains(indicator) {
                        is_crawler = true;
                        break;
                    }
                }

                // 根据文件名判断爬虫类型
                if !is_crawler {
                    if file_name == "scrapy.py" || file_name == "scrapy.cfg" {
                        crawler_type = Some(CrawlerType::Scrapy);
                    } else if file_name.contains("puppeteer") {
                        crawler_type = Some(CrawlerType::Puppeteer);
                    } else if file_name.contains("selenium") {
                        crawler_type = Some(CrawlerType::Selenium);
                    } else if file_name.contains("beautifulsoup") || file_name.contains("cheerio") {
                        crawler_type = Some(CrawlerType::BeautifulSoup);
                    } else if file_name.contains("playwright") {
                        crawler_type = Some(CrawlerType::Playwright);
                    }
                }
            }
        }

        if is_crawler {
            // 提取项目名称
            let project_name = project_path
                .file_name()
                .unwrap_or("unknown")
                .to_string_lossy()
                .replace("_", " ")
                .split_whitespace()
                .take(2)
                .join(" ");

            crawler_name = Some(project_name.clone());

            let supported_sources = self.detect_supported_sources(project_path).await?;

            Ok(Some(OpenSourceCrawler {
                name: project_name,
                crawler_type: crawler_type.unwrap_or(CrawlerType::DirectHttp),
                repository_url: "unknown".to_string(),
                local_path: Some(project_path.clone()),
                config_path: self.find_config_file(project_path).await?,
                supported_sources,
                start_command: self.generate_start_command(&crawler_type.unwrap_or(CrawlerType::DirectHttp))?,
                command_template: self.generate_command_template(&crawler_type.unwrap_or(CrawlerType::DirectHttp))?,
                requires_python: matches!(crawler_type, Some(CrawlerType::Scrapy) | Some(CrawlerType::BeautifulSoup) | Some(CrawlerType::DirectHttp)),
                requires_nodejs: matches!(crawler_type, Some(CrawlerType::Puppeteer) | Some(CrawlerType::Selenium) | Some(CrawlerType::Playwright)),
                default_config: HashMap::new(),
            }))
        } else {
            Ok(None)
        }
    }

    /// 检测项目支持的数据源类型
    async fn detect_supported_sources(&self, project_path: &PathBuf) -> Result<Vec<String>> {
        let mut supported_sources = Vec::new();

        // 检查README文件
        if let Some(readme_path) = self.find_readme_file(project_path).await {
            if let Ok(content) = tokio::fs::read_to_string(&readme_path).await {
                let content_lower = content.to_lowercase();

                // 根据关键词检测支持的数据源
                if content_lower.contains("cryptocurrency") || content_lower.contains("crypto") || content_lower.contains("binance") {
                    supported_sources.push("cryptocurrency".to_string());
                }
                if content_lower.contains("forex") || content_lower.contains("exchange") || content_lower.contains("fx") {
                    supported_sources.push("forex".to_string());
                }
                if content_lower.contains("commodity") || content_lower.contains("metal") || content_lower.contains("energy") {
                    supported_sources.push("commodities".to_string());
                }
                if content_lower.contains("bond") || content_lower.contains("fixed") || content_lower.contains("treasury") {
                    supported_sources.push("bonds".to_string());
                }
                if content_lower.contains("fund") || content_lower.contains("etf") || content_lower.contains("mutual") {
                    supported_sources.push("funds".to_string());
                }
                if content_lower.contains("news") || content_lower.contains("article") || content_lower.contains("rss") {
                    supported_sources.push("news".to_string());
                }
                if content_lower.contains("economic") || content_lower.contains("indicator") || content_lower.contains("gdp") {
                    supported_sources.push("economic_indicators".to_string());
                }
                if content_lower.contains("research") || content_lower.contains("report") || content_lower.contains("analysis") {
                    supported_sources.push("research_reports".to_string());
                }
            }
        }

        Ok(supported_sources)
    }

    /// 查找配置文件
    async fn find_config_file(&self, project_path: &PathBuf) -> Result<Option<PathBuf>> {
        for entry in tokio::fs::read_dir(project_path).await? {
            let path = entry.path();
            if path.is_file() {
                let file_name = path.file_name()
                    .unwrap_or("")
                    .to_string_lossy();

                let config_files = vec![
                    "scrapy.cfg", "scrapy.py", "config.yaml", "config.yml",
                    "settings.py", "settings.json", "config.json",
                    "puppeteer.config.js", "puppeteer.js",
                    ".env", ".config",
                ];

                for config_file in &config_files {
                    if file_name == config_file {
                        return Ok(Some(path));
                    }
                }
            }
        }
        Ok(None)
    }

    /// 生成启动命令
    fn generate_start_command(&self, crawler_type: &CrawlerType) -> Result<String> {
        let command = match crawler_type {
            CrawlerType::Scrapy => "scrapy crawl",
            CrawlerType::Puppeteer => "node {config_file} --headless",
            CrawlerType::Selenium => "python -m selenium {script}",
            CrawlerType::BeautifulSoup => "python {script}",
            CrawlerType::Playwright => "npx playwright {script}",
            CrawlerType::DirectHttp => "curl",
        };

        Ok(command.to_string())
    }

    /// 生成命令模板
    fn generate_command_template(&self, crawler_type: &CrawlerType) -> Result<String> {
        let template = match crawler_type {
            CrawlerType::Scrapy => r#"
# Scrapy爬虫模板
# 使用方法: scrapy crawl {spider_name} -a param1=value1 -a param2=value2
# 配置文件: scrapy.cfg 或 settings.py

import scrapy
from scrapy.crawler import CrawlerProcess

class {project_name}Spider(CrawlerSpider):
    name = '{project_name}'
    start_urls = []
    custom_settings = {{}}

    def __init__(self, **kwargs):
        self.start_urls = kwargs.get('start_urls', [])
        for url in self.start_urls:
            self.start_urls.append(url)

    def parse(self, response):
        # 解析逻辑
        pass

    def closed(self, reason):
        # 结束处理
        pass
"#.to_string(),
            CrawlerType::Puppeteer => r#"
// Puppeteer爬虫模板
const puppeteer = require('puppeteer');

async function {project_name}Crawler() {{
    const browser = await puppeteer.launch();
    const page = await browser.newPage();

    // 配置参数
    const config = require('./config.json');

    try {{
        await page.goto(config.startUrl);
        // 爬取逻辑
        const data = await page.evaluate(() => {{
            // 页面数据提取逻辑
            return document.querySelector('body').innerText;
        }});

        console.log('数据:', data);
        await browser.close();
    }} catch (error) {{
        console.error('爬取失败:', error);
        await browser.close();
    }}
}}

{project_name}Crawler().catch(console.error);
"#.to_string(),
            CrawlerType::Selenium => r#"
# Selenium爬虫模板
from selenium import webdriver
from selenium.webdriver.common.by import By
import time
import json

class {project_name}Crawler:
    def __init__(self):
        self.driver = None
        self.config = {{}}

    def setup(self):
        options = webdriver.ChromeOptions()
        options.add_argument('--headless')
        options.add_argument('--no-sandbox')
        self.driver = webdriver.Chrome(options=options)
        self.driver.implicitly_wait(10)

    def crawl(self, url):
        self.driver.get(url)
        time.sleep(2)

        # 数据提取逻辑
        data = {{
            'url': url,
            'title': self.driver.title,
            'content': self.driver.find_element(By.TAG_NAME, 'body').text,
            'timestamp': time.time()
        }}

        return data

    def cleanup(self):
        if self.driver:
            self.driver.quit()
"#.to_string(),
            CrawlerType::DirectHttp => r#"
# HTTP请求爬虫模板
import requests
import time
import json

def {project_name}Crawler:
    def __init__(self):
        self.session = requests.Session()
        self.config = {{}}

    def crawl(self, url):
        headers = self.config.get('headers', {{}})
        response = self.session.get(url, headers=headers, timeout=30)

        if response.status_code == 200:
            data = {{
                'url': url,
                'content': response.text,
                'status': 'success',
                'timestamp': time.time()
            }}
        else:
            data = {{
                'url': url,
                'error': response.status_code,
                'status': 'failed',
                'timestamp': time.time()
            }}

        return data

    def crawl_list(self, urls):
        results = []
        for url in urls:
            data = self.crawl(url)
            results.append(data)
            time.sleep(1)
        return results
"#.to_string(),
            _ => r#"
# 默认爬虫模板
import requests
import time

class {project_name}Crawler:
    def __init__(self):
        self.config = {{}}

    def crawl(self, target):
        print(f"开始爬取: {{target}}")
        # 基本爬取逻辑
        return {{'status': 'completed', 'data': 'placeholder_data'}}
"#.to_string(),
        };

        Ok(template.to_string())
    }

    /// 智能任务分配
    pub fn allocate_task(
        &self,
        task_definition: &crate::types::TaskDefinition,
    ) -> Result<TaskAllocation> {
        let data_source_type = match &task_definition.source {
            crate::types::TaskSource::AShare { .. } => "ashare".to_string(),
            crate::types::TaskSource::HKShare { .. } => "hkshare".to_string(),
            crate::types::TaskSource::USShare { .. } => "usshare".to_string(),
            crate::types::TaskSource::Cryptocurrency { .. } => "cryptocurrency".to_string(),
            crate::types::TaskSource::Forex { .. } => "forex".to_string(),
            crate::types::TaskSource::Commodities { .. } => "commodities".to_string(),
            crate::types::TaskSource::Bonds { .. } => "bonds".to_string(),
            crate::types::TaskSource::Funds { .. } => "funds".to_string(),
            crate::types::TaskSource::EconomicIndicators { .. } => "economic_indicators".to_string(),
            crate::types::TaskSource::News { .. } => "news".to_string(),
            crate::types::TaskSource::SocialMedia { .. } => "social_media".to_string(),
            crate::types::TaskSource::Announcements { .. } => "announcements".to_string(),
            crate::types::TaskSource::FinancialReports { .. } => "financial_reports".to_string(),
            crate::types::TaskSource::ESGData { .. } => "esg_data".to_string(),
            crate::types::TaskSource::ResearchReports { .. } => "research_reports".to_string(),
            crate::types::TaskSource::Futures { .. } => "futures".to_string(),
            crate::types::TaskSource::Custom { source_type, .. } => source_type.clone(),
        };

        // 智能选择最适合的爬虫
        let best_crawler = self.select_best_crawler(&data_source_type)?;

        let estimated_duration = self.estimate_execution_time(task_definition);
        let allocation_reason = format!(
            "基于数据源类型 '{}' 选择爬虫 '{}'",
            data_source_type,
            best_crawler.name
        );

        Ok(TaskAllocation {
            task_id: task_definition.id.clone(),
            crawler_name: best_crawler.name.clone(),
            data_source_type,
            estimated_duration,
            allocation_reason,
        })
    }

    /// 选择最佳爬虫
    fn select_best_crawler(&self, data_source_type: &str) -> Result<&OpenSourceCrawler> {
        let mut best_crawler = None;
        let mut best_score = 0.0;

        for (name, crawler) in &self.crawlers {
            if crawler.supported_sources.contains(&data_source_type.to_string()) {
                // 根据爬虫类型和特性评分
                let score = self.calculate_crawler_score(crawler, data_source_type);

                if score > best_score {
                    best_score = score;
                    best_crawler = Some(crawler);
                }
            }
        }

        match best_crawler {
            Some(crawler) => {
                info!("为数据源 '{}' 选择爬虫: {} (评分: {:.2})",
                     data_source_type, crawler.name, best_score);
                Ok(crawler)
            }
            None => {
                warn!("没有找到支持数据源 '{}' 的爬虫", data_source_type);
                Err(anyhow!("No crawler available for data source type: {}", data_source_type))
            }
        }
    }

    /// 计算爬虫评分
    fn calculate_crawler_score(&self, crawler: &OpenSourceCrawler, data_source_type: &str) -> f64 {
        let mut score = 50.0; // 基础分数

        // 根据爬虫类型调整评分
        match (data_source_type, &crawler.crawler_type) {
            ("cryptocurrency", CrawlerType::Scrapy) => score += 20.0,
            ("cryptocurrency", CrawlerType::DirectHttp) => score += 15.0,
            ("cryptocurrency", CrawlerType::BeautifulSoup) => score += 10.0,

            ("forex", CrawlerType::Scrapy) => score += 20.0,
            ("forex", CrawlerType::DirectHttp) => score += 15.0,

            ("news", CrawlerType::Scrapy) => score += 25.0,
            ("news", CrawlerType::BeautifulSoup) => score += 15.0,
            ("news", CrawlerType::Puppeteer) => score += 30.0,
            ("news", CrawlerType::Selenium) => score += 25.0,

            ("commodities", CrawlerType::Scrapy) => score += 20.0,
            ("commodities", CrawlerType::BeautifulSoup) => score += 10.0,

            // 默认评分
            _ => score += 0.0,
        }

        // 根据项目复杂度调整
        if crawler.supported_sources.len() > 3 {
            score += 10.0;
        }

        score
    }

    /// 估算任务执行时间
    fn estimate_execution_time(&self, task: &crate::types::TaskDefinition) -> Duration {
        let base_time = Duration::from_secs(300); // 5分钟基础时间

        // 根据任务复杂度调整
        let complexity_multiplier = match &task.source {
            crate::types::TaskSource::Cryptocurrency { symbols, .. } => {
                1.0 + (symbols.len() as f64 * 0.1)
            }
            crate::types::TaskSource::Forex { currency_pairs, .. } => {
                1.0 + (currency_pairs.len() as f64 * 0.05)
            }
            crate::types::TaskSource::News { keywords, .. } => {
                1.0 + (keywords.len() as f64 * 0.02)
            }
            _ => 1.0,
        };

        Duration::from_millis((base_time.as_millis() as f64 * complexity_multiplier) as u64)
    }

    /// 启动爬虫实例
    pub async fn start_crawler_instance(
        &mut self,
        crawler_name: &str,
        task_definition: &crate::types::TaskDefinition,
    ) -> Result<String> {
        let crawler = match self.crawlers.get(crawler_name) {
            Some(crawler) => crawler,
            None => {
                return Err(anyhow!("Crawler not found: {}", crawler_name));
            }
        };

        let instance_id = Uuid::new_v4().to_string();
        let output_file = self.workspace_root
            .join("output")
            .join(format!("{}_{}.json", crawler_name, instance_id));

        // 生成配置
        let config = self.generate_crawler_config(crawler, task_definition)?;

        info!("启动爬虫实例: {} (ID: {})", crawler_name, instance_id);

        // 启动爬虫进程
        let mut cmd = match &crawler.crawler_type {
            CrawlerType::Scrapy => {
                let mut cmd = Command::new("scrapy");
                cmd.arg("crawl");
                cmd.arg("-s");
                cmd.arg("LOG_LEVEL=INFO");
                cmd.arg(&format!("OUTPUT={}", output_file.display()));
                cmd
            }
            CrawlerType::Puppeteer => {
                let mut cmd = Command::new("node");
                cmd.arg(&crawler.config_path.as_ref().unwrap_or(&PathBuf::from("puppeteer.config.js")).display());
                cmd.arg(&crawler.local_path.as_ref().unwrap_or(&PathBuf::from("crawler.js")).display());
                cmd
            }
            CrawlerType::Selenium => {
                let mut cmd = Command::new("python");
                cmd.arg(&crawler.local_path.as_ref().unwrap_or(&PathBuf::from("selenium_crawler.py")).display());
                cmd.arg("--output");
                cmd.arg(&output_file.display());
                cmd
            }
            CrawlerType::BeautifulSoup => {
                let mut cmd = Command::new("python");
                cmd.arg(&crawler.local_path.as_ref().unwrap_or(&PathBuf::from("bs4_crawler.py")).display());
                cmd.arg("--output");
                cmd.arg(&output_file.display());
                cmd
            }
            CrawlerType::DirectHttp => {
                let mut cmd = Command::new("curl");
                cmd.arg("-o");
                cmd.arg(&output_file.display());
                cmd.arg("http://example.com"); // 需要根据任务配置
                cmd
            }
            CrawlerType::Playwright => {
                let mut cmd = Command::new("npx");
                cmd.arg("playwright");
                cmd.arg("crawl");
                cmd.arg("--config");
                cmd.arg(&crawler.config_path.as_ref().unwrap_or(&PathBuf::from("playwright.config.js")).display());
                cmd
            }
        };

        // 设置环境变量
        cmd.env("TASK_ID", &task_definition.id);
        cmd.env("DATA_SOURCE", &self.get_data_source_identifier(&task_definition.source));
        cmd.env("OUTPUT_FILE", &output_file.to_string_lossy());

        // 启动进程
        let child = cmd.spawn()?;

        // 记录实例信息
        self.running_instances.insert(
            instance_id.clone(),
            CrawlerInstance {
                project_name: crawler_name.to_string(),
                process_id: Some(child.id()),
                started_at: Some(std::time::Instant::now()),
                config,
                data_source: self.get_data_source_identifier(&task_definition.source),
                output_file,
            },
        );

        info!("爬虫实例已启动，进程ID: {:?}", child.id());

        Ok(instance_id)
    }

    /// 生成爬虫配置
    fn generate_crawler_config(
        &self,
        crawler: &OpenSourceCrawler,
        task: &crate::types::TaskDefinition,
    ) -> Result<CrawlerConfig> {
        let data_source_id = self.get_data_source_identifier(&task.source);

        let mut config = CrawlerConfig {
            language: CrawlerLanguage::Python, // 默认Python
            script_path: None,
            inline_code: None,
            working_directory: Some(format!("workspaces/{}", task.id)),
            environment: {
                // 基础环境变量
                "TASK_ID".to_string(): task.id.clone(),
                "DATA_SOURCE".to_string(): data_source_id,
                "USER_AGENT".to_string(): "Mozilla/5.0 (compatible; AlphaFinanceBot)".to_string(),
                "REQUEST_TIMEOUT".to_string(): "30".to_string(),
                "RETRY_COUNT".to_string(): "3".to_string(),
            },
            timeout: Some(3600), // 1小时
            arguments: vec![
                format!("--data-source={}", data_source_id),
                "--log-level=INFO",
            ],
            python_venv: None,
            node_project_path: None,
            go_module_path: None,
        };

        // 根据任务配置调整
        match &task.source {
            crate::types::TaskSource::AShare { symbols, .. } => {
                config.arguments.push(format!("--symbols={}", symbols.join(",")));
            }
            crate::types::TaskSource::Cryptocurrency { symbols, .. } => {
                config.arguments.push(format!("--symbols={}", symbols.join(",")));
                config.environment.insert("EXCHANGES".to_string(), "BINANCE,HUOBI,KRAKEN".to_string());
            }
            crate::types::TaskSource::Forex { currency_pairs, .. } => {
                config.arguments.push(format!("--pairs={}", currency_pairs.join(",")));
            }
            crate::types::TaskSource::News { keywords, languages, .. } => {
                config.arguments.push(format!("--keywords={}", keywords.join(",")));
                if !languages.is_empty() {
                    config.arguments.push(format!("--languages={}", languages.join(",")));
                }
            }
            _ => {}
        }

        Ok(config)
    }

    /// 获取数据源标识符
    fn get_data_source_identifier(&self, source: &crate::types::TaskSource) -> String {
        match source {
            crate::types::TaskSource::AShare { .. } => "ashare".to_string(),
            crate::types::TaskSource::HKShare { .. } => "hkshare".to_string(),
            crate::types::TaskSource::USShare { .. } => "usshare".to_string(),
            crate::types::TaskSource::Cryptocurrency { .. } => "cryptocurrency".to_string(),
            crate::types::TaskSource::Forex { .. } => "forex".to_string(),
            crate::types::TaskSource::Commodities { .. } => "commodities".to_string(),
            crate::types::TaskSource::Bonds { .. } => "bonds".to_string(),
            crate::types::TaskSource::Funds { .. } => "funds".to_string(),
            crate::types::TaskSource::EconomicIndicators { .. } => "economic_indicators".to_string(),
            crate::types::TaskSource::News { .. } => "news".to_string(),
            crate::types::TaskSource::SocialMedia { .. } => "social_media".to_string(),
            crate::types::TaskSource::Announcements { .. } => "announcements".to_string(),
            crate::types::TaskSource::FinancialReports { .. } => "financial_reports".to_string(),
            crate::types::TaskSource::ESGData { .. } => "esg_data".to_string(),
            crate::types::TaskSource::ResearchReports { .. } => "research_reports".to_string(),
            crate::types::TaskSource::Futures { .. } => "futures".to_string(),
            crate::types::TaskSource::Custom { source_type, .. } => source_type.clone(),
        }
    }

    /// 停止爬虫实例
    pub fn stop_crawler_instance(&mut self, instance_id: &str) -> Result<()> {
        if let Some(instance) = self.running_instances.get(instance_id) {
            info!("停止爬虫实例: {} (进程ID: {:?})",
                     instance.project_name,
                     instance.process_id);

            // 尝试优雅停止进程
            if let Some(process_id) = instance.process_id {
                // 使用系统命令终止进程
                #[cfg(unix)]
                {
                    use std::os::unix::process::CommandExt;
                    let mut kill_cmd = std::process::Command::new("kill");
                    kill_cmd.arg("-TERM");
                    kill_cmd.arg(process_id.to_string());

                    match kill_cmd.status() {
                        Ok(_) => {
                            info!("已发送TERM信号给进程 {}", process_id);
                        }
                        Err(e) => {
                            warn!("发送TERM信号失败: {}", e);
                            // 尝试强制终止
                            let mut force_kill = std::process::Command::new("kill");
                            force_kill.arg("-9");
                            force_kill.arg(process_id.to_string());
                            let _ = force_kill.status();
                        }
                    }
                }

                #[cfg(windows)]
                {
                    warn!("Windows进程终止未实现");
                }
            }

            self.running_instances.remove(instance_id);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crawler_discovery() {
        let manager = DistributedCrawlerManager::new("/tmp");
        // 这里需要创建测试目录结构
    }

    #[test]
    fn test_task_allocation() {
        let manager = DistributedCrawlerManager::new("/tmp");
        // 创建测试爬虫
        let crypto_crawler = OpenSourceCrawler {
            name: "test_crypto".to_string(),
            crawler_type: CrawlerType::Scrapy,
            repository_url: "https://github.com/test/crypto-crawler".to_string(),
            local_path: Some(PathBuf::from("/tmp/test_crypto")),
            config_path: None,
            supported_sources: vec!["cryptocurrency".to_string()],
            start_command: "scrapy crawl".to_string(),
            command_template: "".to_string(),
            requires_python: true,
            requires_nodejs: false,
            default_config: HashMap::new(),
        };

        manager.crawlers.insert("test_crypto".to_string(), crypto_crawler);

        let task = crate::types::TaskDefinition::new(
            "test_task",
            crate::types::TaskSource::Cryptocurrency {
                exchanges: vec![crate::types::CryptoExchange::Binance],
                symbols: vec!["BTC".to_string(), "ETH".to_string()],
            },
            "测试加密货币任务".to_string(),
        );

        let allocation = manager.allocate_task(&task).unwrap();
        assert_eq!(allocation.crawler_name, "test_crypto");
        assert_eq!(allocation.data_source_type, "cryptocurrency");
    }

    #[test]
    fn test_crawler_scoring() {
        let manager = DistributedCrawlerManager::new("/tmp");

        // 测试不同类型的爬虫
        let scrapy_crawler = OpenSourceCrawler {
            name: "scrapy_test".to_string(),
            crawler_type: CrawlerType::Scrapy,
            supported_sources: vec!["cryptocurrency".to_string(), "news".to_string()],
            ..Default::default()
        };

        let puppeteer_crawler = OpenSourceCrawler {
            name: "puppeteer_test".to_string(),
            crawler_type: CrawlerType::Puppeteer,
            supported_sources: vec!["news".to_string()],
            ..Default::default()
        };

        manager.crawlers.insert("scrapy_test".to_string(), scrapy_crawler);
        manager.crawlers.insert("puppeteer_test".to_string(), puppeteer_crawler);

        let crypto_task = crate::types::TaskDefinition::new(
            "crypto_test",
            crate::types::TaskSource::Cryptocurrency {
                exchanges: vec![],
                symbols: vec!["BTC".to_string()],
            },
            "加密货币测试任务".to_string(),
        );

        let news_task = crate::types::TaskDefinition::new(
            "news_test",
            crate::types::TaskSource::News {
                sources: vec![],
                keywords: vec!["test".to_string()],
                languages: vec!["zh".to_string()],
            },
            "新闻测试任务".to_string(),
        );

        // 测试加密货币任务分配
        let crypto_allocation = manager.allocate_task(&crypto_task).unwrap();
        assert_eq!(crypto_allocation.crawler_name, "scrapy_test"); // Scrapy更适合加密货币API

        // 测试新闻任务分配
        let news_allocation = manager.allocate_task(&news_task).unwrap();
        assert_eq!(news_allocation.crawler_name, "puppeteer_test"); // Puppeteer更适合新闻抓取
    }
}