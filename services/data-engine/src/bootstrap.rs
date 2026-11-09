//! 存储面装配：ClickHouse / Timescale 持久化 / Parquet 湖 writer 按配置
//! 三态初始化（未启用 → None；启用但不可用 → 告警降级 None；正常 → Some），
//! 由 `AppState::new` 调用（原 main.rs 738-809 行，行为不变）。

use std::sync::Arc;

use crate::settings::{AppConfig, ClickHouseSettings, StorageConfig};
use alpha_storage::{
    clickhouse::{ClickHouseConfig, ClickHouseStorage},
    lake::LakeWriter,
    partition::PartitionScheme,
    TimescaleTimeSeriesStorage,
};

pub(crate) async fn initialize_clickhouse(
    settings: &ClickHouseSettings,
) -> Option<Arc<ClickHouseStorage>> {
    if !settings.enabled {
        return None;
    }

    let cfg = ClickHouseConfig {
        url: settings.url.clone(),
        native_url: "tcp://localhost:9000".to_string(),
        database: settings.database.clone(),
        user: settings.user.clone(),
        password: settings.password.clone(),
    };

    match ClickHouseStorage::new(cfg).await {
        Ok(storage) => {
            tracing::info!("ClickHouse backend enabled");
            Some(Arc::new(storage))
        }
        Err(err) => {
            tracing::warn!("ClickHouse enabled but connection failed: {}", err);
            None
        }
    }
}

/// 按配置装配 Timescale 持久化（与 ClickHouse 装配同口径的三态降级）：
/// - 未启用（persistence_enabled=false）→ None，纯内存；
/// - 启用但 URL 缺失/无效、连接失败 → 告警后降级为 None（服务不因落库不可用而拒绝启动）；
/// - 正常 → Some，规范化写入经 write_normalized 镜像落库。
pub(crate) async fn initialize_persistence(
    settings: &StorageConfig,
) -> Option<Arc<TimescaleTimeSeriesStorage>> {
    if !settings.persistence_enabled {
        return None;
    }

    let Some(url) = settings.timescale_url.as_deref() else {
        tracing::warn!(
            "Persistence enabled but storage.timescale_url is not set; falling back to memory-only"
        );
        return None;
    };

    match TimescaleTimeSeriesStorage::connect(url).await {
        Ok(storage) => {
            tracing::info!("Timescale persistence enabled");
            Some(Arc::new(storage))
        }
        Err(err) => {
            tracing::warn!(
                "Persistence enabled but Timescale connect failed: {}; falling back to memory-only",
                err
            );
            None
        }
    }
}

/// 按配置装配 Parquet 湖 writer：未启用 → None（export 端点保持纯即时导出）。
pub(crate) fn initialize_lake(config: &AppConfig) -> Option<Arc<LakeWriter>> {
    if !config.lake.enabled {
        return None;
    }
    tracing::info!(
        lake_root = %config.lake.lake_root,
        "Parquet lake write-through enabled"
    );
    Some(Arc::new(LakeWriter::new(
        config.lake.lake_root.as_str(),
        PartitionScheme::DateOnly,
    )))
}
