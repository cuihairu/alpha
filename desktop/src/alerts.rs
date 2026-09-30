//! 价格告警的本地存储（配置目录 `alerts.json`）
//!
//! 与配置同口径：原子写（临时文件 + rename）、损坏文件容错回退空表。
//! 触发与系统通知属 TODO L114（系统通知/托盘），此处只管持久化。

use crate::error::{DesktopError, DesktopResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// 告警方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertKind {
    /// 价格上穿目标价
    Above,
    /// 价格下穿目标价
    Below,
}

impl AlertKind {
    /// 解析前端传入的方向串
    pub fn parse(raw: &str) -> DesktopResult<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "above" => Ok(Self::Above),
            "below" => Ok(Self::Below),
            other => Err(DesktopError::InvalidInput(format!(
                "不支持的告警方向: {other}（支持 above/below）"
            ))),
        }
    }

    /// 稳定标签
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Above => "above",
            Self::Below => "below",
        }
    }

    /// 方向串是否与给定价格匹配（触发判定；L114 通知接线时复用）
    pub fn matches(self, price: f64, target: f64) -> bool {
        match self {
            Self::Above => price >= target,
            Self::Below => price <= target,
        }
    }
}

/// 单条告警
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Alert {
    /// 标的代码
    pub symbol: String,
    /// 目标价
    pub target_price: f64,
    /// 触发方向
    pub alert_type: AlertKind,
    /// 创建时刻
    pub created_at: DateTime<Utc>,
    /// 是否生效
    pub active: bool,
}

/// 告警集合类型（key = 告警 id）
pub type AlertBook = HashMap<String, Alert>;

/// 告警 id：`<symbol>_<unix 秒>`（与既有实现口径一致）
pub fn alert_id(symbol: &str, at: DateTime<Utc>) -> String {
    format!("{symbol}_{}", at.timestamp())
}

