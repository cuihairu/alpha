//! 键盘快捷键与右键菜单模型（TODO L117「开发键盘快捷键和右键菜单支持」）
//!
//! 纯 Rust、零 Tauri 依赖：组合键合法性、默认快捷键表（组合键 ↔ 动作 id）、
//! 跨平台展示标签、右键菜单项模型（文案/可用性/加速器提示）全部在此，可
//! 在无 GUI 环境测试。接线层（`shortcut_gui.rs`）只把表注册进 Tauri 的
//! `GlobalShortcutManager` 并广播事件；壳层按动作 id 分发到既有命令流，
//! 菜单项数据经 `get_context_menu` 命令下发后渲染。
//!
//! 非交互假设（自行判定，已注明）：
//! 1. 快捷键走全局注册（`global-shortcut-all` 特性已在 allowlist）：系统级
//!    生效，故默认表只收**带修饰键**的组合（裸键如 F5 不收，避免劫持系统键）；
//! 2. 右键菜单用内容层 DOM 实现——Tauri 1.x 无原生 context menu API（v2 才有
//!    `Menu::popup`），菜单**数据**来自本模块模型，壳层只渲染与分发；
//! 3. 动作分发在壳层 `ACTIONS` 表（复用既有命令流），本模块只定义动作 id
//!    与组合键映射，不新增动作命令；
//! 4. 导出动作指「首个标的经另存为对话框导出」（与导出卡片同流）；
//!    「复制最新价」是纯前端剪贴板操作，无 Rust 命令。

/// 动作 id：刷新行情（壳层重跑 `get_real_time_quotes`）
pub const ACTION_REFRESH: &str = "refresh_quotes";
/// 动作 id：复制最新价（纯前端剪贴板）
pub const ACTION_COPY_PRICE: &str = "copy_price";
/// 动作 id：导出首个标的（另存为对话框 + `export_symbol_to_file`）
pub const ACTION_EXPORT: &str = "export_symbol";
/// 动作 id：联网增量同步（`sync_offline_data`）
pub const ACTION_SYNC: &str = "sync_offline";
/// 动作 id：读取离线行情（`get_offline_quotes`）
pub const ACTION_READ_OFFLINE: &str = "read_offline";

/// 合法修饰键（Tauri v1 快捷键解析器接受的别名，大小写敏感）
pub const MODIFIERS: &[&str] = &[
    "CmdOrCtrl",
    "CommandOrControl",
    "Ctrl",
    "Control",
    "Meta",
    "Command",
    "Super",
    "Shift",
    "Alt",
    "Option",
];

/// 一条快捷键：组合键串（Tauri 注册格式）→ 动作 id
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShortcutSpec {
    /// 组合键串，如 `CmdOrCtrl+Shift+S`
    pub combo: &'static str,
    /// 触发的动作 id（[`ACTION_REFRESH`] 等常量）
    pub action: &'static str,
}

/// 默认快捷键表（注册顺序即声明顺序；跨平台组合键用 `CmdOrCtrl` 别名，
/// 由 Tauri 按平台映射为 Ctrl/⌘）
pub const DEFAULT_SHORTCUTS: &[ShortcutSpec] = &[
    ShortcutSpec {
        combo: "CmdOrCtrl+R",
        action: ACTION_REFRESH,
    },
    ShortcutSpec {
        combo: "CmdOrCtrl+E",
        action: ACTION_EXPORT,
    },
    ShortcutSpec {
        combo: "CmdOrCtrl+D",
        action: ACTION_READ_OFFLINE,
    },
    ShortcutSpec {
        combo: "CmdOrCtrl+Shift+S",
        action: ACTION_SYNC,
    },
];

/// 单键是否合法：单个 ASCII 字母/数字，或 F1–F12
fn valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return c.is_ascii_alphanumeric();
    }
    key.len() >= 2
        && key.starts_with('F')
        && key[1..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n))
}

/// 组合键串是否合法：`+` 分隔、无空段、键在末位、修饰键均合法且不重复、
/// 至少一个修饰键（假设 ①：全局快捷键不收裸键）
pub fn valid_combo(combo: &str) -> bool {
    let parts: Vec<&str> = combo.split('+').collect();
    if parts.len() < 2 || parts.iter().any(|part| part.is_empty()) {
        return false;
    }
    let (modifiers, key) = parts.split_at(parts.len() - 1);
    let mut seen = std::collections::BTreeSet::new();
    modifiers
        .iter()
        .all(|m| MODIFIERS.contains(m) && seen.insert(*m))
        && valid_key(key[0])
}

/// 组合键 → 动作 id（查默认表）
pub fn resolve(combo: &str) -> Option<&'static str> {
    DEFAULT_SHORTCUTS
        .iter()
        .find(|spec| spec.combo == combo)
        .map(|spec| spec.action)
}

