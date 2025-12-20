//! 集成爬虫系统演示
//!
//! 展示如何整合和自动发现各种开源爬虫项目
//! 支持多种数据源和编程语言

use std::collections::HashMap;
use std::path::PathBuf;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::{
    multilang_simple::CrawlerConfig,
    distributed_crawler::{OpenSourceCrawler, DistributedCrawlerManager, TaskAllocation},
    types::{TaskDefinition, TaskSource, TaskConfig},
    data_sources::*,
};

/// 集成爬虫配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegratedCrawlerConfig {
    /// 爬虫项目列表
    pub crawlers: Vec<OpenSourceCrawler>,
    /// 数据源映射
    pub data_source_mappings: HashMap<String, Vec<String>>,
    /// 默认设置
    pub default_settings: IntegratedCrawlerSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegratedCrawlerSettings {
    /// 最大并发爬虫数
    pub max_concurrent_crawlers: u32,
    /// 心跳间隔
    pub heartbeat_interval: u64,
    /// 资源限制
    pub resource_limits: ResourceLimits,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// 每个爬虫最大内存
    pub max_memory_per_crawler: String,
    /// 每个爬虫最大CPU使用率
    pub max_cpu_per_crawler: String,
}

/// 集成爬虫演示系统
pub struct IntegratedCrawlerDemo {
    workspace_root: PathBuf,
    config: IntegratedCrawlerConfig,
    manager: DistributedCrawlerManager,
}

impl IntegratedCrawlerDemo {
    pub fn new<P: AsRef<std::path::Path>>(
        workspace_root: P,
        config: IntegratedCrawlerConfig,
    ) -> Self {
        let workspace_root = workspace_root.as_ref().to_path_buf();
        let manager = DistributedCrawlerManager::new(&workspace_root);

        Self {
            workspace_root,
            config,
            manager,
        }
    }

    /// 运行集成爬虫演示
    pub async fn run_demo(&self) -> Result<()> {
        println!("🚀 Alpha Finance 集成爬虫系统演示");
        println!("======================================");

        // 初始化分布式爬虫管理器
        self.manager.discover_crawlers().await?;

        // 演示自动发现的爬虫项目
        self.demo_auto_discovery().await?;

        // 演示智能任务分配
        self.demo_intelligent_task_allocation().await?;

        // 演示多语言爬虫支持
        self.demo_multilang_crawler().await?;

        // 演示分布式爬虫管理
        self.demo_distributed_management().await?;

        println!("✅ 集成爬虫系统演示完成！");
        Ok(())
    }

    /// 演示自动发现爬虫项目
    pub async fn demo_auto_discovery(&self) -> Result<()> {
        println!("\n🔍 自动发现爬虫项目演示");

        // 初始化爬虫发现
        self.manager.discover_crawlers().await?;

        // 模拟发现过程
        let discovered_crawlers = self.manager.crawlers.len();
        println!("📊 发现 {} 个爬虫项目", discovered_crawlers);

        for (i, crawler) in self.manager.crawlers.iter().take(5).enumerate() {
            println!("  {}. {} ({})", i + 1, crawler.name, crawler.crawler_type);
            println!("    📍 仓库: {}", crawler.repository_url);

            if let Some(local_path) = &crawler.local_path {
                println!("    📁 本地路径: {}", local_path.display());
            }

            println!("    🔧 支持数据源: {:?}", crawler.supported_sources);
        }

        // 演示配置加载
        if let Err(e) = self.manager.load_crawler_config("scrapy-redis").await {
            println!("    ❌ 配置加载失败: {}", e);
        } else {
            println!("    ✅ 配置加载成功");
        }

        Ok(())
    }

