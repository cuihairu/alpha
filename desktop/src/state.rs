//! 应用状态：分析引擎 + 目录布局（Tauri `manage` 注入的载荷）
//!
//! 只含纯 Rust 类型，故可在无 GUI 环境构造并测试；Tauri 接线层用它做命令参数。

use crate::error::DesktopResult;
use crate::paths::AppPaths;
use alpha_core::analytics::AnalysisEngine;

/// 全局应用状态
#[derive(Debug)]
pub struct AppState {
    engine: AnalysisEngine,
    paths: AppPaths,
}

impl AppState {
    /// 以已解析的目录启动：建目录 + 初始化分析引擎
    pub fn bootstrap(
        config_dir: impl Into<std::path::PathBuf>,
        data_dir: impl Into<std::path::PathBuf>,
    ) -> DesktopResult<Self> {
        let paths = AppPaths::new(config_dir, data_dir);
        paths.ensure()?;
        Ok(Self {
            engine: AnalysisEngine::new(),
            paths,
        })
    }

    /// 仅构造不建目录（测试/只读场景）
    pub fn with_paths(paths: AppPaths) -> Self {
        Self {
            engine: AnalysisEngine::new(),
            paths,
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
}
