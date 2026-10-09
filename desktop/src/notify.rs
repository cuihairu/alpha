//! 系统通知与托盘状态（TODO L114「开发系统通知和托盘集成功能」）
//!
//! 框架层只落「可测的纯逻辑」：通知模型、有界队列（去重）、托盘状态计算、
//! 告警触发 → 通知文案。平台 API（[`tauri::api::notification::Notification`]
//! 展示、`SystemTrayHandle::set_tooltip`）属接线层——命令体只做「取句柄 →
//! 委派 → `map_err`」，与 L112/L113 同口径。
//!
//! 触发判定复用 `alerts::AlertKind::matches`（L112 已落，其注释标明 L114 复用）。

use crate::alerts::{Alert, AlertKind};
use crate::error::{DesktopError, DesktopResult};
use crate::ipc::NotificationRequest;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::Path;

/// 托盘 id（`Builder::system_tray` 需显式 `with_id`，否则句柄 id 是随机串——
/// `tray_handle_by_id(TRAY_ID)` 才能找得到）
pub const TRAY_ID: &str = "main";

/// 托盘菜单项 id：显示主窗口
pub const TRAY_ITEM_SHOW: &str = "tray-show";
/// 托盘菜单项 id：隐藏主窗口
pub const TRAY_ITEM_HIDE: &str = "tray-hide";
/// 托盘菜单项 id：退出
pub const TRAY_ITEM_QUIT: &str = "tray-quit";

/// 主窗口标签（tauri.conf.json 的 windows[0] 未指定 label，Tauri 1.x 默认 "main"）
pub const MAIN_WINDOW_LABEL: &str = "main";

/// 通知队列默认容量（AppState 内嵌）
pub const DEFAULT_QUEUE_CAPACITY: usize = 50;

/// 通知级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotificationLevel {
    /// 普通信息
    Info,
    /// 警告
    Warning,
    /// 严重（告警触发）
    Critical,
}

impl NotificationLevel {
    /// 解析前端传入的级别串（大小写不敏感）
    pub fn parse(raw: &str) -> DesktopResult<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "info" => Ok(Self::Info),
            "warning" => Ok(Self::Warning),
            "critical" => Ok(Self::Critical),
            other => Err(DesktopError::InvalidInput(format!(
                "不支持的通知级别: {other}（支持 info/warning/critical）"
            ))),
        }
    }

    /// 稳定标签
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }
}

/// 单条通知（IPC 契约：字段名即前端读取键）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    /// 通知 id（`<symbol>_<unix 秒>`）
    pub id: String,
    /// 标的代码
    pub symbol: String,
    /// 标题
    pub title: String,
    /// 正文
    pub body: String,
    /// 级别
    pub level: NotificationLevel,
    /// 创建时刻
    pub created_at: DateTime<Utc>,
}

/// 托盘状态（IPC 契约：命令返回值经 Tauri 序列化给前端）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrayState {
    /// 状态文本（托盘 tooltip）
    pub status_text: String,
    /// 生效告警数
    pub active_alerts: usize,
    /// 最近一次触发的标的（无则 None）
    pub last_trigger: Option<String>,
}

/// 通知 id：`<symbol>_<unix 秒>`（与 `alerts::alert_id` 同口径）
pub fn notification_id(symbol: &str, at: DateTime<Utc>) -> String {
    format!("{symbol}_{}", at.timestamp())
}

/// 通知队列：有界 FIFO + 同标题正文去重（窗口内重复触发只保留一条）
#[derive(Debug)]
pub struct NotificationQueue {
    capacity: usize,
    entries: VecDeque<Notification>,
}

impl NotificationQueue {
    /// 创建队列（容量须 > 0）
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "通知队列容量须大于 0");
        Self {
            capacity,
            entries: VecDeque::new(),
        }
    }

    /// 入队；同标题正文已在队中（去重）返回 false，否则 true
    pub fn push(&mut self, notification: Notification) -> bool {
        if self
            .entries
            .iter()
            .any(|n| n.title == notification.title && n.body == notification.body)
        {
            return false;
        }
        self.entries.push_back(notification);
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
        true
    }

    /// 队列长度
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 容量
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 最近 n 条（新 → 旧）
    pub fn recent(&self, n: usize) -> Vec<Notification> {
        self.entries.iter().rev().take(n).cloned().collect()
    }
}