    /// 演示智能任务分配
    pub async fn demo_intelligent_task_allocation(&self) -> Result<()> {
        println!("\n🧠 智能任务分配演示");

        // 创建示例任务
        let tasks = vec![
            TaskDefinition::new(
                "crypto_task_1",
                TaskSource::Cryptocurrency {
                    exchanges: vec![CryptoExchange::Binance],
                    symbols: vec!["BTC".to_string(), "ETH".to_string()],
                },
                "low".into(),
                TaskConfig::default(),
            ),
            TaskDefinition::new(
                "news_task_1",
                TaskSource::News {
                    keywords: vec!["财经新闻".to_string(), "股市动态".to_string()],
                    languages: vec!["zh".to_string()],
                },
                "high".into(),
                TaskConfig::default(),
            ),
            TaskDefinition::new(
                "forex_task_1",
                TaskSource::Forex {
                    currency_pairs: vec!["EUR/USD".to_string(), "GBP/USD".to_string()],
                },
                "high".into(),
                TaskConfig::default(),
            ),
            TaskDefinition::new(
                "ashare_task_1",
                TaskSource::AShare {
                    symbols: vec!["000001".to_string(), "000002".to_string()],
                },
                "critical".into(),
                TaskConfig::default(),
            ),
        ];

        for task in &tasks {
            println!("  📋 任务: {}", task.name);

            // 分配任务
            match self.manager.allocate_task(task).await {
                Ok(allocation) => {
                    println!("    ✅ 分配成功:");
                    println!("      爬虫: {}", allocation.crawler_name);
                    println!("      数据源: {}", allocation.data_source_type);
                    println!("      估计时间: {}秒", allocation.estimated_duration.as_secs());
                    println!("      原因: {}", allocation.allocation_reason);
                    println!("      实例ID: {}", allocation.task_id);
                }
                Err(e) => {
                    println!("    ❌ 分配失败: {}", e);
                }
            }
        }

        Ok(())
    }

    /// 演示多语言爬虫支持
    pub async fn demo_multilang_crawler(&self) -> Result<()> {
        println!("\n🌐 多语言爬虫支持演示");

        let languages = [
            crate::multilang_simple::CrawlerLanguage::Python,
            crate::multilang_simple::CrawlerLanguage::NodeJs,
            crate::multilang_simple::CrawlerLanguage::Go,
            crate::multilang_simple::CrawlerLanguage::Rust,
        ];

        for language in &languages {
            let available = language.is_available().await;
            if available {
                println!("  ✅ {} 可用: {}", language, language.command());
            } else {
                println!("  ❌ {} 不可用: {}", language, language.command());
            }
        }

        // 演示Python爬虫
        let python_config = CrawlerConfig {
            language: crate::multilang_simple::CrawlerLanguage::Python,
            script_path: Some("demo_crypto.py".into()),
            arguments: vec!["--exchange=Binance", "--symbol=BTC"],
            timeout: Some(3600),
            inline_code: None,
        };

        match self.manager.allocate_task(&TaskDefinition::new(
            "demo_crypto_task",
            TaskSource::Cryptocurrency {
                exchanges: vec![CryptoExchange::Binance],
                symbols: vec!["BTC".to_string()],
            },
            "high".into(),
            TaskConfig::default(),
        ), python_config).await {
            Ok(_) => {
                println!("    ✅ Python加密货币任务分配成功");
            }
            Err(e) => {
                println!("    ❌ Python加密货币任务分配失败: {}", e);
            }
        }

        // 演示Node.js爬虫
        let node_config = CrawlerConfig {
            language: crate::multilang_simple::CrawlerLanguage::NodeJs,
            script_path: Some("demo_puppeteer.js".into()),
            arguments: vec!["--data-source=news", "--keywords=科技新闻"],
            timeout: Some(3000),
            inline_code: None,
        };

        match self.manager.allocate_task(&TaskDefinition::new(
            "demo_news_task",
            TaskSource::News {
                keywords: vec!["科技新闻".to_string(), "财经新闻".to_string()],
                languages: vec!["zh".to_string()],
            },
            "medium".into(),
            TaskConfig::default(),
        ), node_config).await {
            Ok(_) => {
                println!("    ✅ Node.js新闻任务分配成功");
            }
            Err(e) => {
                println!("    ❌ Node.js新闻任务分配失败: {}", e);
            }
        }

        Ok(())
    }

