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
            "#,
            FileFormat::Yaml,
        ));
        let cfg = AppConfig::load_from_builder(builder).expect("config overrides apply");

        assert_eq!(cfg.server.addr, "127.0.0.1:9000");
        assert_eq!(cfg.telemetry.level.to_lowercase(), "debug");
        assert!(!cfg.data.seed_demo_data);
        assert_eq!(cfg.server.grpc_addr, "127.0.0.1:50060");
        assert!(cfg.storage.persistence_enabled);
        assert_eq!(cfg.storage.timescale_url.as_deref(), Some("postgres://demo"));
    }
}