/// 发送通知（`send_notification` 命令的实现体）
///
/// 级别串解析、标的/标题判空、id 生成、入队都在框架层；接线层只负责
/// 平台 API 展示。返回构建好的通知（前端提示用）。
pub fn notify_request(
    request: &NotificationRequest,
    queue: &mut NotificationQueue,
    at: DateTime<Utc>,
) -> DesktopResult<Notification> {
    let level = NotificationLevel::parse(&request.level)?;
    if request.symbol.trim().is_empty() {
        return Err(DesktopError::InvalidInput("标的代码不能为空".to_string()));
    }
    if request.title.trim().is_empty() {
        return Err(DesktopError::InvalidInput("通知标题不能为空".to_string()));
    }
    let notification = Notification {
        id: notification_id(&request.symbol, at),
        symbol: request.symbol.clone(),
        title: request.title.clone(),
        body: request.body.clone(),
        level,
        created_at: at,
    };
    queue.push(notification.clone());
    Ok(notification)
}

/// 由告警集合计算托盘状态（`set_tray_status` 命令的实现体）
///
/// 读告警文件（损坏回退空表，与 `alerts::load` 同口径）→ 统计生效告警 →
/// 最近触发标的。无告警时状态文本为「无告警」。
pub fn tray_status_request(path: &Path) -> TrayState {
    let alerts = crate::alerts::list(path);
    let active: Vec<&Alert> = alerts
        .iter()
        .filter(|(_, a)| a.active)
        .map(|(_, a)| a)
        .collect();
    let last_trigger = active
        .iter()
        .max_by_key(|a| a.created_at)
        .map(|a| a.symbol.clone());
    let status_text = if active.is_empty() {
        "无告警".to_string()
    } else {
        format!("{} 个告警生效中", active.len())
    };
    TrayState {
        status_text,
        active_alerts: active.len(),
        last_trigger,
    }
}

/// 告警触发 → 通知（复用 `AlertKind::matches` 判定；未触发/已停用返回 None）
pub fn alert_notification(alert: &Alert, price: f64, at: DateTime<Utc>) -> Option<Notification> {
    if !alert.active || !alert.alert_type.matches(price, alert.target_price) {
        return None;
    }
    let direction = match alert.alert_type {
        AlertKind::Above => "上穿",
        AlertKind::Below => "下穿",
    };
    Some(Notification {
        id: notification_id(&alert.symbol, at),
        symbol: alert.symbol.clone(),
        title: format!(
            "价格告警：{} {} {:.2}",
            alert.symbol, direction, alert.target_price
        ),
        body: format!("当前价 {:.2}，目标价 {:.2}", price, alert.target_price),
        level: NotificationLevel::Critical,
        created_at: at,
    })
}

/// 托盘菜单项（框架层模型；接线层机械翻译为 `SystemTrayMenu`，不掺判断）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayEntry {
    /// 可点击项
    Item {
        /// 稳定 id（`tray_action` 的键）
        id: &'static str,
        /// 菜单文案
        label: &'static str,
        /// 是否可点（状态机：与主窗可见性互补）
        enabled: bool,
    },
    /// 分隔线
    Separator,
}

/// 托盘菜单动作（id → 动作的映射在框架层，接线层只执行平台调用）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// 显示主窗口并聚焦
    ShowWindow,
    /// 隐藏主窗口
    HideWindow,
    /// 退出进程
    Quit,
}

