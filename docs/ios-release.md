# iOS IPA 签名与 TestFlight/App Store 发布

**登记性交付**：`scripts/ios-release.sh`（可执行登记——非 macOS 环境明确
跳过 exit 2，供 CI 矩阵无条件调用）+ 本文档的流程/密钥/审核口径。
真机执行需 Apple Developer 账号与证书（全部走 CI secret/本地钥匙串，
**本仓不持有任何密钥**）；`mobile/ios/` 属 L119 交付面，本仓不动——
Xcode 工程与 Info.plist 的落地点在脚本守卫中显式报缺。

## 1. 前置资产（实现时到位，登记清单）

| 资产 | 形态 | 保管位置 |
|---|---|---|
| Apple Developer Program 账号 | 团队 + `TEAM_ID` | App Store Connect |
| Apple Distribution 证书 | .cer + 私钥 | CI macOS 作业钥匙串（secret 导入） |
| App Store Provisioning Profile | .mobileprovision（App Store 类型，绑定证书与 App ID） | 同上；`signingStyle: automatic` 时由 Xcode 托管 |
| App Store Connect API 密钥 | KeyID + IssuerID + .p8 | CI secret（`~/.appstoreconnect/private_keys/`） |
| Bundle ID | `com.alpha.ios`（脚本 `APP_SPEC` 可覆盖） | 开发者后台登记，启用 Push（随 L337/§L520 planned） |

## 2. 构建链路（脚本四阶段）

1. **bindings**：`mobile/ios/gen-bindings.sh`——UniFFI 产 Swift 绑定进 `gen/`
   （运行时生成，暂未入库；L301 约定产物可审）；aarch64-apple-ios 交叉编译
   `libalpha_mobile`（L118 FFI 面）在脚本内为注释占位，归 CI macOS 作业
   （L470）；
2. **archive**：`xcodebuild archive`（Release、`generic/platform=iOS`）；
   工程缺失时守卫报「工程创建归 L470」；
3. **export**：`exportArchive` + ExportOptions（`method: app-store-connect`、
   `signingStyle: automatic`、`teamID` 必填校验）→ 签名 IPA；
4. **upload**：`xcrun altool --upload-app --apiKey/--apiIssuer` 上
   TestFlight。altool 已被 Apple 标记弃用路径，替换窗口登记：
   Transporter CLI / App Store Connect API（`/v1/builds` + 二进制上传
   预签名 URL），脚本只换 `run_upload` 一处。

## 3. TestFlight → App Store 节奏

- **内部测试**：≤100 人即时可用（同团队 App Store Connect 用户）；
- **外部测试**：≤10000 人，首次需 Beta App Review（1–2 天）；
- **正式发布**：提审前完成 §4 审核清单；用 Phased Release（7 天分批）
  控制发布风险；崩溃率/回退依托 App Store Connect 数据面。

## 4. App Store 审核清单（提审门禁）

1. **隐私政策 URL 必填**——指向 L520 `docs/platform-compliance.md` §1
   草案的托管页（发布站点归 L515 CDN）；
2. **Privacy 清单**（PrivacyInfo.xcprivacy）：声明收集类别——本应用
   「不收集」（无账号/无遥测，与 L520 §1 口径一致）；Required Reason
   API（UserDefaults 等）按苹果类别登记；
3. **权限用途字符串**：每个 Info.plist 权限键配 purpose string——
   当前规划仅 `NSFaceIDUsageDescription`（L512 落地时随
   `mobile/ios` 交付面进入，文案与 L520 §2.2 一致）；推送授权弹窗
   文案随 L337；
4. **分类/年龄**：Finance 类别；无用户生成内容、无赌博属性（行情
   展示与工具，不含交易下单）；
5. **出口合规**：仅用系统 HTTPS/标准加密 → ITSAppUsesNonExemptEncryption
   = false（标准豁免）。

## 5. 与相邻项分工

- **L470 发布流水线**：把脚本接进 CI macOS 作业（secret 注入 + STAGE=all）；
- **L471 应用商店集成**：App Store Connect 侧的版本/截图/元数据管理面；
- **L517 Android 分包**：对称的 Android 侧发布项；
- **L519 更新机制**：iOS 无自更新（App Store 强制），更新节奏完全
  随商店审核——已在 docs/auto-update.md §2 登记为「明确不做」。

## 6. 非交互假设

1. Bundle ID `com.alpha.ios`、Team/证书等资产以占位登记，实现时替换；
2. `signingStyle: automatic`（Xcode 托管签名）——手动 profile 管理仅
   在企业分发场景需要，不在本仓范围；
3. 上传通道先 altool、弃用窗口切换点登记在 §2.4，不预实现新 API。
