# 平台合规性：隐私政策与权限申请

本仓的合规面分两层：**可门禁的权限注册表**（机器对账，本单落地）与
**隐私政策文本**（工程草案，见 §1）。检查器：`scripts/check-compliance.sh`，
基线：`config/compliance/permissions-registry.txt`（`[allow]` 为检查白名单、
`[planned]` 留档计划权限；新增 allow 条目 = conscious ack，与
`packages/core` safety_audit 的 unsafe 预算同一纪律）。CI 接线：`check-compliance.sh` 已进 `.github/workflows/ci.yml` 的
`compliance` 作业（checkout 后直跑，非阻塞面之外的常规门禁——权限与 CSP
回归即红）。

## 1. 隐私政策（应用内展示文本 · 工程草案）

> 发布前须法务复核；此处为工程实现口径的政策草案，随构建打包进
> 「关于」页与首次启动引导。

1. **数据最小化**：本应用不要求注册、不收集手机号/邮箱等个人身份信息；
   自选列表、图表配置等用户数据仅存本地（Android SharedPreferences /
   桌面 Tauri fs 用户目录 / Web localStorage），当前不上传服务器——
   账户同步服务端面已就绪（L476），客户端接线时本条随
   `docs/data-privacy.md` §1 数据清单重写。
2. **市场数据**：行情、K 线、财务数据均为公开市场信息的缓存展示，
   数据源为公开接口与授权供应商，不含任何用户生成内容。
3. **无遥测上报**：当前构建不内置任何统计分析/埋点 SDK；崩溃日志仅在
   用户主动反馈时附带（后续若引入崩溃收集，须在此节更新并随版本公告）。
4. **网络访问**：仅用于拉取行情与平台公告（目标：api-gateway），
   不在后台扫描其他应用、不读取通讯录/位置/存储中与功能无关的数据。
5. **权限用途**：逐条见 §2 注册表——每个权限都对应一个可关停的功能，
   拒绝授权只降级对应功能，不锁死应用。
6. **数据删除**：清除应用数据/卸载即完全删除本地数据；账户同步面（L476）
   存的服务端数据可经 `DELETE /api/v1/account` 整账户删除（幂等 204 +
   `AccountDataDeleted` 审计留痕，详见 account-sync §2.5）。

## 2. 权限台账（注册表的人读版）

### 2.1 Android（`mobile/android/app/src/main/AndroidManifest.xml`）

现状：`USE_BIOMETRIC` 已入注册表 `[allow]`（L512 落地，BiometricGate.kt +
10 个 JVM 测试），android 段对账基数 1 = 清单零权限之外仅生物识别一项；
`INTERNET`/`POST_NOTIFICATIONS` 仍留档 `[planned]`，触发时按
`[planned]` → `[allow]` 登记抬基线：

| 权限 | 类型 | 申请语义 | 状态/触发条件 |
|---|---|---|---|
| `USE_BIOMETRIC` | normal（安装期授予） | 启动/敏感操作认证 | ✅ 已入清单（L512 生物识别落地） |
| `INTERNET` | normal（安装期授予） | 拉取行情 | planned：api-gateway 接入 |
| `POST_NOTIFICATIONS` | **runtime（API 33+ 运行时申请）** | 推送/告警通知 | planned：L337 推送落地；拒绝 → 降级站内告警 |

### 2.2 iOS（**仅登记边界——`mobile/ios/` 属 L119 交付面，本仓不动**）

落地时随 Info.plist 登记：push entitlement（APNs）、
`NSFaceIDUsageDescription`（FaceID 用途声明，缺失会被 App Store 审核拒）、
推送通知权限弹窗语义同 Android（用户拒绝即静默降级）。

### 2.3 桌面（`desktop/tauri.conf.json` allowlist）

Tauri v1 以 allowlist 组为权限粒度，当前启用 6 组（检查器双向对账）：

| 组 | 理由 | 最小权限评审 |
|---|---|---|
| `shell` | 仅 `shell.open` 打开外部链接 | 不放开 `execute`（任意命令执行） |
| `dialog` | 导入/导出本地数据文件 | open/save 显式用户操作 |
| `fs` | 工作区数据读写 | 限 dialog 选定路径；后续可评估 scope 收窄 |
| `path` | 路径解析（fs/dialog 配套） | 只读 |
| `notification` | 行情告警本地通知 | OS 首次发送时弹系统授权 |
| `globalShortcut` | 快捷键唤起窗口 | 无系统级敏感面 |

组粒度收窄（fs scope、notification 事件面）归 L516 打包项复核；
macOS entitlements 现为 null（未申请沙盒外能力），签名/公证归 L518。

### 2.4 Web

纯静态站点无安装时权限；CSP 在桌面壳内由 `security.csp` 承载
（检查器校验非空）。PWA 通知（L506）落地时：`Notification.requestPermission()`
必须由用户手势触发，拒绝则降级站内告警——与注册表 `web:Notification`
planned 条目一致。

## 3. 检查器语义（scripts/check-compliance.sh）

1. 注册表 `[allow]` 每条理由非空（无理由的权限不许进白名单）；
2. Android 清单 `<uses-permission>` ⊆ 注册表 android 段；
3. Tauri allowlist 启用组 ↔ 注册表 tauri 段**双向对账**（未登记的启用组
   失败、已登记但未启用的陈旧条目失败——注册表不许腐烂）；
4. CSP 非空。

新增权限流程：先在 `[planned]` 确认触发条件 → 实现时移入 `[allow]` 带
理由 → 同步本文档 §1/§2 影响面 → 检查器过门禁。

## 4. 非交互假设

1. 隐私政策文本为工程草案口径（无账号/无遥测/本地优先），法务复核
   归发布前置项，不阻塞工程验收；
2. Tauri allowlist 六组维持现状（收窄是优化不是缺陷，登记进 L516）；
3. Android 保持零权限清单——INTERNET 提前加入无收益（骨架期无网络
   调用），按 planned 触发条件走。