/// 读取告警集合：文件缺失/损坏均回退空表（不让坏文件挡住应用启动）
pub fn load(path: &Path) -> AlertBook {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return AlertBook::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// 原子写入告警集合
pub fn save(path: &Path, alerts: &AlertBook) -> DesktopResult<()> {
    let json = serde_json::to_string_pretty(alerts)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// 新增/覆盖一条告警，返回其 id（覆盖 = 同 id 时替换）
pub fn upsert(
    path: &Path,
    symbol: &str,
    target_price: f64,
    kind: AlertKind,
    at: DateTime<Utc>,
) -> DesktopResult<String> {
    if symbol.trim().is_empty() {
        return Err(DesktopError::InvalidInput("标的代码不能为空".to_string()));
    }
    if !target_price.is_finite() || target_price <= 0.0 {
        return Err(DesktopError::InvalidInput("目标价须为正有限数".to_string()));
    }
    let mut alerts = load(path);
    let id = alert_id(symbol, at);
    alerts.insert(
        id.clone(),
        Alert {
            symbol: symbol.to_string(),
            target_price,
            alert_type: kind,
            created_at: at,
            active: true,
        },
    );
    save(path, &alerts)?;
    Ok(id)
}

/// 读取全部告警（按 id 排序，便于前端稳定展示）
pub fn list(path: &Path) -> Vec<(String, Alert)> {
    let mut items: Vec<(String, Alert)> = load(path).into_iter().collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    items
}

/// 停用告警（保留记录，不物理删除——审计需要）
pub fn deactivate(path: &Path, id: &str) -> DesktopResult<bool> {
    let mut alerts = load(path);
    match alerts.get_mut(id) {
        Some(alert) => {
            alert.active = false;
            save(path, &alerts)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("固定时间戳")
    }

    #[test]
    fn kind_parses_known_directions() {
        assert_eq!(AlertKind::parse("above").expect("above"), AlertKind::Above);
        assert_eq!(
            AlertKind::parse(" BELOW ").expect("below"),
            AlertKind::Below
        );
        assert_eq!(AlertKind::Above.as_str(), "above");
    }

    #[test]
    fn kind_rejects_unknown_direction() {
        let err = AlertKind::parse("sideways").expect_err("应判非法");
        assert_eq!(err.kind(), "invalid_input");
    }

    #[test]
    fn kind_matches_price_direction() {
        assert!(AlertKind::Above.matches(101.0, 100.0));
        assert!(AlertKind::Above.matches(100.0, 100.0), "含等于");
        assert!(!AlertKind::Above.matches(99.0, 100.0));
        assert!(AlertKind::Below.matches(99.0, 100.0));
        assert!(AlertKind::Below.matches(100.0, 100.0));
        assert!(!AlertKind::Below.matches(101.0, 100.0));
    }

    #[test]
    fn load_missing_file_is_empty() {
        let tmp = tempfile::tempdir().expect("临时目录");
        assert!(load(&tmp.path().join("alerts.json")).is_empty());
    }

    #[test]
    fn load_corrupt_file_falls_back_to_empty() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let path = tmp.path().join("alerts.json");
        std::fs::write(&path, b"{ broken").expect("写坏文件");
        assert!(load(&path).is_empty(), "坏文件不应阻塞启动");
    }

    #[test]
    fn upsert_persists_and_returns_id() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let path = tmp.path().join("alerts.json");
        let id = upsert(&path, "600519", 1500.0, AlertKind::Above, at()).expect("写入");

        assert_eq!(id, "600519_1700000000");
        let alerts = load(&path);
        let alert = alerts.get(&id).expect("应持久化");
        assert_eq!(alert.symbol, "600519");
        assert_eq!(alert.target_price, 1500.0);
        assert_eq!(alert.alert_type, AlertKind::Above);
        assert!(alert.active);
        assert_eq!(alert.created_at, at());
    }

    #[test]
    fn upsert_accumulates_multiple_alerts() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let path = tmp.path().join("alerts.json");
        upsert(&path, "600519", 1500.0, AlertKind::Above, at()).expect("第一条");
        upsert(&path, "000001", 12.0, AlertKind::Below, at()).expect("第二条");
        assert_eq!(load(&path).len(), 2);
    }

    #[test]
    fn upsert_same_id_replaces_entry() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let path = tmp.path().join("alerts.json");
        let first = upsert(&path, "600519", 1500.0, AlertKind::Above, at()).expect("首次");
        let second = upsert(&path, "600519", 1600.0, AlertKind::Above, at()).expect("覆盖");
        assert_eq!(first, second, "同秒同标的应命中原 id");
        let alerts = load(&path);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[&second].target_price, 1600.0, "值应被更新");
    }

    #[test]
    fn upsert_rejects_blank_symbol_and_bad_price() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let path = tmp.path().join("alerts.json");
        assert!(upsert(&path, " ", 10.0, AlertKind::Above, at()).is_err());
        assert!(upsert(&path, "X", 0.0, AlertKind::Above, at()).is_err());
        assert!(upsert(&path, "X", -1.0, AlertKind::Above, at()).is_err());
        assert!(upsert(&path, "X", f64::NAN, AlertKind::Above, at()).is_err());
        assert!(!path.exists(), "非法输入不应落盘");
    }

    #[test]
    fn deactivate_keeps_record_but_clears_flag() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let path = tmp.path().join("alerts.json");
        let id = upsert(&path, "600519", 1500.0, AlertKind::Above, at()).expect("写入");
        assert!(deactivate(&path, &id).expect("停用应成功"));
        assert!(!load(&path)[&id].active, "应标记为停用");
        assert_eq!(load(&path).len(), 1, "记录应保留");
        assert!(
            !deactivate(&path, "ghost").expect("停用不存在 id"),
            "不存在 id 返回 false"
        );
    }

    #[test]
    fn list_is_sorted_by_id() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let path = tmp.path().join("alerts.json");
        upsert(&path, "MSFT", 300.0, AlertKind::Above, at()).expect("MSFT");
        upsert(&path, "AAPL", 200.0, AlertKind::Above, at()).expect("AAPL");
        let ids: Vec<String> = list(&path).into_iter().map(|(id, _)| id).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted, "list 应稳定有序");
    }

    #[test]
    fn save_leaves_no_tmp_file() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let path = tmp.path().join("alerts.json");
        upsert(&path, "600519", 1500.0, AlertKind::Above, at()).expect("写入");
        let names: Vec<String> = std::fs::read_dir(tmp.path())
            .expect("列目录")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["alerts.json".to_string()], "残留: {names:?}");
    }

    #[test]
    fn alert_book_roundtrips_json_with_direction_label() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let path = tmp.path().join("alerts.json");
        upsert(&path, "600519", 1500.0, AlertKind::Below, at()).expect("写入");
        let raw = std::fs::read_to_string(&path).expect("读文件");
        assert!(raw.contains("\"below\""), "方向应落为可读标签: {raw}");
        assert_eq!(
            load(&path)[&alert_id("600519", at())].alert_type,
            AlertKind::Below
        );
    }
}
