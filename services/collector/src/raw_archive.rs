//! 原始响应归档（RawArchiver，architecture-review §2.1 权威后端三件套的
//! MinIO/S3 原始归档写路径）。
//!
//! 爬虫脚本把响应原文（字节级）与元信息 best-effort 落到任务工作目录
//! （`raw_response.txt` / `raw_meta.json`），本模块在任务完成后把它们
//! 上传到 S3/MinIO——数据质量告警（背离/异常检测）的取证底座。
//!
//! 门控：`ALPHA_COLLECTOR_RAW_ARCHIVE_URL` 未设置 = 关闭（默认，零行为变化）。
//! 旁路语义：归档失败只记指标与 warn，**不影响任务状态**——采集可用性
//! 永远优先于归档完整性。
//!
//! 覆盖边界（诚实登记）：raw 落盘面目前 = `crawlers/python/eastmoney_quote.py`
//! 与 inline Python/NodeJs 模板；Go/Shell 模板及 sources/ 下的 Rust 源适配器
//! 尚不落 raw（Rust 模板在 simple runner 本就不支持执行）。指标：
//! `alpha_collector_raw_archive_total{result=uploaded|failed}`。

use std::path::{Path, PathBuf};

use alpha_storage::CloudStorage;
use chrono::Utc;

/// 工作目录内登记的归档产物（固定文件名，脚本侧按此约定落盘）
pub const RAW_BODY_FILE: &str = "raw_response.txt";
pub const RAW_META_FILE: &str = "raw_meta.json";

/// 归档器：None = 未启用（默认）
#[derive(Clone)]
pub struct RawArchiver {
    sink: CloudStorage,
    prefix: String,
}

impl RawArchiver {
    /// env 装配：`ALPHA_COLLECTOR_RAW_ARCHIVE_URL`（形如
    /// `s3://bucket?endpoint=http://minio:9000&access_key=..&secret_key=..`，
    /// 与 storage CloudStorage::from_connection_string 同参）；
    /// `ALPHA_COLLECTOR_RAW_ARCHIVE_PREFIX` 可选，缺省 `raw`。
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        let url = std::env::var("ALPHA_COLLECTOR_RAW_ARCHIVE_URL")
            .ok()
            .unwrap_or_default();
        Self::from_url(
            &url,
            &std::env::var("ALPHA_COLLECTOR_RAW_ARCHIVE_PREFIX")
                .ok()
                .unwrap_or_default(),
        )
    }

    /// 纯参数装配（测试与 from_env 共用）：URL 空 = 未启用；
    /// 非空但连接串非法 = 启动期报错（配置错误不静默吞）
    pub fn from_url(url: &str, prefix: &str) -> anyhow::Result<Option<Self>> {
        if url.trim().is_empty() {
            return Ok(None);
        }
        let prefix = if prefix.trim().is_empty() {
            "raw".to_string()
        } else {
            prefix.trim().trim_matches('/').to_string()
        };
        let sink = CloudStorage::from_connection_string(url)
            .map_err(|e| anyhow::anyhow!("原始归档连接串非法: {e}"))?;
        Ok(Some(Self { sink, prefix }))
    }

    pub fn enabled(&self) -> bool {
        true
    }

    /// 对象键布局：`{prefix}/{YYYY-MM-DD}/{task_id}/{filename}`
    /// （日期分片便于生命周期策略按天过期）
    pub fn object_key(&self, task_id: &str, filename: &str) -> String {
        format!(
            "{}/{}/{}/{}",
            self.prefix,
            Utc::now().format("%Y-%m-%d"),
            task_id,
            filename
        )
    }

    /// 扫描工作目录内的登记产物（缺哪个跳哪个，不报错）
    pub fn artifacts_in(dir: &Path) -> Vec<PathBuf> {
        [RAW_BODY_FILE, RAW_META_FILE]
            .iter()
            .map(|name| dir.join(name))
            .filter(|path| path.is_file())
            .collect()
    }

    /// 归档一个任务的工作目录产物，返回实际上传数。
    /// 调用方（execute_task）负责把 Err 降级为指标+warn，不外溢到任务状态。
    pub async fn archive_task(&self, task_id: &str, dir: &Path) -> anyhow::Result<usize> {
        let mut uploaded = 0;
        for artifact in Self::artifacts_in(dir) {
            let filename = artifact
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            let data = tokio::fs::read(&artifact).await?;
            let key = self.object_key(task_id, &filename);
            // bucket 语义归 from_connection_string 的 host 部分；上传走
            // CloudStorage 自带 bucket（config 内），这里只给 key
            self.sink.upload_object(&key, data).await?;
            uploaded += 1;
        }
        Ok(uploaded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// env 未设置 = 未启用（默认零行为变化）
    #[test]
    fn from_url_empty_is_disabled() {
        assert!(RawArchiver::from_url("", "").unwrap().is_none());
        assert!(RawArchiver::from_url("  ", "").unwrap().is_none());
    }

    /// 非法连接串启动期报错（不静默吞配置错误）
    #[test]
    fn from_url_invalid_scheme_errors() {
        assert!(RawArchiver::from_url("::not-a-url::", "").is_err());
    }

    /// 前缀缺省 raw、两侧斜杠收敛；对象键含日期分片
    #[test]
    fn object_key_layout_uses_prefix_date_task() {
        let archiver =
            RawArchiver::from_url("s3://alpha-raw?endpoint=http://127.0.0.1:9000", " /raw/ ")
                .unwrap()
                .unwrap();
        assert_eq!(archiver.prefix, "raw");
        let key = archiver.object_key("task-1", RAW_BODY_FILE);
        let parts: Vec<&str> = key.split('/').collect();
        assert_eq!(parts.len(), 4, "prefix/date/task/filename 四段: {key}");
        assert_eq!(parts[0], "raw");
        assert_eq!(parts[2], "task-1");
        assert_eq!(parts[3], RAW_BODY_FILE);
        // 日期段形如 YYYY-MM-DD
        assert_eq!(parts[1].len(), 10);
        assert_eq!(parts[1].matches('-').count(), 2);
    }

    /// 产物扫描：缺哪个跳哪个，两个都在就都收
    #[test]
    fn artifacts_in_picks_registered_files_only() {
        let dir = std::env::temp_dir().join(format!(
            "alpha-raw-archive-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // 空目录 → 0
        assert!(RawArchiver::artifacts_in(&dir).is_empty());
        // 只有 body → 1
        std::fs::write(dir.join(RAW_BODY_FILE), "{}").unwrap();
        assert_eq!(RawArchiver::artifacts_in(&dir).len(), 1);
        // body + meta → 2；其他文件不收
        std::fs::write(dir.join(RAW_META_FILE), "{}").unwrap();
        std::fs::write(dir.join("output.json"), "junk").unwrap();
        assert_eq!(RawArchiver::artifacts_in(&dir).len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
