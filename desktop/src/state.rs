//! 应用状态：分析引擎 + 目录布局（Tauri `manage` 注入的载荷）
//!
//! 只含纯 Rust 类型，故可在无 GUI 环境构造并测试；Tauri 接线层用它做命令参数。

use crate::error::DesktopResult;
use crate::notify;
use crate::paths::AppPaths;
use alpha_core::analytics::AnalysisEngine;

/// 全局应用状态
#[derive(Debug)]
pub struct AppState {
    engine: AnalysisEngine,
    paths: AppPaths,
    /// 通知队列（L114）：会话内通知历史，`send_notification` 入队、
    /// `list_notifications` 读取；互斥锁只保护入队/读取的短临界区
    notifications: std::sync::Mutex<notify::NotificationQueue>,
    /// 窗口几何节流器（L115）：接线层移动/缩放事件高频触发，经此判定
    /// 是否值得落盘；返回 `&Mutex` 而非守卫，调用方自行控制临界区
    window_tracker: std::sync::Mutex<crate::window::WindowStateTracker>,
    /// 本地数据库（L116）：kv 目录的文件键值存储，离线快照/同步水位落这里
    kv: crate::kv::FileKeyValueStore,
    /// 远端取数缝（L116）：生产为演示行情口径 [`crate::offline::SyntheticRemote`]，
    /// 真实后端接入时替换实现，同步/降级语义不动
    remote: std::sync::Arc<dyn crate::offline::QuoteRemote>,
    /// 后端地址（L116 连通探测目标；来自生效配置，缺省 `DEFAULT_API_URL`）
    api_url: String,
}

impl AppState {
    /// 以已解析的目录启动：建目录 + 初始化分析引擎
    pub fn bootstrap(
        config_dir: impl Into<std::path::PathBuf>,
        data_dir: impl Into<std::path::PathBuf>,
    ) -> DesktopResult<Self> {
        let paths = AppPaths::new(config_dir, data_dir);
        paths.ensure()?;
        Ok(Self::with_paths(paths))
    }

    /// 仅构造不建目录（测试/只读场景）
    pub fn with_paths(paths: AppPaths) -> Self {
        let kv = crate::kv::FileKeyValueStore::new(paths.kv_dir());
        Self {
            engine: AnalysisEngine::new(),
            kv,
            remote: std::sync::Arc::new(crate::offline::SyntheticRemote),
            api_url: crate::config::DEFAULT_API_URL.to_string(),
            paths,
            notifications: std::sync::Mutex::new(notify::NotificationQueue::new(
                notify::DEFAULT_QUEUE_CAPACITY,
            )),
            window_tracker: std::sync::Mutex::new(crate::window::WindowStateTracker::default()),
        }
    }

    /// 分析引擎（只读借用）
    pub fn engine(&self) -> &AnalysisEngine {
        &self.engine
    }

    /// 目录布局
    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }

    /// 通知队列（L114）：接线层 `send_notification`/`list_notifications` 经此
    /// 取锁；返回 `&Mutex` 而非守卫，使调用方自行控制临界区范围
    pub fn notification_queue(&self) -> &std::sync::Mutex<notify::NotificationQueue> {
        &self.notifications
    }

    /// 窗口几何节流器（L115）：接线层 `on_window_event` 的移动/缩放/关闭路径
    /// 经此取锁判定是否落盘
    pub fn window_tracker(&self) -> &std::sync::Mutex<crate::window::WindowStateTracker> {
        &self.window_tracker
    }

    /// 本地数据库（L116）：离线快照与同步水位的读写面
    pub fn kv(&self) -> &crate::kv::FileKeyValueStore {
        &self.kv
    }

    /// 远端取数缝（L116）：命令体只透传，实现可替换
    pub fn remote(&self) -> &std::sync::Arc<dyn crate::offline::QuoteRemote> {
        &self.remote
    }

    /// 后端地址（L116 连通探测目标）
    pub fn api_url(&self) -> &str {
        &self.api_url
    }
}

