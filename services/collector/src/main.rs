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

    let collector = alpha_collector::main_simple::SimpleCollector::new("/tmp/alpha-collector");
    collector.start().await?;

    let router = alpha_collector::main_simple::build_router(Arc::new(collector));

    let addr: SocketAddr = "0.0.0.0:3000".parse()?;
    info!("Collector service listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;

    info!("Alpha Collector Service stopped");
    Ok(())
}

