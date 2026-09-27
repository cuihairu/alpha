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
    let collector = alpha_collector::main_simple::SimpleCollector::new(workspace_root);
    collector.start().await?;

    let router = alpha_collector::main_simple::build_router(Arc::new(collector));

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
