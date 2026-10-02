# 移动端离线数据存储与同步机制

口径：**设计要点先行 + 最小可编译闭环 + 单测**（沿 L118/L301/L337/L367 既定口径）——
离线数据的**决策面**（允许存什么/何时同步/增量基线）在 Rust 核心库，**执行面**
（kv 落盘/恢复）在平台壳；融入既有架构模式（`RefreshGateway` 注入面、JSON 桥、
`Mutex` 决策状态、FFI 只增不改）。

上游依托：docs/mobile-core-architecture.md（FFI 面/观察列表边界）、
docs/mobile-push-sync.md（L337 指纹口径 `content_seq`/`fingerprint_of`）、
桌面 L116（kv 快照 + 指纹增量的同源口径）、alpha-core `platform::KeyValueStore`
（后写覆盖语义的参考实现）。

## 1. 术语（红线：同步与备份严格区分，不得混用）

| 术语 | 定义 | 数据去向 | 本轮落地 |
|---|---|---|---|
| **备份（backup / snapshot）** | 本地快照**落盘**：把授权范围内的行情快照写入壳层本地 kv，供断网/重启后恢复 | **不出设备**（纯本地持久化） | ✅ 本轮闭环（`offline_snapshot_json` / `restore_offline_snapshot_json` / `OfflineStore`） |
| **同步（sync / delta）** | 与**远端**做增量对齐：以指纹比较决定是否需要对齐、对齐哪些数据 | **出设备**（经网络到远端） | 决策与增量判断闭环本轮落地；真实网络执行归远端接入 TODO（§8④） |

纪律落点：载荷与 API 命名一一对应——备份面用 `Snapshot`（`captured_at`、
`restore`），同步面用 `SyncDelta`（`since_fingerprint`、`needed`）；注释、UI 文案
与本文档一律不互换使用两词。

## 2. 产品红线（任何数据外发/同步类功能）

1. **默认关闭**：`OfflineSyncConfig::default()` → `enabled = false`；`MobileCore`
   构造即持默认配置（构造器签名不变——L119 iOS 壳红线）。未开启时快照生成直接
   拒绝（`Failed`，文案含「离线同步未开启」）——红线在**核心库强制**，壳层绕不过
   （与「观察列表是核心库唯一业务判断」同一纪律）。
2. **须显式配置开启**：唯一开启路径 = FFI `set_offline_sync_config_json` 显式传
   `enabled=true`；且 **`enabled=true` 必须携带非空 `scopes`**（无范围开启 = 拒绝，
   `Failed`）——「开启」与「明示范围」在同一请求里强制绑定。
3. **开启时明示数据范围**：配置载荷回显 `scopes`（授权的数据类别）；壳层
   `dataScopeSummary()` 把范围翻成明示文案（「开启后将把以下数据落盘/用于同步：
   行情快照（含代码、价格、成交量、买卖一档）」），供设置 UI 展示（设置页骨架归
   后续 TODO，配置与文案面本轮就绪）。

红线对备份同样生效：备份虽不出设备，但属用户数据持久化——与同步共用同一授权
开关（一条红线管两个面，杜绝「备份偷偷落盘」）。

## 3. 数据范围（DataScope，首期单变体）

```
Quotes  行情快照（代码/价格/成交量/买卖一档/open；serde 即 MarketData 字段）
        —— Analysis / Watchlist 等变体留扩展位，随能力逐个评审后加入
```

未知 scope 字符串经 serde 拒绝（`Failed`）——范围枚举是封闭集合，不允许壳层
自造。

## 4. 分层与 FFI 面（只增不改，L118 构造器 + L118 三方法 + L337 六方法原样）

```
┌─ 平台壳（android / ios，存储与网络执行）───────────────────────┐
│  OfflineStore：KeyValueStore（SharedPreferences 实现位/InMemory  │
│  测试）落盘、恢复；OfflineSyncManager：备份/同步编排；UI 明示文案  │
└───────────────▲───────────────────────────────────────────────┘
                │ FFI 五方法（新增）
┌─ alpha-mobile（决策，offline.rs + state.rs 接线）──────────────┐
│  OfflineSyncConfig（enabled 默认 false + scopes）               │
│  OfflineManager：快照生成（scope 过滤 + L337 指纹）/ 增量判断    │
└───────────────────────────────────────────────────────────────┘
```

