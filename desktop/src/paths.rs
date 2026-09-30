//! 应用目录布局：配置目录 / 数据目录 / 导出目录 / 键值目录
//!
//! 路径由接线层用 Tauri 的 `path_resolver` 解析后注入（平台差异不出框架层），
//! 框架层只负责「拿到路径后干什么」：派生文件位置并幂等建目录，测试直接注入临时目录。

use std::path::{Path, PathBuf};

/// 配置文件名（落在配置目录）
pub const CONFIG_FILE_NAME: &str = "config.json";
/// 告警文件名（落在配置目录）
pub const ALERTS_FILE_NAME: &str = "alerts.json";
/// 导出子目录（相对数据目录）
pub const EXPORTS_DIR_NAME: &str = "exports";
/// 键值存储子目录（相对数据目录）
pub const KV_DIR_NAME: &str = "kv";

/// 应用目录布局
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    config_dir: PathBuf,
    data_dir: PathBuf,
}

impl AppPaths {
    /// 以已解析好的配置/数据目录构造（不触碰文件系统）
    pub fn new(config_dir: impl Into<PathBuf>, data_dir: impl Into<PathBuf>) -> Self {
        Self {
            config_dir: config_dir.into(),
            data_dir: data_dir.into(),
        }
    }

    /// 配置目录（用户偏好、告警等小体积配置）
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// 数据目录（导出、键值等运行时产物）
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// 配置文件路径
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join(CONFIG_FILE_NAME)
    }

    /// 告警文件路径
    pub fn alerts_file(&self) -> PathBuf {
        self.config_dir.join(ALERTS_FILE_NAME)
    }

    /// 导出目录路径
    pub fn exports_dir(&self) -> PathBuf {
        self.data_dir.join(EXPORTS_DIR_NAME)
    }

    /// 键值存储目录路径
    pub fn kv_dir(&self) -> PathBuf {
        self.data_dir.join(KV_DIR_NAME)
    }

    /// 全部需存在的目录（配置、数据、导出、键值）
    pub fn all_dirs(&self) -> [PathBuf; 4] {
        [
            self.config_dir.clone(),
            self.data_dir.clone(),
            self.exports_dir(),
            self.kv_dir(),
        ]
    }

    /// 幂等创建全部目录
    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in self.all_dirs() {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(root: &Path) -> AppPaths {
        AppPaths::new(root.join("config"), root.join("data"))
    }

    #[test]
    fn derives_file_locations_from_dirs() {
        let p = paths(Path::new("/tmp/app"));
        assert_eq!(p.config_file(), Path::new("/tmp/app/config/config.json"));
        assert_eq!(p.alerts_file(), Path::new("/tmp/app/config/alerts.json"));
        assert_eq!(p.exports_dir(), Path::new("/tmp/app/data/exports"));
        assert_eq!(p.kv_dir(), Path::new("/tmp/app/data/kv"));
    }

    #[test]
    fn config_and_data_dirs_are_distinct_scopes() {
        let p = paths(Path::new("/tmp/app"));
        assert!(p.config_dir().starts_with("/tmp/app/config"));
        assert!(p.data_dir().starts_with("/tmp/app/data"));
        assert_ne!(p.config_dir(), p.data_dir());
    }

    #[test]
    fn ensure_creates_all_dirs_and_is_idempotent() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let p = paths(tmp.path());
        p.ensure().expect("首次建目录");
        for dir in p.all_dirs() {
            assert!(dir.is_dir(), "缺目录: {}", dir.display());
        }
        // 幂等：重复调用不报错，且目录内容不被清空
        let marker = p.exports_dir().join("keep.txt");
        std::fs::write(&marker, b"x").expect("写标记");
        p.ensure().expect("重复建目录");
        assert!(marker.exists(), "重复 ensure 不应删除既有内容");
    }

    #[test]
    fn all_dirs_lists_config_and_data_roots() {
        let p = paths(Path::new("/x"));
        let dirs = p.all_dirs();
        assert_eq!(dirs.len(), 4);
        assert!(dirs.contains(&PathBuf::from("/x/config")));
        assert!(dirs.contains(&PathBuf::from("/x/data")));
    }
}
