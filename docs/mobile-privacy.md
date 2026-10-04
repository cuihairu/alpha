# 移动端生物识别门与隐私保护

三件套：**生物识别门**（`BiometricGate.kt`：Keystore auth-per-use 门密钥
+ BiometricPrompt CryptoObject 流 + 纯逻辑状态机）、**静态加密存储**
（`EncryptedKeyValueStore`，AES-256-GCM 每写新 IV）、**防泄漏基线**
（FLAG_SECURE + 隐私设置持久化）。Android 侧落地；iOS 侧边界登记见 §6。

## 1. 两层分离（职责红线）

| 层 | 保护对象 | 机制 | 密钥 |
|---|---|---|---|
| 门（gate） | 进入应用 UI 的「人」 | BiometricPrompt 强认证 + CryptoObject | `alpha_biometric_gate`：auth-per-use，**每次使用都要生物识别**，新录入生物凭据即作废 |
| 静态加密（at-rest） | 数据本身 | 离线快照值 AES-GCM 加密落盘 | `alpha_offline_data`：keystore AES-256，**不绑认证**（逐条解密无法每次弹窗） |

分开的理由：门证明「解锁那一刻是本人在场」；加密保证「门失效/设备直接
读盘时数据仍是密文」。auth-per-use 密钥不能当数据密钥用（每次 doFinal
都要弹认证），非认证密钥不能证明「人在场」——互为补充，不混用。

解锁事件的可验证性：认证成功回调里对哨兵明文 `alpha-gate-proof-v1`
做一次 doFinal，密文作为本次会话的门禁凭证（伪造 UI 状态拿不到密钥
操作——密钥操作被 TEE 限制在认证边界内）。

## 2. 状态机与默认值（`GateStateMachine` / `PrivacySettings`）

迁移规则全部显式、JVM 单测锁定（`BiometricGateTest`）：

- 初始会话：`biometricEnabled && lockOnBackground` → 冷启动即锁；
- 退后台：`lockOnBackground`（默认开）→ 回锁并清失败计数；
- 认证成功 → 解锁清计数；失败留在锁态、失败计数累计（UI 提示面；
  节流由系统凭据层承担，不自建永久锁死）；
- 关闭 `biometricEnabled` → 立即解锁（门的存在依赖开关，opt-out 即放行）；
- 开启「退后台即锁」不突袭锁定当前会话，下次退后台生效。

默认值：`biometricEnabled=false`（**opt-in**，不抢首次启动体验）、
`lockOnBackground=true`、`screenshotShield=true`（金融应用最小暴露面）。
设置损坏时 fail-safe 回默认（锁屏开）。

## 3. 能力降级（不把用户锁在门外）

门激活条件 = 设置开启 **且** `BiometricManager.canAuthenticate(BIOMETRIC_WEAK)`
为 SUCCESS。无硬件/未录入时门不激活、直接进应用——此时数据安全由静态
加密层兜底。`GateAvailability` 三态（Available/NoHardware/NoneEnrolled）
接口化（`BiometricCapabilities`），设备实现与 JVM 假件共用。

## 4. 权限与合规台账

`USE_BIOMETRIC`（normal，安装期授予，无运行时弹窗）已从 L520 注册表
`[planned]` 移入 `[allow]`（`scripts/check-compliance.sh` 实测对账通过：
android=1）。清单仍无 INTERNET（真实数据接入时另批）。

## 5. 可测性切面

JVM 单测（无需设备/.so）：设置持久化往返与损坏回退、状态机全部迁移
规则、加密存储语义（往返/密文不泄明文/同明文两次写入密文不同（随机
IV）/位翻转篡改→null/换钥→null/delete·keys 透传）。Keystore 与
BiometricPrompt 只在设备路径实例化（`MobileKeys` / `promptBiometricGate`），
单测不触 Android 框架类。

实测：`:app:testPlayDebugUnitTest` 40 用例全绿（L512 时点，含本项 10 个；
现测试目录 7 文件共 54 例）。

## 6. iOS 边界（登记，不动）

`mobile/ios/` 属 L119 交付面，本项不改。iOS 对应物登记为：FaceID 门
（LocalAuthentication + Info.plist `NSFaceIDUsageDescription`——L520 注册表
`[planned]` 留档待 iOS 目录解封时随 Info.plist 落地）、钥匙串 kSecAttr
accessControl 绑生物识别（对应门密钥）、`UIApplication.shared.isIdleTimerDisabled`
等价物不涉及。窗口遮挡（防截屏等价物）iOS 无公开 API，登记为平台差异。

## 7. 与相邻项

- **L337 推送/告警**：门锁定期间通知仍可见（系统层），应用内告警面板
  随门隐藏；
- **L390 离线存储**：`OfflineStore` 的 SharedPreferences 实现接入生产时，
  以 `EncryptedKeyValueStore(SharedPreferencesKeyValueStore(ctx),
  MobileKeys.dataKey())` 包装落盘（本项提供封装与密钥位，接线随设置页
  TODO 一起接，见 docs/mobile-offline.md §8⑦）；
- **L520 权限注册表**：本项完成了 USE_BIOMETRIC 的 planned→allow 迁移；
- **L512 后续（桌面/iOS）**：桌面 Tauri 无生物识别门（OS 锁屏承担），
  登记 docs/platform-compliance.md 的差异说明归 L520 复核。