| FFI 方法 | 入参 | 出载荷/语义 |
|---|---|---|
| `offline_sync_config_json` | — | 当前 `{enabled, scopes}`（UI 读取开关与范围） |
| `set_offline_sync_config_json` | `{enabled, scopes}` | 生效配置回显；`enabled=true` 且 `scopes` 空 → `Failed`；未知 scope → `Failed` |
| `offline_snapshot_json` | — | `OfflineSnapshot`；未开启 → `Failed`（红线①） |
| `restore_offline_snapshot_json` | 快照 JSON | 校验结构与版本，返回恢复条目数；结构非法 → `Failed` |
| `offline_sync_delta_json` | `since_fingerprint: u64` | `SyncDelta`；未开启 → `needed=false, reason="sync_disabled"`（红线③：未授权不产生外发工作项） |

载荷契约（单测逐字段点名）：

- `OfflineSyncConfig`：`{enabled: bool, scopes: [snake_case 字符串]}`
- `OfflineSnapshot`：`{version, enabled, scopes, captured_at, entries, fingerprint}`，
  条目 `{key, payload_json, content_seq}`（key 形如 `quote:600519`；`payload_json`
  为 `MarketData` serde 全文；`content_seq` 沿 L337 FNV-1a 口径；`fingerprint` 为
  symbol 排序聚合指纹——与 L337 `fingerprint_of` 同源，跨面可比）
- `SyncDelta`：`{needed, since_fingerprint, current_fingerprint, reason?}`——
  指纹相同 → `needed=false, reason="unchanged"`；不同 → `needed=true`（对齐哪些
  条目由壳层经 `offline_snapshot_json` 取全量快照比对，首期不做条目级 diff）

## 5. 壳层执行面（Kotlin，融入 AlphaBridge/Gestures 既有架构）

- **`KeyValueStore` 接口 + 双实现**：`InMemoryKeyValueStore`（JVM 单测）+
  `SharedPreferencesKeyValueStore`（真机实现位，骨架不实例化——Context 依赖归
  设置页 TODO）；语义对齐 alpha-core `platform::KeyValueStore`：后写覆盖、删除幂等。
- **`OfflineStore`**：`saveSnapshot`（`enabled=false` 的载荷拒收——红线在壳层
  第二道防线）/`loadSnapshot`/`clear`；快照整体序列化为单 kv 条目（沿桌面 L116
  「kv 快照」口径，不逐条目建键）。
- **`OfflineGateway` 接口**（沿 L367 `RefreshGateway` 模式）：`offlineSnapshot()` /
  `offlineSyncDelta(since)`——`AlphaBridge` 实现（五透传中的两个 + 配置读写），
  JVM 单测注入 fake。
- **`OfflineSyncManager`**：`backupNow()`（快照→落盘）与 `planSync()`（增量判断，
  不执行网络）；异常不捕获，冒泡给 UI 按既有 `AlphaBridge.Error` 口径降级（§6）。
- **`dataScopeSummary(scopes)`**：红线③的明示文案生成（纯函数，JVM 可测）。
- 与 **Gestures 的融合点**：下拉刷新编排（`refreshWithManualSync`）完成后可顺手
  `backupNow()`——本轮**不自动接**（备份触发时机应属用户显式配置/设置页动作，
  自动落盘有违「显式开启」精神），接缝留 `OfflineSyncManager` 备用。

## 6. 线程与错误

- 决策状态（配置）挂 `Mutex<OfflineManager>` 于 `MobileCore`（既有模式）；FFI
  一律经 `AlphaBridge` 切 `Dispatchers.IO`（既有透传纪律）。
- 错误沿用 `MobileError`：未开启/范围空/未知 scope/版本不匹配/结构非法 →
  `Failed{detail}`（Display 全文）；壳层 UI 按既有 catch 文案降级，不加新错误面。

