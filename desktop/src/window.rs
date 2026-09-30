//! 窗口几何持久化与多显示器放置（TODO L115「构建跨平台窗口管理和主题适配」）
//!
//! 纯逻辑全在框架层（Linux 门禁可测）：合法性清洗、跨显示器钳制（拔出显示器/
//! 布局变化后窗口回到主屏而非丢在屏外）、拖动/缩放高频事件的节流落盘判定。
//! 平台差异（物理像素坐标、监视器枚举、最大化状态查询）由接线层
//! `window_gui.rs`（gui 门控）翻译成 [`MonitorRect`]/[`WindowGeometry`] 后进本层。
//!
//! 主题的口径（与 config 模块衔接）：原生窗口装饰的主题在 Tauri 1.x 只能由
//! `tauri.conf.json` 创建期决定（本仓已配 `"theme": "System"` 跟随系统），
//! 运行期无 `set_theme`；配置的 light/dark 覆盖作用于**内容层**（壳层 CSS
//! `data-theme`），规范映射见 [`crate::config::resolve_theme`]。

use crate::error::DesktopResult;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 窗口可见判定阈值：与显示器交集宽/高各 ≥ 此值才算「在屏上」
///（贴边半遮挡仍算可见，避免拖到边缘一点就判丢）
pub const MIN_VISIBLE: u32 = 100;

/// 恢复尺寸下限——宽（与 tauri.conf.json windows[0] 的 minWidth 一致）
pub const MIN_WIDTH: u32 = 1024;
/// 恢复尺寸下限——高（与 tauri.conf.json windows[0] 的 minHeight 一致）
pub const MIN_HEIGHT: u32 = 768;

/// 拖动/缩放期间落盘节流间隔（事件高频，不能每事件一次 IO）
pub const MIN_PERSIST_INTERVAL_MILLIS: i64 = 1000;

/// 显示器矩形（物理像素；接线层从 `tauri::Monitor` 翻译）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorRect {
    /// 左上角横坐标（物理像素，可为负——多屏可为负）
    pub x: i32,
    /// 左上角纵坐标（物理像素，可为负）
    pub y: i32,
    /// 宽（物理像素）
    pub width: u32,
    /// 高（物理像素）
    pub height: u32,
}

impl MonitorRect {
    /// 是否能「看到」窗口：交集宽/高各 ≥ [`MIN_VISIBLE`]
    pub fn shows_window(&self, g: &WindowGeometry) -> bool {
        let overlap_w = (self.x + self.width as i32).min(g.x + g.width as i32) - self.x.max(g.x);
        let overlap_h = (self.y + self.height as i32).min(g.y + g.height as i32) - self.y.max(g.y);
        overlap_w >= MIN_VISIBLE as i32 && overlap_h >= MIN_VISIBLE as i32
    }
}

/// 窗口几何（持久化契约：字段名即 window-state.json 的键）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowGeometry {
    /// 窗口外框左上角横坐标（物理像素）
    pub x: i32,
    /// 窗口外框左上角纵坐标（物理像素）
    pub y: i32,
    /// 内容区宽（物理像素）
    pub width: u32,
    /// 内容区高（物理像素）
    pub height: u32,
    /// 最大化状态（恢复时只 maximize，坐标不钳制）
    pub maximized: bool,
}

/// 合法性清洗：尺寸低于 conf 最小值 → `None`（交还 OS 默认放置）
pub fn sanitize(g: WindowGeometry) -> Option<WindowGeometry> {
    if g.width < MIN_WIDTH || g.height < MIN_HEIGHT {
        return None;
    }
    Some(g)
}