/// 动作 id → 组合键（查默认表；右键菜单的加速器提示用）
pub fn combo_of(action: &str) -> Option<&'static str> {
    DEFAULT_SHORTCUTS
        .iter()
        .find(|spec| spec.action == action)
        .map(|spec| spec.combo)
}

/// 组合键的用户可读展示：macOS 用 ⌘/⌃/⇧/⌥ 无分隔（`⌘⇧S`），其余平台
/// 用 Ctrl/Win/Shift/Alt 以 `+` 连接（`Ctrl+Shift+S`）；非法组合返回 `None`
pub fn display_label(combo: &str, mac: bool) -> Option<String> {
    if !valid_combo(combo) {
        return None;
    }
    let parts: Vec<&str> = combo.split('+').collect();
    let (modifiers, key) = parts.split_at(parts.len() - 1);
    let mut label = String::new();
    for modifier in modifiers {
        let (glyph_mac, glyph_other) = match *modifier {
            // CmdOrCtrl 是跨平台别名：mac 上是 ⌘，其它平台映射为 Ctrl
            "CmdOrCtrl" | "CommandOrControl" => ("⌘", "Ctrl"),
            "Command" | "Meta" | "Super" => ("⌘", "Win"),
            "Ctrl" | "Control" => ("⌃", "Ctrl"),
            "Shift" => ("⇧", "Shift"),
            _ => ("⌥", "Alt"),
        };
        let glyph = if mac { glyph_mac } else { glyph_other };
        label.push_str(glyph);
        if !mac {
            label.push('+');
        }
    }
    label.push_str(key[0]);
    Some(label)
}

/// 右键菜单项：动作 id + 文案 + 加速器提示 + 可用性（数据全部框架层定，
/// 壳层只渲染；`hint` 为 `None` 表示无对应快捷键）
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ContextMenuItem {
    /// 动作 id（壳层 `ACTIONS` 表的键）
    pub id: &'static str,
    /// 菜单文案
    pub label: &'static str,
    /// 加速器提示（如 `Ctrl+R`/`⌘R`），无快捷键为 `None`
    pub hint: Option<String>,
    /// 可用性（不可用时壳层置灰，仍展示文案）
    pub enabled: bool,
}

/// 右键菜单模型：按界面状态（是否有选中行情/是否有观察标的）计算可用性，
/// 按平台计算加速器提示展示
pub fn context_menu_for(has_quote: bool, has_symbols: bool, mac: bool) -> Vec<ContextMenuItem> {
    let hint = |action: &str| combo_of(action).and_then(|combo| display_label(combo, mac));
    vec![
        ContextMenuItem {
            id: ACTION_REFRESH,
            label: "刷新行情",
            hint: hint(ACTION_REFRESH),
            enabled: true,
        },
        ContextMenuItem {
            id: ACTION_COPY_PRICE,
            label: "复制最新价",
            hint: None,
            enabled: has_quote,
        },
        ContextMenuItem {
            id: ACTION_EXPORT,
            label: "导出首个标的…",
            hint: hint(ACTION_EXPORT),
            enabled: has_symbols,
        },
        ContextMenuItem {
            id: ACTION_SYNC,
            label: "联网增量同步",
            hint: hint(ACTION_SYNC),
            enabled: true,
        },
        ContextMenuItem {
            id: ACTION_READ_OFFLINE,
            label: "读取离线行情（缓存优先）",
            hint: hint(ACTION_READ_OFFLINE),
            enabled: true,
        },
    ]
}