    /// 演示分布式爬虫管理
    pub async fn demo_distributed_management(&self) -> Result<()> {
        println!("\n🔧 分布式爬虫管理演示");

        // 获取状态
        let status = self.manager.get_cluster_status().await?;
        println!("📊 集群状态:");
        println!("  🤖 运行中: {}/{}", status.running_crawlers, status.total_instances);
        println!("  ⏸️ 总实例: {}/{}", status.total_instances, status.total_instances);
        println!("  📈 待处理任务: {}/{}", status.pending_tasks, status.total_instances);
        println!("  💾 已完成任务: {}/{}", status.completed_tasks, status.total_instances);

        // 演示扩容操作
        println!("\n📈 扩容演示 (3个新实例)...");

        // 启动3个新实例
        for i in 1..=3 {
            let instance_name = format!("demo_crawler_{}", i);
            let crawler_name = "puppeteer-crawler-master";

            match self.manager.start_crawler_instance(&instance_name, crawler_name).await {
                Ok(instance_id) => {
                    println!("  ✅ 扩容实例 {} (ID: {}) 启动成功", instance_name, instance_id);
                }
                Err(e) => {
                    println!("  ❌ 扩容实例 {} 失败: {}", instance_name, e);
                }
            }
        }

        // 检查扩容后状态
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        let new_status = self.manager.get_cluster_status().await?;
        println!("\n扩容后状态:");
        println!("  🤖 运行中: {}/{}", new_status.running_crawlers, new_status.total_instances);
        println!("  ⏸️ 总实例: {}/{}", new_status.total_instances, new_status.total_instances);
        println!("  📈 待处理任务: {}/{}", new_status.pending_tasks, new_status.total_instances);
        println!("  💾 已完成任务: {}/{}", new_status.completed_tasks, new_status.total_instances);

        println!("✅ 分布式爬虫管理演示完成！");
        Ok(())
    }

    /// 创建示例配置
    pub fn create_sample_config() -> IntegratedCrawlerConfig {
        IntegratedCrawlerConfig {
            crawlers: vec![
                OpenSourceCrawler {
                    name: "scrapy-redis".to_string(),
                    display_name: "Scrapy Redis 爬虫".to_string(),
                    crawler_type: crate::data_sources::CrawlerType::Scrapy,
                    repository_url: "https://github.com/scrapy-redis".to_string(),
                    local_path: None,
                    config_path: Some("settings.py".to_string()),
                    supported_sources: vec!["ashare".to_string(), "cryptocurrency".to_string(), "forex".to_string(), "news".to_string()],
                    start_command: "scrapy crawl".to_string(),
                    command_template: "scrapy crawl {spider_name} -s LOG_LEVEL=INFO".to_string(),
                    requires_python: true,
                    requires_nodejs: false,
                    default_config: HashMap::from([
                        ("CONCURRENT_REQUESTS".to_string(), "16".to_string()),
                        ("REDIS_HOST".to_string(), "localhost".to_string()),
                        ("REDIS_PORT".to_string(), "6379".to_string()),
                        ("LOG_LEVEL".to_string(), "INFO".to_string()),
                        ("COOKIES_ENABLED".to_string(), "true".to_string()),
                        ("TELNETCONSOLE_ENABLED".to_string(), "false".to_string()),
                        ("USER_AGENT".to_string(), "Mozilla/5.0 (compatible; AlphaFinanceBot 1.0)".to_string()),
                    ]),
                },
            ],
            data_source_mappings: HashMap::from([
                ("ashare".to_string(), vec!["scrapy-redis".to_string(), "selenium-finance-crawler".to_string()]),
                ("cryptocurrency".to_string(), vec!["scrapy-redis".to_string(), "node-crypto-trader".to_string()]),
                ("forex".to_string(), vec!["python-forex-scraper".to_string()]),
                ("news".to_string(), vec!["puppeteer-crawler-master".to_string(), "selenium-finance-crawler".to_string()]),
            ]),
            default_settings: IntegratedCrawlerSettings {
                max_concurrent_crawlers: 5,
                heartbeat_interval: 30,
                resource_limits: ResourceLimits {
                    max_memory_per_crawler: "2GB".to_string(),
                    max_cpu_per_crawler: "50%".to_string(),
                },
            },
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let demo = IntegratedCrawlerDemo::new(
        ".",
        IntegratedCrawlerDemo::create_sample_config(),
    );

    demo.run_demo().await
}