/// 放置决策（接线层在启动恢复时调用）
///
/// * 无保存 → `None`：OS 默认居中，不干预（首启口径）
/// * 保存非法（尺寸过小）→ `None`：宁缺毋滥
/// * 最大化 → 原样返回（坐标无需钳制，接线层只 `maximize()`）
/// * 任一显示器能看到 → 原样返回
/// * 否则（显示器拔出/分辨率变化）→ 钳入 `monitors[0]`（调用方保证主屏排首）
/// * 无监视器信息 → 原样返回（无法判定时宁可不干预）
pub fn resolve_placement(
    saved: Option<WindowGeometry>,
    monitors: &[MonitorRect],
) -> Option<WindowGeometry> {
    let g = sanitize(saved?)?;
    if g.maximized || monitors.iter().any(|m| m.shows_window(&g)) {
        return Some(g);
    }
    let primary = monitors.first()?;
    let width = g.width.min(primary.width).max(MIN_WIDTH.min(primary.width));
    let height = g
        .height
        .min(primary.height)
        .max(MIN_HEIGHT.min(primary.height));
    Some(WindowGeometry {
        x: clamp_axis(g.x, primary.x, primary.width, width),
        y: clamp_axis(g.y, primary.y, primary.height, height),
        width,
        height,
        maximized: false,
    })
}

/// 单轴钳制：落在 `[origin, origin + span - size]`；显示器比窗口还小时贴原点
fn clamp_axis(value: i32, origin: i32, span: u32, size: u32) -> i32 {
    let max = origin + span as i32 - size as i32;
    if max < origin {
        return origin;
    }
    value.clamp(origin, max)
}