/// 右键菜单（运行时入口）：平台按编译目标自判；测试用 [`context_menu_for`]
/// 显式传平台
pub fn context_menu(has_quote: bool, has_symbols: bool) -> Vec<ContextMenuItem> {
    context_menu_for(has_quote, has_symbols, cfg!(target_os = "macos"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 默认表自洽：组合键全合法、动作 id 非空、组合键与动作均不重复
    #[test]
    fn default_shortcuts_are_unique_and_valid() {
        let mut combos = std::collections::BTreeSet::new();
        let mut actions = std::collections::BTreeSet::new();
        for spec in DEFAULT_SHORTCUTS {
            assert!(valid_combo(spec.combo), "组合键应合法: {}", spec.combo);
            assert!(!spec.action.is_empty(), "动作 id 非空");
            assert!(combos.insert(spec.combo), "组合键重复: {}", spec.combo);
            assert!(actions.insert(spec.action), "动作重复: {}", spec.action);
        }
        assert!(!DEFAULT_SHORTCUTS.is_empty());
    }

    #[test]
    fn valid_combo_accepts_standard_forms() {
        for combo in [
            "CmdOrCtrl+R",
            "Ctrl+Shift+S",
            "Meta+D",
            "CommandOrControl+E",
            "Alt+F4",
            "Shift+F12",
        ] {
            assert!(valid_combo(combo), "应合法: {combo}");
        }
    }

    #[test]
    fn valid_combo_rejects_malformed() {
        for combo in [
            "",            // 空
            "R",           // 无修饰键（假设 ①：不收裸键）
            "Ctrl+",       // 空键
            "+R",          // 空修饰键
            "Ctrl++R",     // 空段
            "Ctrl+Ctrl+R", // 修饰键重复
            "Ctrl+RR",     // 多字符非功能键
            "Ctrl+F13",    // F 键越界
            "Ctrl+工具",   // 非 ASCII 键
        ] {
            assert!(!valid_combo(combo), "应非法: {combo}");
        }
    }

    /// 表查询双向一致：resolve ↔ combo_of 互为逆映射
    #[test]
    fn resolve_and_combo_of_roundtrip() {
        for spec in DEFAULT_SHORTCUTS {
            assert_eq!(resolve(spec.combo), Some(spec.action));
            assert_eq!(combo_of(spec.action), Some(spec.combo));
        }
        assert_eq!(resolve("Ctrl+F9"), None);
        assert_eq!(combo_of("no_such_action"), None);
    }

    #[test]
    fn display_label_mac_uses_glyphs() {
        assert_eq!(
            display_label("CmdOrCtrl+Shift+S", true).as_deref(),
            Some("⌘⇧S")
        );
        assert_eq!(display_label("Ctrl+R", true).as_deref(), Some("⌃R"));
        assert_eq!(display_label("Alt+F4", true).as_deref(), Some("⌥F4"));
    }

    #[test]
    fn display_label_non_mac_uses_plus_joined() {
        assert_eq!(
            display_label("CmdOrCtrl+Shift+S", false).as_deref(),
            Some("Ctrl+Shift+S")
        );
        assert_eq!(
            display_label("CmdOrCtrl+R", false).as_deref(),
            Some("Ctrl+R")
        );
    }

    #[test]
    fn display_label_rejects_invalid_combo() {
        assert_eq!(display_label("R", true), None);
        assert_eq!(display_label("Ctrl+", false), None);
    }

    /// 菜单项自洽：id 唯一、文案非空、有快捷键的项提示与默认表一致
    #[test]
    fn context_menu_items_are_consistent() {
        let menu = context_menu_for(true, true, false);
        assert!(menu.len() >= 5, "菜单应覆盖全部动作");
        let mut ids = std::collections::BTreeSet::new();
        for item in &menu {
            assert!(!item.id.is_empty() && !item.label.is_empty());
            assert!(ids.insert(item.id), "菜单 id 重复: {}", item.id);
            if let Some(hint) = &item.hint {
                let combo = combo_of(item.id).expect("有提示的项应在默认表里");
                assert_eq!(hint, &display_label(combo, false).expect("组合键应合法"));
            }
        }
    }

    /// 可用性判定：无选中行情 → 复制置灰；无观察标的 → 导出置灰；
    /// 刷新/同步/离线读恒可用
    #[test]
    fn context_menu_disable_rules() {
        let none = context_menu_for(false, false, false);
        let enabled = |menu: &[ContextMenuItem], id: &str| {
            menu.iter().find(|i| i.id == id).expect("项存在").enabled
        };
        assert!(!enabled(&none, ACTION_COPY_PRICE), "无行情时复制应置灰");
        assert!(!enabled(&none, ACTION_EXPORT), "无标的时导出应置灰");
        assert!(enabled(&none, ACTION_REFRESH));
        assert!(enabled(&none, ACTION_SYNC));
        assert!(enabled(&none, ACTION_READ_OFFLINE));
        let full = context_menu_for(true, true, false);
        assert!(enabled(&full, ACTION_COPY_PRICE));
        assert!(enabled(&full, ACTION_EXPORT));
    }

    /// 平台差异：同一组合键在 macOS 与其它平台的提示不同（⌘R vs Ctrl+R）
    #[test]
    fn context_menu_hints_differ_by_platform() {
        let mac = context_menu_for(true, true, true);
        let other = context_menu_for(true, true, false);
        for (m, o) in mac.iter().zip(other.iter()) {
            assert_eq!(m.id, o.id, "两平台菜单项应一一对应");
            if m.hint.is_some() {
                assert_ne!(m.hint, o.hint, "有快捷键的项提示应随平台变化: {}", m.id);
            }
        }
    }

    /// serde 字段契约：ContextMenuItem 序列化为 {id,label,hint,enabled}，
    /// 壳层按这些键渲染（缺键/改名都会在这里红）
    #[test]
    fn context_menu_item_serde_field_contract() {
        let json = serde_json::to_value(&context_menu_for(true, true, false)[0]).expect("序列化");
        for key in ["id", "label", "hint", "enabled"] {
            assert!(json.get(key).is_some(), "序列化应含字段 {key}: {json}");
        }
        assert_eq!(json["id"], serde_json::json!(ACTION_REFRESH));
        assert_eq!(json["enabled"], serde_json::json!(true));
    }
}
