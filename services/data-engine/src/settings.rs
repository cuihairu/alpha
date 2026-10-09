//! 配置加载模块

#[cfg(test)]
use config::FileFormat;
use config::{builder::DefaultState, ConfigBuilder, ConfigError, Environment, File};
use serde::Deserialize;
use tracing::Level;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub telemetry: TelemetryConfig,
    pub data: DataConfig,
    pub storage: StorageConfig,
    pub clickhouse: ClickHouseSettings,
    pub sweeper: SweeperConfig,
    /// 第三方集成鉴权（L504）：api_keys 非空时行情数据面统一要求 X-Api-Key
    pub security: SecurityConfig,
    /// Parquet 湖写透（docs/data-lake-parquet.md §8/§10）：默认关——关闭时
    /// export 端点保持纯即时导出，零行为变化
    pub lake: LakeSettings,
}

/// API key 门配置：空表 = 关闭（内网默认形态，历史行为不变）。
/// 运维面 /health、/metrics 不受本配置影响（存活探测与抓取器不带业务凭据）。
#[derive(Debug, Clone, Deserialize)]
pub struct SecurityConfig {
    pub api_keys: Vec<String>,
}

/// 消费组孤儿 pending 周期兜底（claim_stale：XPENDING 扫描 + XCLAIM 认领）配置
#[derive(Debug, Clone, Deserialize)]
pub struct SweeperConfig {
    pub enabled: bool,
    /// pending 闲置多久才认定为孤儿（需大于正常处理耗时）
    pub min_idle_ms: u64,
    /// 兜底扫描间隔（秒）
    pub interval_secs: u64,
    /// 单条消息累计投递次数封顶（含首次投递）：达到上限仍 pending 即判定「毒消息」，
    /// sweeper 不再认领重投，转 DLQ 契约隔离（publish_dlq + ack）。默认 5：按默认
    /// 30s 扫描间隔约 2 分钟重试窗口，足以覆盖部署重启类瞬断，又不会让坏消息无限空转。
    pub max_delivery_count: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub addr: String,
    pub enable_cors: bool,
    pub grpc_addr: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelemetryConfig {
    pub level: String,
    pub json: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DataConfig {
    pub seed_demo_data: bool,
    pub seed_symbols: Vec<String>,
    pub lookback_days: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    pub persistence_enabled: bool,
    pub timescale_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClickHouseSettings {
    pub enabled: bool,
    pub url: String,
    pub database: String,
    pub user: String,
    pub password: String,
}

/// Parquet 湖写透配置：启用后 `/clickhouse/export.parquet?query_id=market_data`
/// 在返回响应的同时把数据按交易日分区落湖（写失败只告警不拒绝请求），
/// `?from_lake=true` 读旁路可用。骨架期单写者（§10.2）、DateOnly 分区（§3）。
/// `maintenance_interval_secs` 为后台维护周期（compaction + manifest 重建），
/// 0 = 不启动维护任务（默认， lake 只作写透与读旁路）。
#[derive(Debug, Clone, Deserialize)]
pub struct LakeSettings {
    pub enabled: bool,
    pub lake_root: String,
    pub layer: String,
    pub table: String,
    pub maintenance_interval_secs: u64,
}

impl AppConfig {
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_from_builder(
            Self::base_builder()
                .add_source(File::with_name("services/data-engine/config").required(false))
                .add_source(File::with_name("Config").required(false))
                .add_source(Environment::with_prefix("ALPHA").separator("__")),
        )
    }

    fn base_builder() -> ConfigBuilder<DefaultState> {
        config::Config::builder()
            .set_default("server.addr", "0.0.0.0:8081")
            .expect("failed to set server.addr default")
            .set_default("server.enable_cors", true)
            .expect("failed to set cors default")
            .set_default("server.grpc_addr", "0.0.0.0:50051")
            .expect("failed to set grpc addr default")
            .set_default("telemetry.level", "info")
            .expect("failed to set telemetry.level default")
            .set_default("telemetry.json", false)
            .expect("failed to set telemetry.json default")
            .set_default("data.seed_demo_data", true)
            .expect("failed to set seed_demo_data default")
            .set_default(
                "data.seed_symbols",
                vec!["AAPL", "MSFT", "TSLA", "AMZN", "NVDA"],
            )
            .expect("failed to set seed_symbols default")
            .set_default("data.lookback_days", 90_u32)
            .expect("failed to set lookback default")
            .set_default("storage.persistence_enabled", false)
            .expect("failed to set persistence default")
            .set_default("storage.timescale_url", "")
            .expect("failed to set timescale_url default")
            .set_default("clickhouse.enabled", false)
            .expect("failed to set clickhouse.enabled default")
            .set_default("clickhouse.url", "http://localhost:8123")
            .expect("failed to set clickhouse.url default")
            .set_default("clickhouse.database", "alpha_finance")
            .expect("failed to set clickhouse.database default")
            .set_default("clickhouse.user", "admin")
            .expect("failed to set clickhouse.user default")
            .set_default("clickhouse.password", "admin123")
            .expect("failed to set clickhouse.password default")
            .set_default("sweeper.enabled", true)
            .expect("failed to set sweeper.enabled default")
            .set_default("sweeper.min_idle_ms", 30_000_u64)
            .expect("failed to set sweeper.min_idle_ms default")
            .set_default("sweeper.interval_secs", 30_u64)
            .expect("failed to set sweeper.interval_secs default")
            .set_default("sweeper.max_delivery_count", 5_u64)
            .expect("failed to set sweeper.max_delivery_count default")
            .set_default("security.api_keys", Vec::<String>::new())
            .expect("failed to set security.api_keys default")
            .set_default("lake.enabled", false)
            .expect("failed to set lake.enabled default")
            .set_default("lake.lake_root", "lake")
            .expect("failed to set lake.lake_root default")
            .set_default("lake.layer", "silver")
            .expect("failed to set lake.layer default")
            .set_default("lake.table", "market_data")
            .expect("failed to set lake.table default")
            .set_default("lake.maintenance_interval_secs", 0_u64)
            .expect("failed to set lake.maintenance_interval_secs default")
    }

    fn load_from_builder(builder: ConfigBuilder<DefaultState>) -> Result<Self, ConfigError> {
        let mut cfg: AppConfig = builder.build()?.try_deserialize()?;
        if let Some(url) = cfg.storage.timescale_url.as_ref() {
            if url.trim().is_empty() {
                cfg.storage.timescale_url = None;
            }
        }
        Ok(cfg)
    }
}

impl TelemetryConfig {
    pub fn level_filter(&self) -> Level {
        match self.level.to_lowercase().as_str() {
            "debug" => Level::DEBUG,
            "warn" => Level::WARN,
            "error" => Level::ERROR,
            "trace" => Level::TRACE,
            _ => Level::INFO,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::load_from_builder(Self::base_builder())
            .expect("default configuration should never fail")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_loaded() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.server.addr, "0.0.0.0:8081");
        assert!(cfg.server.enable_cors);
        assert_eq!(cfg.server.grpc_addr, "0.0.0.0:50051");
        assert!(cfg.data.seed_demo_data);
        assert!(!cfg.data.seed_symbols.is_empty());
        assert!(!cfg.storage.persistence_enabled);
        assert!(cfg.storage.timescale_url.is_none());
        assert!(!cfg.clickhouse.enabled);
        assert!(cfg.sweeper.enabled);
        assert_eq!(cfg.sweeper.min_idle_ms, 30_000);
        assert_eq!(cfg.sweeper.interval_secs, 30);
        assert_eq!(cfg.sweeper.max_delivery_count, 5);
        assert!(cfg.security.api_keys.is_empty());
        assert!(!cfg.lake.enabled);
        assert_eq!(cfg.lake.lake_root, "lake");
        assert_eq!(cfg.lake.layer, "silver");
        assert_eq!(cfg.lake.table, "market_data");
        assert_eq!(cfg.lake.maintenance_interval_secs, 0);
    }

    #[test]
    fn overrides_are_applied() {
        let builder = AppConfig::base_builder().add_source(File::from_str(
            r#"
                server:
                  addr: "127.0.0.1:9000"
                  grpc_addr: "127.0.0.1:50060"
                telemetry:
                  level: "debug"
                data:
                  seed_demo_data: false
                storage:
                  persistence_enabled: true
                  timescale_url: "postgres://demo"
                clickhouse:
                  enabled: true
                  url: "http://127.0.0.1:8123"
                security:
                  api_keys: ["third-party-key-a", "third-party-key-b"]
                lake:
                  enabled: true
                  lake_root: "/tmp/lake-demo"
                  layer: "bronze"
                  table: "market_data"
                  maintenance_interval_secs: 600
            "#,
            FileFormat::Yaml,
        ));
        let cfg = AppConfig::load_from_builder(builder).expect("config overrides apply");

        assert_eq!(cfg.server.addr, "127.0.0.1:9000");
        assert_eq!(cfg.telemetry.level.to_lowercase(), "debug");
        assert!(!cfg.data.seed_demo_data);
        assert_eq!(cfg.server.grpc_addr, "127.0.0.1:50060");
        assert!(cfg.storage.persistence_enabled);
        assert_eq!(
            cfg.storage.timescale_url.as_deref(),
            Some("postgres://demo")
        );
        assert!(cfg.clickhouse.enabled);
        assert_eq!(cfg.clickhouse.url, "http://127.0.0.1:8123");
        assert_eq!(
            cfg.security.api_keys,
            vec!["third-party-key-a", "third-party-key-b"]
        );
        assert!(cfg.lake.enabled);
        assert_eq!(cfg.lake.lake_root, "/tmp/lake-demo");
        assert_eq!(cfg.lake.layer, "bronze");
        assert_eq!(cfg.lake.maintenance_interval_secs, 600);
    }
}