/// 读取窗口状态：缺失/损坏 → `None`（与 config/alerts 的容错回退同口径）
pub fn load_window_state(path: &Path) -> Option<WindowGeometry> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// 原子写入窗口状态（临时文件 + rename，与 config/alerts 同口径）
pub fn save_window_state(path: &Path, geometry: &WindowGeometry) -> DesktopResult<()> {
    let json = serde_json::to_string_pretty(geometry)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// 节流落盘判定器（`AppState` 内嵌；接线层移动/缩放事件高频触发）
#[derive(Debug)]
pub struct WindowStateTracker {
    min_interval: Duration,
    last_saved: Option<(DateTime<Utc>, WindowGeometry)>,
}

impl Default for WindowStateTracker {
    fn default() -> Self {
        Self::new(Duration::milliseconds(MIN_PERSIST_INTERVAL_MILLIS))
    }
}

impl WindowStateTracker {
    /// 以指定最小间隔构造（测试注入短间隔用）
    pub fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            last_saved: None,
        }
    }

    /// 高频事件入口：几何变化且距上次落盘 ≥ 间隔 → `Some(需落盘几何)`。
    /// 间隔内的中间变化会被跳过（由 [`Self::flush`] 在关闭时兜底）。
    pub fn observe(&mut self, at: DateTime<Utc>, g: WindowGeometry) -> Option<WindowGeometry> {
        match self.last_saved {
            Some((t, last)) if last == g || at - t < self.min_interval => None,
            _ => {
                self.last_saved = Some((at, g));
                Some(g)
            }
        }
    }

    /// 关闭/退出兜底：与上次落盘不同即 `Some`（不受间隔限制）
    pub fn flush(&mut self, at: DateTime<Utc>, g: WindowGeometry) -> Option<WindowGeometry> {
        match self.last_saved {
            Some((_t, last)) if last != g => {
                self.last_saved = Some((at, g));
                Some(g)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000 + secs, 0).expect("固定时间戳")
    }

    fn geom(x: i32, y: i32, width: u32, height: u32) -> WindowGeometry {
        WindowGeometry {
            x,
            y,
            width,
            height,
            maximized: false,
        }
    }

    fn monitor(x: i32, y: i32, width: u32, height: u32) -> MonitorRect {
        MonitorRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn sanitize_rejects_below_conf_minimums() {
        assert!(sanitize(geom(0, 0, 1024, 768)).is_some(), "下限尺寸合法");
        assert!(sanitize(geom(0, 0, 1023, 768)).is_none(), "宽不足");
        assert!(sanitize(geom(0, 0, 1024, 767)).is_none(), "高不足");
    }

    #[test]
    fn shows_window_requires_visible_area_on_both_axes() {
        let screen = monitor(0, 0, 1920, 1080);
        assert!(screen.shows_window(&geom(0, 0, 1024, 768)), "完整在屏");
        assert!(
            screen.shows_window(&geom(-900, 0, 1024, 768)),
            "露出 124px 可见"
        );
        assert!(
            !screen.shows_window(&geom(-950, 0, 1024, 768)),
            "仅露出 74px"
        );
        assert!(!screen.shows_window(&geom(2000, 0, 1024, 768)), "完全离屏");
    }

    #[test]
    fn resolve_placement_keeps_window_on_any_monitor() {
        let main = monitor(0, 0, 1920, 1080);
        let side = monitor(1920, 0, 2560, 1440);
        let saved = geom(3000, 100, 1400, 900);
        let placed = resolve_placement(Some(saved), &[main, side]).expect("应保留");
        assert_eq!(placed, saved, "副屏可见的窗口原样保留");
    }

    #[test]
    fn resolve_placement_clamps_back_to_primary_when_monitor_gone() {
        let main = monitor(0, 0, 1920, 1080);
        // 上次会话窗口在副屏 (1920..4480)，本次副屏已拔出
        let saved = geom(3000, 100, 1400, 900);
        let placed = resolve_placement(Some(saved), &[main]).expect("应钳回主屏");
        assert_eq!(placed.width, 1400);
        assert_eq!(placed.x, 1920 - 1400, "右缘贴主屏右缘");
        assert_eq!(placed.y, 100, "纵轴本就在范围内");
    }

    #[test]
    fn resolve_placement_handles_none_and_invalid_and_maximized() {
        let main = monitor(0, 0, 1920, 1080);
        assert_eq!(resolve_placement(None, &[main]), None, "无保存不干预");
        assert_eq!(
            resolve_placement(Some(geom(0, 0, 100, 100)), &[main]),
            None,
            "尺寸非法交还 OS 默认"
        );
        let maximized = WindowGeometry {
            x: 3000,
            y: 100,
            width: 1400,
            height: 900,
            maximized: true,
        };
        assert_eq!(
            resolve_placement(Some(maximized), &[main]),
            Some(maximized),
            "最大化原样保留（只恢复 maximize）"
        );
    }

    #[test]
    fn resolve_placement_clamps_size_into_tiny_primary() {
        // 主屏小于窗口最小尺寸：尺寸钳到屏宽（不强撑出屏）
        let tiny = monitor(0, 0, 800, 600);
        let placed =
            resolve_placement(Some(geom(5000, 5000, 1400, 900)), &[tiny]).expect("小屏也要有落点");
        assert_eq!(placed.width, 800);
        assert_eq!(placed.height, 600);
        assert_eq!((placed.x, placed.y), (0, 0), "贴原点");
    }

    #[test]
    fn window_state_roundtrips_and_falls_back_on_missing_or_corrupt() {
        let tmp = tempfile::tempdir().expect("临时目录");
        let path = tmp.path().join("window-state.json");
        assert!(load_window_state(&path).is_none(), "缺失 → None");

        save_window_state(&path, &geom(10, -20, 1400, 900)).expect("落盘");
        assert_eq!(
            load_window_state(&path),
            Some(geom(10, -20, 1400, 900)),
            "往返一致（负坐标保留）"
        );

        std::fs::write(&path, "{not json").expect("写损坏内容");
        assert!(load_window_state(&path).is_none(), "损坏 → None");
    }

    #[test]
    fn tracker_throttles_rapid_events_but_flushes_changes_on_close() {
        let mut tracker = WindowStateTracker::new(Duration::milliseconds(1000));
        let g0 = geom(0, 0, 1400, 900);
        assert_eq!(tracker.observe(at(0), g0), Some(g0), "首次必落盘");
        assert_eq!(tracker.observe(at(0), g0), None, "同几何不重写");
        assert_eq!(
            tracker.observe(at(0), geom(30, 0, 1400, 900)),
            None,
            "间隔内变化跳过"
        );
        assert_eq!(
            tracker.observe(at(1), geom(30, 0, 1400, 900)),
            Some(geom(30, 0, 1400, 900)),
            "过间隔的变化落盘"
        );
        assert_eq!(
            tracker.flush(at(1), geom(30, 0, 1400, 900)),
            None,
            "关闭时无变化不写"
        );

        let g2 = geom(60, 0, 1400, 900);
        assert_eq!(tracker.observe(at(1), g2), None, "间隔内的最后一次变化暂存");
        assert_eq!(tracker.flush(at(2), g2), Some(g2), "关闭兜底强制落盘");
        assert_eq!(tracker.flush(at(2), g2), None, "兜底不重复");
    }
}
