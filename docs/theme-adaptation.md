# 深色模式与系统主题适配

全平台一览：主题的「语义」三端同构（三态偏好 system/light/dark → 生效
主题，system 跟随系统深色检测），「实现」各平台落在自己的 UI 层。

| 平台 | 实现位 | 偏好持久化 | 系统跟随 | 状态 |
|---|---|---|---|---|
| Web | `web/app/src/lib/theme.ts` + ThemeToggle（L432） | localStorage `alpha.theme` | `matchMedia('prefers-color-scheme: dark')` + change 监听 | ✅ L432 |
| Desktop | 兜底壳 `web/dist/desktop-shell.js` 的 `applyTheme`（窗口装 web/dist 原生页，非 React 工程）；`tauri.conf.json` 不配 `theme` 字段——v1 schema 只收 `light/dark` 无 system 档，留空即跟随 OS | Rust `AppConfig.theme`（`config.rs:29`，默认 `system`，非法值校验拦下），随 `initialize_app` payload 下发——零 localStorage | `matchMedia('prefers-color-scheme: dark')` + change 监听（`desktop-shell.js:97-116`） | ✅ L115 |
| Android | `mobile/android/.../Theme.kt`（本项）：`AlphaTheme` 切 Material3 light/dark | `ThemeSettingsStore`（KeyValueStore 单键，损坏回 SYSTEM） | `isSystemInDarkTheme()` | ✅ L511 |
| iOS | 目录属 L119 交付面不动；对应物登记：`.preferredColorScheme` + `@Environment(\.colorScheme)`，偏好入 App Storage | 留档待 iOS 解封 | `UITraitCollection.userInterfaceStyle` | 登记 |

## 语义契约（三端一致，各自单测锁定）

1. 解析收口：任意持久化值 → 合法三态，未知/损坏/缺省一律回 **SYSTEM**
   （跟随系统是统一缺省口径；web `parseThemePref` / Android
   `parseThemePreference` 同语义）；
2. 映射：`system` 档跟随系统深色检测实时生效（web 监听 change 事件，
   Android 依赖 `isSystemInDarkTheme()` 组合刷新）；显式档不受系统影响；
3. Android 偏好变更重启生效（读点在 Activity onCreate——骨架期取简，
   进程内热切换随设置页 TODO 一起接）。

## 与相邻项

- **L432**：web 主题系统是本项的语义源头（Android 对齐其口径）；
- **L512**：Android 偏好存储复用其 KeyValueStore 抽象与
  PrivacySettingsStore 的单键 JSON/单键值模式；
- **深色令牌**：web `styles.css` 的 CSS 变量对与 Android Material3
  默认 light/dark 配色板为骨架期配色，品牌色板归后续设计项（三端
  换色点各自集中：web `:root[data-theme='dark']` / Android
  `darkColorScheme()` 调用处）。