/// 按主窗可见性生成托盘菜单（状态机：显示/隐藏随可见性互斥可用，分隔线隔开退出）
///
/// 常用菜单三件套即 TODO L114 的口径：显示/隐藏主窗、退出。
pub fn tray_menu_model(window_visible: bool) -> Vec<TrayEntry> {
    vec![
        TrayEntry::Item {
            id: TRAY_ITEM_SHOW,
            label: "显示主窗口",
            enabled: !window_visible,
        },
        TrayEntry::Item {
            id: TRAY_ITEM_HIDE,
            label: "隐藏主窗口",
            enabled: window_visible,
        },
        TrayEntry::Separator,
        TrayEntry::Item {
            id: TRAY_ITEM_QUIT,
            label: "退出",
            enabled: true,
        },
    ]
}

/// 菜单项 id → 动作（未知 id 返回 None，接线层忽略而非 panic）
pub fn tray_action(id: &str) -> Option<TrayAction> {
    match id {
        TRAY_ITEM_SHOW => Some(TrayAction::ShowWindow),
        TRAY_ITEM_HIDE => Some(TrayAction::HideWindow),
        TRAY_ITEM_QUIT => Some(TrayAction::Quit),
        _ => None,
    }
}

/// 通知点击动作 id：XDG 通知规范约定点击通知本体触发 `default` 动作（注册时不
/// 显示为按钮）；notify-rust 4 在通知关闭（超时/手动消去）时回调 `__closed` 哨兵。
pub const NOTIFY_ACTION_OPEN: &str = "default";

/// 通知点击动作 → 窗口动作（未知动作返回 None，接线层忽略而非 panic）
///
/// 点击通知本体与托盘「显示主窗口」同语义，复用 [`TrayAction::ShowWindow`]——
/// 唤起主窗保持单一路径；`__closed` 与未知 id 都不是「打开」。
pub fn notification_click_action(action: &str) -> Option<TrayAction> {
    if action == NOTIFY_ACTION_OPEN {
        Some(TrayAction::ShowWindow)
    } else {
        None
    }
}