/// 启动应用：加载/自举配置 → 落盘可用配置 → 建目录 → 初始化引擎
///
/// `initialize_app` 命令的实现体。放在框架层的理由同上：配置自举（缺失或损坏
/// 时把可用配置写回磁盘，保证下次走 File 路径）是业务口径而非平台接线，
/// 且必须能在无 GUI 环境测试——接线层只负责取目录与 `manage` 注入。
pub fn bootstrap_app(
    config_dir: impl Into<std::path::PathBuf>,
    data_dir: impl Into<std::path::PathBuf>,
) -> DesktopResult<(AppState, crate::ipc::InitPayload)> {
    let paths = AppPaths::new(config_dir.into(), data_dir.into());
    let config_file = paths.config_file();
    let (config, source) = crate::config::load_or_default(&config_file);
    if source != crate::config::ConfigSource::File {
        // 首次启动/配置损坏：把可用配置落盘，保证下次走 File 路径
        crate::config::save(&config_file, &config)?;
    }
    paths.ensure()?;
    let state = AppState {
        engine: AnalysisEngine::new(),
        kv: crate::kv::FileKeyValueStore::new(paths.kv_dir()),
        remote: std::sync::Arc::new(crate::offline::SyntheticRemote),
        api_url: config.api_url.clone(),
        paths,
        notifications: std::sync::Mutex::new(notify::NotificationQueue::new(
            notify::DEFAULT_QUEUE_CAPACITY,
        )),
        window_tracker: std::sync::Mutex::new(crate::window::WindowStateTracker::default()),
    };
    let payload = crate::ipc::InitPayload::new(config, source);
    // 降级提示的判定也在此：接线层只回传载荷，不再复述一遍分支
    if payload.is_recovered() {
        tracing::warn!(problems = ?payload.validation, "配置损坏，已回退默认值");
    } else if !payload.validation.is_empty() {
        tracing::warn!(problems = ?payload.validation, "应用配置不完整，按当前值启动");
    }
    Ok((state, payload))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[tokio::test]
    async fn bootstrap_creates_directory_tree() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let state = AppState::bootstrap(tmp.path().join("config"), tmp.path().join("data"))
            .expect("启动应成功");
        for dir in state.paths().all_dirs() {
            assert!(dir.is_dir(), "缺目录: {}", dir.display());
        }
    }

    #[test]
    fn bootstrap_app_persists_default_config_on_first_run() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let config_dir = tmp.path().join("config");
        let data_dir = tmp.path().join("data");
        let (_state, payload) = bootstrap_app(&config_dir, &data_dir).expect("首次启动应成功");

        assert_eq!(payload.source, "defaults", "首次应走默认配置");
        assert!(!payload.is_recovered());
        let config_file = config_dir.join(crate::paths::CONFIG_FILE_NAME);
        assert!(config_file.is_file(), "可用配置应已落盘: {config_file:?}");
        // 二次启动应读到文件而非再判 defaults
        let (_state2, again) = bootstrap_app(&config_dir, &data_dir).expect("二次启动应成功");
        assert_eq!(again.source, "file");
        assert_eq!(again.config, payload.config, "落盘内容应与生效配置一致");
    }

    #[test]
    fn bootstrap_app_creates_all_directories() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let (state, _payload) =
            bootstrap_app(tmp.path().join("config"), tmp.path().join("data")).expect("启动应成功");
        for dir in state.paths().all_dirs() {
            assert!(dir.is_dir(), "缺目录: {}", dir.display());
        }
    }

    #[test]
    fn bootstrap_app_reports_validation_problems_without_failing() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let config_dir = tmp.path().join("config");
        // 先落一份非法配置（symbols 为空）
        let mut bad = crate::config::AppConfig::default();
        bad.symbols.clear();
        crate::config::save(&config_dir.join(crate::paths::CONFIG_FILE_NAME), &bad)
            .expect("写配置");
        let (_state, payload) =
            bootstrap_app(&config_dir, tmp.path().join("data")).expect("配置不完整不应阻断启动");
        assert!(
            payload.validation.iter().any(|p| p.contains("symbols")),
            "应报出 symbols 问题: {:?}",
            payload.validation
        );
        assert_eq!(payload.source, "file", "读到的是文件（虽有校验问题）");
    }

    #[tokio::test]
    async fn engine_is_usable_right_after_bootstrap() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let state = AppState::bootstrap(tmp.path().join("config"), tmp.path().join("data"))
            .expect("启动应成功");
        let result = crate::analysis::analyze(state.engine(), "600519")
            .await
            .expect("分析应成功");
        assert_eq!(result.symbol, "600519");
    }

    #[test]
    fn with_paths_does_not_touch_filesystem() {
        let paths = AppPaths::new(
            Path::new("/nonexistent/config"),
            Path::new("/nonexistent/data"),
        );
        let state = AppState::with_paths(paths.clone());
        assert_eq!(state.paths(), &paths);
    }

    #[test]
    fn bootstrap_reports_io_error_for_unusable_dir() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let blocker = tmp.path().join("blocker");
        std::fs::write(&blocker, b"file").expect("写占位文件");
        let err = AppState::bootstrap(&blocker, tmp.path().join("data")).expect_err("应失败");
        assert_eq!(err.kind(), "io");
    }

    #[test]
    fn notification_queue_is_usable_right_after_bootstrap() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let state = AppState::bootstrap(tmp.path().join("config"), tmp.path().join("data"))
            .expect("启动应成功");
        let queue = state.notifications.lock().expect("通知队列锁中毒");
        assert!(queue.is_empty());
        assert_eq!(queue.capacity(), notify::DEFAULT_QUEUE_CAPACITY);
    }

    #[test]
    fn window_tracker_is_usable_right_after_bootstrap() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let state = AppState::bootstrap(tmp.path().join("config"), tmp.path().join("data"))
            .expect("启动应成功");
        let geometry = crate::window::WindowGeometry {
            x: 10,
            y: 20,
            width: 1400,
            height: 900,
            maximized: false,
            monitor: None,
        };
        let mut tracker = state.window_tracker().lock().expect("窗口节流锁中毒");
        assert_eq!(
            tracker.observe(chrono::Utc::now(), geometry.clone()),
            Some(geometry),
            "首次事件应判定落盘"
        );
    }

    #[tokio::test]
    async fn local_db_and_remote_are_usable_right_after_bootstrap() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let state = AppState::bootstrap(tmp.path().join("config"), tmp.path().join("data"))
            .expect("启动应成功");
        assert_eq!(
            state.api_url(),
            crate::config::DEFAULT_API_URL,
            "缺省探测目标"
        );
        // 本地数据库读写闭环
        let record = crate::offline::QuoteRecord::from_market(
            crate::market::synthetic_quote("600519"),
            chrono::Utc::now(),
        );
        crate::offline::save_quote(state.kv(), &record)
            .await
            .expect("落库");
        let loaded = crate::offline::load_quote(state.kv(), "600519")
            .await
            .expect("读取")
            .expect("应有快照");
        assert_eq!(loaded, record, "经 state.kv() 的读写闭环");
        // 远端缝可用
        let fetched = state.remote().fetch("000001").await.expect("演示远端");
        assert_eq!(fetched.symbol, "000001");
    }

    #[tokio::test]
    async fn bootstrap_app_carries_configured_api_url_into_state() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let config_dir = tmp.path().join("config");
        let config = crate::config::AppConfig {
            api_url: "http://backend.internal:9000".to_string(),
            ..crate::config::AppConfig::default()
        };
        crate::config::save(&config_dir.join(crate::paths::CONFIG_FILE_NAME), &config)
            .expect("写配置");
        let (state, _payload) = bootstrap_app(&config_dir, tmp.path().join("data")).expect("启动");
        assert_eq!(
            state.api_url(),
            "http://backend.internal:9000",
            "探测目标应取生效配置"
        );
    }
}
