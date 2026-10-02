# 应用商店发布（TODO L471）

打包产产物（L516–L518）→ 本项负责「上架」：元数据 + 提交流程 + 分阶段发布。
密钥/签名资产仍走各打包文档的 CI secret 口径，本仓零密钥。

## 1. 产物对接表（L470 流水线产出 → 商店入口）

| 商店 | 喂入产物 | 产出脚本 |
|---|---|---|
| Google Play | `app-play-release.aab`（Play 动态分包） | `scripts/android-release.sh` |
| App Store | archive → `.ipa`（ExportOptions app-store-connect） | `scripts/ios-release.sh`（真机在 L470 macOS 作业） |
| Microsoft Store | 见 §4（PWA 备选 / MSIX 后续） | — |

## 2. 商店元数据（本仓 `store/`，中英双语，L479 字典口径一致）

- `store/play/`：short（≤80 字符）+ full（≤4000 字符）× zh/en；
- `store/appstore/`：subtitle（≤30）+ description（≤4000）+ keywords（≤100，以半角逗号分隔）× zh/en；
- 长度上限已用脚本核过（见本项提交记录），改文案后重跑：
  `python3 -c "…"`（上限表见上）。
- 隐私政策 URL：挂 L520 草案（提审必填，发布前法务复核）；
- 内容分级：金融类、无交易下单、无真钱交易——问卷如实勾选（Finance 分类）；
- 截图清单（提审前在真机/模拟器实拍，不入库占位图）：
  看板（中/英各一）/ 工作区 / K 线 / 深色模式 / 小组件（Android）/ 灵动岛（iOS）。

## 3. 分阶段发布（灰度是商店侧能力，不自建通道）

- Play：staged rollout（先 10% → 50% → 全量，崩溃率阈值熔断，halt 即停）；
  内部测试轨道先行（dogfood），`versionCode` 递增是唯一判据（L472）。
- App Store：phased release（7 天自动放量）+ TestFlight 外测 10000 人；
  紧急修复走加速审核（一年有限次，留刀）。
- 桌面：Tauri updater feed 即分阶段能力（L519；先小群 `notes` 标注 beta，
  稳定后再全量）——桌面无商店审核，节奏由 feed 控制。

## 4. Microsoft Store（边界登记）

桌面端当前打包形态为 nsis/msi（L516），直上 MS Store 需 MSIX：
两条路——① PWA 备选：本应用已具 manifest + 自包含 SW（L507），
PWABuilder 套壳即得 MSIX，无需改代码；② 原生 MSIX：需 Windows 打包机 +
开发者证书，归 L470 Windows 作业后续。本项不产 MSIX，只登记路径与
上架资料（元数据沿用 `store/` 英文面 + 年龄分级 IARC 问卷）。

## 5. 国内 Android 市场（边界登记）

`docs/android-release.md` 已登记「国内市场代收归 L471」——现状：版号、
软著、代收方商务流程均未启动，universal APK 即技术就绪形态；
本项交付三店（Play/App Store/MS）流程与元数据，国内市场提交登记为后续
（需法务/商务先行，非工程可先行项）。

## 6. 与 L517/L518/L520 接缝

- 签名与上传钥：`docs/android-release.md` §3（Play App Signing 上传密钥托管）、
  `docs/ios-release.md`（TEAM_ID/altool，ASC API 替换点）；
- 权限与隐私问卷答案源：`docs/platform-compliance.md` 四平台台账 +
  `docs/data-privacy.md`（数据收集声明「不收集」口径）；
- 版本号：L472（各店展示版本与 versionCode 取发版号）。