/// 告警检查（`check_alerts` 命令的实现体）：告警集合 → 触发判定 → 通知入队 +
/// 触发者停用落盘。返回「本次真正新入队」的通知——平台只弹这些，重复内容由
/// [`NotificationQueue`] 的同文去重抑制，不再打扰用户。
///
/// 持久化沿用 `alerts` 既有语义：触发即 `deactivate`（停用保留记录），
/// 避免确定性行情下同一告警每次检查都重复触发。
pub fn check_request(
    path: &Path,
    queue: &mut NotificationQueue,
    at: DateTime<Utc>,
) -> DesktopResult<Vec<Notification>> {
    let mut fired = Vec::new();
    for (id, alert) in crate::alerts::load(path) {
        if !alert.active {
            continue;
        }
        // 取数与 `quotes_request` 同一口径（`market::synthetic_quote`，确定性演示行情）
        let price = crate::market::synthetic_quote(&alert.symbol).price;
        if let Some(notification) = alert_notification(&alert, price, at) {
            let fresh = queue.push(notification.clone());
            // 先入队再落盘：落盘失败则保留 active，下次检查可重试停用
            //（已入队内容会被去重，不产生重复弹窗）
            crate::alerts::deactivate(path, &id)?;
            if fresh {
                fired.push(notification);
            }
        }
    }
    Ok(fired)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("固定时间戳")
    }

    fn request(level: &str) -> NotificationRequest {
        NotificationRequest {
            symbol: "600519".to_string(),
            title: "测试标题".to_string(),
            body: "测试正文".to_string(),
            level: level.to_string(),
        }
    }

    #[test]
    fn level_parses_known_levels_case_insensitively() {
        assert_eq!(
            NotificationLevel::parse("info").expect("info"),
            NotificationLevel::Info
        );
        assert_eq!(
            NotificationLevel::parse(" Warning ").expect("warning"),
            NotificationLevel::Warning
        );
        assert_eq!(
            NotificationLevel::parse("CRITICAL").expect("critical"),
            NotificationLevel::Critical
        );
        assert_eq!(NotificationLevel::Critical.as_str(), "critical");
    }

    #[test]
    fn level_rejects_unknown() {
        let err = NotificationLevel::parse("loud").expect_err("应判非法");
        assert_eq!(err.kind(), "invalid_input");
        assert!(err.to_string().contains("loud"), "实际: {err}");
    }

    #[test]
    fn notify_request_builds_notification_with_id_and_enqueues() {
        let mut queue = NotificationQueue::new(10);
        let notification = notify_request(&request("info"), &mut queue, at()).expect("应构建");

        assert_eq!(notification.id, "600519_1700000000");
        assert_eq!(notification.level, NotificationLevel::Info);
        assert_eq!(notification.created_at, at());
        assert_eq!(queue.len(), 1, "应入队");
    }

    #[test]
    fn notify_request_rejects_blank_symbol_and_title_without_enqueue() {
        let mut queue = NotificationQueue::new(10);
        let mut req = request("info");
        req.symbol = "  ".to_string();
        assert!(notify_request(&req, &mut queue, at()).is_err());

        let mut req = request("info");
        req.title = " ".to_string();
        let err = notify_request(&req, &mut queue, at()).expect_err("空标题应拒绝");
        assert_eq!(err.kind(), "invalid_input");
        assert!(queue.is_empty(), "拒绝时不应入队");
    }

    #[test]
    fn notify_request_rejects_unknown_level_before_enqueue() {
        let mut queue = NotificationQueue::new(10);
        let err = notify_request(&request("x"), &mut queue, at()).expect_err("未知级别应拒绝");
        assert_eq!(err.kind(), "invalid_input");
        assert!(queue.is_empty());
    }

    #[test]
    fn queue_dedups_same_title_and_body() {
        let mut queue = NotificationQueue::new(10);
        notify_request(&request("info"), &mut queue, at()).expect("首次");
        notify_request(&request("info"), &mut queue, at()).expect("重复入队");
        assert_eq!(queue.len(), 1, "重复标题正文只保留一条");
    }

    #[test]
    fn queue_keeps_same_title_with_different_body() {
        let mut queue = NotificationQueue::new(10);
        notify_request(&request("info"), &mut queue, at()).expect("首次");
        let mut req = request("info");
        req.body = "不同正文".to_string();
        notify_request(&req, &mut queue, at()).expect("不同正文应入队");
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn queue_evicts_oldest_beyond_capacity() {
        let mut queue = NotificationQueue::new(2);
        for i in 0..3 {
            let mut req = request("info");
            req.title = format!("标题{i}");
            notify_request(&req, &mut queue, at()).expect("入队");
        }
        assert_eq!(queue.len(), 2, "容量恒为 2");
        let titles: Vec<String> = queue.recent(10).iter().map(|n| n.title.clone()).collect();
        assert_eq!(
            titles,
            vec!["标题2".to_string(), "标题1".to_string()],
            "最旧的应被驱逐，recent 新→旧"
        );
    }

    #[test]
    fn queue_zero_capacity_rejected() {
        let result = std::panic::catch_unwind(|| NotificationQueue::new(0));
        assert!(result.is_err(), "容量 0 必须构造期拒绝");
    }

    #[test]
    fn recent_returns_newest_first() {
        let mut queue = NotificationQueue::new(10);
        for i in 0..3 {
            let mut req = request("info");
            req.title = format!("t{i}");
            notify_request(&req, &mut queue, at()).expect("入队");
        }
        let recent = queue.recent(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].title, "t2");
        assert_eq!(recent[1].title, "t1");
    }

    #[test]
    fn tray_status_empty_when_no_alerts() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let status = tray_status_request(&tmp.path().join("alerts.json"));
        assert_eq!(status.status_text, "无告警");
        assert_eq!(status.active_alerts, 0);
        assert!(status.last_trigger.is_none());
    }

    #[test]
    fn tray_status_counts_active_alerts_and_reports_latest() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let path = tmp.path().join("alerts.json");
        crate::alerts::upsert(&path, "600519", 1500.0, AlertKind::Above, at()).expect("告警1");
        crate::alerts::upsert(&path, "000001", 12.0, AlertKind::Below, at()).expect("告警2");
        crate::alerts::deactivate(&path, &crate::alerts::alert_id("600519", at())).expect("停用1");

        let status = tray_status_request(&path);
        assert_eq!(status.active_alerts, 1, "停用不计入");
        assert_eq!(status.status_text, "1 个告警生效中");
        assert_eq!(status.last_trigger.as_deref(), Some("000001"));
    }

    #[test]
    fn alert_notification_fires_only_when_matched_and_active() {
        let alert = Alert {
            symbol: "600519".to_string(),
            target_price: 100.0,
            alert_type: AlertKind::Above,
            created_at: at(),
            active: true,
        };
        let fired = alert_notification(&alert, 101.0, at()).expect("上穿应触发");
        assert_eq!(fired.level, NotificationLevel::Critical);
        assert!(fired.title.contains("上穿"), "实际: {}", fired.title);
        assert!(fired.body.contains("101"), "实际: {}", fired.body);
        assert!(
            alert_notification(&alert, 99.0, at()).is_none(),
            "未触发应 None"
        );
        let mut inactive = alert.clone();
        inactive.active = false;
        assert!(
            alert_notification(&inactive, 101.0, at()).is_none(),
            "停用应 None"
        );
    }

    #[test]
    fn alert_notification_below_direction() {
        let alert = Alert {
            symbol: "000001".to_string(),
            target_price: 12.0,
            alert_type: AlertKind::Below,
            created_at: at(),
            active: true,
        };
        let fired = alert_notification(&alert, 11.0, at()).expect("下穿应触发");
        assert!(fired.title.contains("下穿"), "实际: {}", fired.title);
    }

    #[test]
    fn notification_and_tray_state_serialize_for_command_boundary() {
        let notification =
            notify_request(&request("warning"), &mut NotificationQueue::new(5), at())
                .expect("构建");
        let json = serde_json::to_value(&notification).expect("序列化");
        for key in ["id", "symbol", "title", "body", "level", "created_at"] {
            assert!(json.get(key).is_some(), "通知应含字段 {key}: {json}");
        }
        assert_eq!(json["level"], "warning");
        let back: Notification = serde_json::from_value(json).expect("往返");
        assert_eq!(back, notification);

        let tmp = tempfile::tempdir().expect("临时目录");
        let status = tray_status_request(&tmp.path().join("alerts.json"));
        let json = serde_json::to_value(&status).expect("序列化");
        for key in ["status_text", "active_alerts", "last_trigger"] {
            assert!(json.get(key).is_some(), "托盘状态应含字段 {key}: {json}");
        }
    }

    #[test]
    fn tray_menu_enables_only_the_available_side() {
        for (visible, show_enabled, hide_enabled) in [(true, false, true), (false, true, false)] {
            let model = tray_menu_model(visible);
            assert_eq!(
                model,
                vec![
                    TrayEntry::Item {
                        id: TRAY_ITEM_SHOW,
                        label: "显示主窗口",
                        enabled: show_enabled,
                    },
                    TrayEntry::Item {
                        id: TRAY_ITEM_HIDE,
                        label: "隐藏主窗口",
                        enabled: hide_enabled,
                    },
                    TrayEntry::Separator,
                    TrayEntry::Item {
                        id: TRAY_ITEM_QUIT,
                        label: "退出",
                        enabled: true,
                    },
                ],
                "可见性 {visible} 的菜单结构/可用态"
            );
        }
    }

    #[test]
    fn tray_action_maps_known_ids_and_ignores_unknown() {
        assert_eq!(tray_action(TRAY_ITEM_SHOW), Some(TrayAction::ShowWindow));
        assert_eq!(tray_action(TRAY_ITEM_HIDE), Some(TrayAction::HideWindow));
        assert_eq!(tray_action(TRAY_ITEM_QUIT), Some(TrayAction::Quit));
        assert_eq!(tray_action("tray-nope"), None, "未知 id 不 panic");
        assert_eq!(tray_action(TRAY_ID), None, "托盘 id 不是菜单项 id");
    }

    #[test]
    fn notification_click_maps_default_only() {
        assert_eq!(
            notification_click_action(NOTIFY_ACTION_OPEN),
            Some(TrayAction::ShowWindow),
            "点击通知本体=唤起主窗（与托盘显示同路径）"
        );
        assert_eq!(notification_click_action("__closed"), None, "关闭不是打开");
        assert_eq!(notification_click_action(""), None);
        assert_eq!(
            notification_click_action(TRAY_ITEM_SHOW),
            None,
            "托盘菜单 id 不是通知动作"
        );
    }

    #[test]
    fn check_request_fires_deactivates_and_stays_quiet_afterwards() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let path = tmp.path().join("alerts.json");
        let price = crate::market::synthetic_quote("600519").price;
        crate::alerts::upsert(&path, "600519", price - 1.0, AlertKind::Above, at())
            .expect("布防上穿告警");

        let mut queue = NotificationQueue::new(10);
        let fired = check_request(&path, &mut queue, at()).expect("首查应触发");
        assert_eq!(fired.len(), 1, "现价已上穿（确定性行情），应触发");
        assert_eq!(fired[0].level, NotificationLevel::Critical);
        assert_eq!(queue.len(), 1, "触发内容应入队");
        let book = crate::alerts::load(&path);
        let id = crate::alerts::alert_id("600519", at());
        assert!(!book[&id].active, "触发即停用（保留记录）");

        // 二次检查：已停用不再触发，队列也不重复入队
        let fired = check_request(&path, &mut queue, at()).expect("二查应空手而归");
        assert!(fired.is_empty(), "停用告警不应再触发: {fired:?}");
        assert_eq!(queue.len(), 1, "队列不因空查变化");
    }

    #[test]
    fn check_request_leaves_unmatched_alerts_active() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let path = tmp.path().join("alerts.json");
        let price = crate::market::synthetic_quote("000001").price;
        crate::alerts::upsert(&path, "000001", price + 1000.0, AlertKind::Above, at())
            .expect("布防远离目标的上穿告警");

        let mut queue = NotificationQueue::new(10);
        let fired = check_request(&path, &mut queue, at()).expect("检查成功");
        assert!(fired.is_empty(), "未上穿不应触发: {fired:?}");
        assert!(queue.is_empty());
        let book = crate::alerts::load(&path);
        let id = crate::alerts::alert_id("000001", at());
        assert!(book[&id].active, "未触发告警保持生效");
    }

    #[test]
    fn check_request_dedup_suppresses_popup_but_not_state_change() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let path = tmp.path().join("alerts.json");
        let price = crate::market::synthetic_quote("600519").price;
        let mut queue = NotificationQueue::new(10);
        crate::alerts::upsert(&path, "600519", price - 1.0, AlertKind::Above, at()).expect("首布");
        assert_eq!(
            check_request(&path, &mut queue, at()).expect("首查").len(),
            1
        );

        // 同目标价重新布防再触发：状态机照常（停用落盘），但同文通知被去重，
        // 不再计入「新入队」→ 平台不重复弹窗
        crate::alerts::upsert(&path, "600519", price - 1.0, AlertKind::Above, at())
            .expect("同目标价重新布防");
        let fired = check_request(&path, &mut queue, at()).expect("复查");
        assert!(fired.is_empty(), "同文重复触发应被队列去重: {fired:?}");
        assert_eq!(queue.len(), 1);
        let book = crate::alerts::load(&path);
        let id = crate::alerts::alert_id("600519", at());
        assert!(!book[&id].active, "去重不影响触发后的停用落盘");
    }

    #[test]
    fn check_request_missing_file_is_empty_not_error() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let mut queue = NotificationQueue::new(10);
        let fired = check_request(&tmp.path().join("alerts.json"), &mut queue, at())
            .expect("缺失告警文件按空表处理");
        assert!(fired.is_empty());
    }
}