## 7. 测试策略

| 层 | 形式 | 覆盖 | 跑在哪 |
|---|---|---|---|
| Rust 单测 | `offline.rs` 11 例 | 默认关闭/开启须范围/未知 scope/未开启拒快照/scope 过滤/指纹与 L337 同源/快照往返/delta 三分支/restore 校验 | workspace 门禁（CI ✅） |
| Rust 单测 | `state.rs` +3 例 | FFI 五方法接线：默认读回 false、开启→快照→恢复计数（含无范围/未知 scope/版本篡改拒绝）、delta 禁用与指纹比较分支 | workspace 门禁（CI ✅） |
| JVM 单测 | `OfflineSyncTest.kt` 7 例 | kv 后写覆盖/快照落盘恢复往返/未授权载荷拒收/编排基线（落盘指纹→同步起点、无基线 0 起步）/明示文案/术语字段契约 | 本机 `gradlew testDebugUnitTest`（CI 无 SDK 靠契约守门） |
| 契约守门 | `android_shell_contract.rs` +1 例 | OfflineStore.kt 在场与纪律（红线拒收/明示文案/术语定义在场/不 import uniffi/MainActivity 不自动备份）、AlphaBridge 实现 OfflineGateway、绑定五方法在场、offline.rs 红线强制在场 | workspace 门禁（CI ✅） |
| 真机 | SharedPreferences 持久化/设置 UI | §9 边界 | 不覆盖 |

## 8. 非交互假设（自行判定，已注明）

1. 本轮台账记 L390（沿 TODO 行号；行号漂移不作编号依据）。
2. `enabled` 开关统一管「备份落盘 + 同步」两面（红线对备份同样生效，§2 尾）。
3. 首期 `DataScope` 仅 `Quotes`；`Analysis`/`Watchlist` 变体随能力评审后加入
   （封闭枚举，未知值拒绝）。
4. 同步的「真实网络执行」仍无远端（api_url 配置槽，沿 L337 口径）——本轮落
   决策 + 增量判断契约；`SyncDelta.needed=true` 后壳层的对齐动作归远端接入 TODO。
5. 快照 `version` 校验：restore 时快照版本须与当前 crate 版本一致（不一致拒绝）；
   跨版本迁移策略归发布流水线 TODO。
6. 快照整体单键落盘（`alpha_offline_snapshot`），不逐条目建键——骨架期快照小；
   分键/增量落盘待条目级 diff（§4 尾）一起做。
7. 壳层无设置 UI（`enabled` 只能经 FFI 置位）——设置页与明示文案展示位归
   后续 TODO，本轮交付配置面 + `dataScopeSummary` 文案函数。
8. FFI 入参 `since_fingerprint: u64` 用原生 u64（uniffi → Kotlin `ULong`，沿
   `set_alert_rules_json` 返回 `u64` 的既有先例）。
9. `mobile/ios/` 不动（L119 会话交付面）；iOS 按同一五方法模式接
   `UserDefaults`/文件，接缝即 FFI 面。

## 9. 真机验收边界（本环境不可观察，登记）

1. SharedPreferences 实际持久化（进程重启/卸载重装后的快照留存）。
2. 设置页的开关交互与明示文案实际展示（红线③的最终呈现形态）。
3. 断网场景的备份恢复全链路（含快照损坏时的降级读）。
4. 真实远端接入后的同步语义（`SyncDelta.needed` → 实际对齐请求）。
5. 快照体积增长后的分键与条目级 diff 策略。

## 10. 后续 TODO 映射

| TODO 项 | 本文依托 |
|---|---|
| 远端接入（api-gateway 服务端面） | §4 `SyncDelta` 契约；§8④ 壳层对齐动作 |
| 设置页 / 配置下行 | §5 `dataScopeSummary` 文案面 + FFI 配置读写 |
| 智能告警与个性化推送（L418） | §3 scope 扩展位（Analysis 授权后可参与快照/同步） |
