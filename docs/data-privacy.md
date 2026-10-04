# GDPR / CCPA 数据合规

与 `docs/platform-compliance.md`（L520 隐私政策与权限台账）分工：本篇是
**数据主体权利与数据生命周期的工程义务映射**——每项法定义务 → 本仓落地面
→ 诚实边界。口径前提：**本地优先**；账户同步（L476）协议与服务端权威面
已落地，客户端尚未接线同步（web 端 account.ts 纯逻辑未接 UI/网络、Android
无 INTERNET 权限）——接线发布前须重评本表。

## 1. 处理活动概览（数据清单）

| 数据 | 位置 | 性质 | 是否个人数据 |
|---|---|---|---|
| 自选工作区 `alpha.workspaces` | Web/桌面 localStorage | 用户生成（偏好） | 本地个人数据 |
| 主题偏好 `alpha.theme` | Web/桌面 localStorage | 用户生成 | 本地个人数据 |
| 生物识别/小组件状态 | Android SharedPreferences | 用户生成（部分带 `alpha_` 前缀；theme_preference/privacy_settings 无前缀，键面见 BiometricGate.kt/Theme.kt） | 本地个人数据 |
| 账户同步记录（profile/工作区 KV，L476） | api-gateway 内存，配 `ALPHA_GATEWAY_ACCOUNT_STORE_URL` 时 Postgres | 用户生成 | 是（同步上线即服务端个人数据） |
| 行情/K线/指标缓存 | 服务端内存/Timescale/ClickHouse | 公开市场数据 | 否 |
| 访问/审计/护栏日志 | 服务端日志流（Loki）+ 进程文件 | 交互元数据 | 过程数据（见 §4 保留） |
| JWT/密钥、API key | 配置/env 注入 | 凭据 | 否 |

服务端账户权威面（`/api/v1/account/profile`、`/api/v1/account/sync`）按账户
持久化工作区数据：默认内存（重启即失），可配 Postgres 持久化；除该面外，
行情上报只携带查询参数与凭据。

## 2. 权利义务映射

| 义务（GDPR / CCPA） | 落地面 | 状态 |
|---|---|---|
| 访问权 / 可携权（Art.15/20，CCPA 右至副本） | `web/app/src/lib/privacy.ts` `exportUserData`：`alpha.` 键全量 JSON 快照 + PrivacyPanel 一键下载 | ✅ web/桌面 |
| 删除权 / 被遗忘权（Art.17，CCPA 右至删除） | 本地 `clearUserData`：仅删 `alpha.` 应用键（显式清单 ∪ 前缀扫描），非应用存储不动，幂等；服务端 `DELETE /api/v1/account` 整账户清除（account-sync §2.5），幂等 + 审计留痕 | ✅ web/桌面 + 服务端 |
| 撤回同意（Art.7(3)） | 无非必要处理面（无遥测/无第三方 tracker/无 cookie 同意需求）；生物识别门可关闭（L512，默认关，opt-in） | ✅ |
| 数据最小化（Art.5(1)(c)） | 本地即最小化；服务端只收查询参数；隐私政策 §1.1 | ✅ |
| 存储限制（Art.5(1)(e)） | §3 保留期限表；无长期用户维度存储 | ✅ |
| 透明（Art.12/13） | `docs/platform-compliance.md` 隐私政策草案 + 应用内「数据与隐私」面板清单展示 | ✅ |
| 问责（Art.5(2)） | 本映射表 + 门槛化的审计日志（L487 登记） | ✅ |
| 自动化决策（Art.22） | 无自动化决策/画像（告警规则为用户自设，非分析画像） | ✅ N/A |

## 3. 保留期限

| 数据 | 期限 | 依据/处置 |
|---|---|---|
| 本地用户数据 | 保留至用户清除/卸载（面板一键清除已提供） | 数据主体自主控制 |
| 访问日志/指标 | 随日志轮转与 Prometheus 存储配置（滚动覆盖，无归档） | 运维必需，不进归档 |
| Redis Streams/PEL | 消费即 ack；DLQ 按队列语义滚动（L451） | 短暂过程数据 |
| Timescale/ClickHouse 行情 | 配置驱动（persistence 三态；public 数据） | 业务数据非个人数据 |

## 4. 跨平台边界

- **Android**：SharedPreferences 应用键即数据集（含无前缀的
  theme_preference/privacy_settings）；「清除应用数据/卸载」完成删除
  （隐私政策 §1.6），应用内导出/清除归后续项（登记）；
- **iOS**：`mobile/ios/` 属 L119 会话交付面绝不动，同步登记；
- **服务端响应数据主体请求**：L476 账户面已落（本表已重评）——存储对象为
  profile 与工作区 KV；删除走 `DELETE /api/v1/account`（account-sync §2.5，
  2026-10 落地：持久层先删/内存后删、幂等 204、`AccountDataDeleted` 审计
  留痕），未配 Postgres 时默认内存形态重启即清。

## 5. 泄露响应（Art.33，72h 通知链）

发现 → 评估影响面（L460 trace-id 贯穿已可还原交互链；安全审计日志面
归 L487）→ 通知监管机构与受影响数据主体 → 修复与复盘。当前客户端未接线
同步，服务端账户数据默认内存形态，个人数据持久化面小；流程保留为登记项，
无自动化编排（诚实边界）。
