//! Alpha Collector Service
//!
//! 多语言异步爬虫与数据采集引擎（简化版入口）

use std::{net::SocketAddr, sync::Arc};

use anyhow::Result;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("Starting Alpha Collector Service...");

    let workspace_root = std::env::var("ALPHA_WORKSPACE_ROOT")
        .ok()
        .map(std::path::PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    let collector = std::sync::Arc::new(
        alpha_collector::main_simple::SimpleCollector::new(workspace_root).with_raw_archiver(
            // 原始响应归档（env 门控默认关）：URL 未设置=None 零行为变化；
            // 设置了但连接串非法=启动即退出（配置错误不静默吞）
            alpha_collector::raw_archive::RawArchiver::from_env()
                .map_err(|e| anyhow::anyhow!("原始归档配置错误：{e}"))?,
        ),
    );
    collector.start().await?;

    // 任务模板（architecture §24 任务描述）：ALPHA_COLLECTOR_TASKS 指向 YAML/JSON
    // 模板文件或目录即启动时装载登记；未设置时零行为变化。
    // 示例见 config/collector.tasks.yaml
    if let Ok(template_path) = std::env::var("ALPHA_COLLECTOR_TASKS") {
        if !template_path.trim().is_empty() {
            let (submitted, skipped) = collector
                .submit_task_templates(std::path::Path::new(&template_path))
                .await
                .map_err(|e| anyhow::anyhow!("装载任务模板 {} 失败：{}", template_path, e))?;
            info!(
                "Collector task templates loaded: {} submitted, {} disabled-skipped",
                submitted, skipped
            );
        }
    }

    // Cron 调度（architecture §24 刷新频率执行面）：schedule 非空的任务按
    // cron 周期自动执行；HTTP API 与模板提交的任务同享
    Arc::clone(&collector).start_cron_scheduler().await;

    let router = alpha_collector::main_simple::build_router(Arc::clone(&collector));

    // 与 docker-compose/Dockerfile/dev-start 的约定一致（8083），可用 ALPHA_COLLECTOR_BIND 覆盖
    let addr: SocketAddr = std::env::var("ALPHA_COLLECTOR_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8083".to_string())
        .parse()?;
    info!("Collector service listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;

    info!("Alpha Collector Service stopped");
    Ok(())
}
