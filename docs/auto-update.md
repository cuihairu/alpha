# 自动更新与增量更新机制

本单落地**更新分发通道的完整骨架**：桌面 Tauri updater 登记（schema 已入
conf 并被契约测试锁定、feed 生成器就绪）+ 各平台更新策略边界。激活
（active 翻真）依赖发布流水线产物与密钥，接线归 L470/L472。

## 1. 桌面（Tauri v1 updater）

三件套：

1. **conf 登记**（`desktop/tauri.conf.json` → `tauri.updater`）：
   `active: false`（未激活）、`dialog: true`（更新可用时弹用户确认）、
   `pubkey: ""`（翻真时填 minisign 公钥）、endpoints 模板
   `…/tauri/{{target}}/{{current_version}}`。契约测试
   `tests/tauri_config.rs::updater_registered_but_inert` 锁定「已登记但
   inert」——active 翻真而无 pubkey 会让 gui 构建期失败，测试先行拦住。
   注：Url 解析会把模板里的 `{}` 规范化为 `%7B/%7D`，tauri v1 updater
   运行时对两种形态都做占位符替换（测试同时验证两种形态）。
2. **feed 生成器**（`scripts/release-update-feed.sh`）：按 Tauri v1
   manifest 格式产 `latest.json`（version/notes/pub_date(RFC3339
   UTC)/platforms\{signature,url\}），semver 与 rust triple 在入口校验，
   产物过 JSON 合法性检查。CI 在打包作业末尾调用并上传静态 CDN。
3. **签名链路**（翻真步骤）：`tauri signer generate` 产出 minisign
   密钥对——**公钥**入 conf `pubkey`，**私钥**进 CI secret（绝不入库）；
   打包作业对每个安装包产出 `.<ext>.signature`，feed 的 `signature`
   字段取自该文件内容。

**增量更新现实**：Tauri v1 的更新是整包替换（下载新安装包→校验
minisign 签名→重装），无内置差分。带宽优化路径登记：Windows NSIS/
msi 差分与 macOS 差分需自建 bsdiff 服务端 + 客户端 hook，归 L516
打包形态确定后复核；Linux AppImage 有 zsync 类差分潜力，同批评估。
**不做**：自行开发 delta 协议（收益/复杂度比在当前用户量级不成立）。

## 2. 各平台策略边界

| 平台 | 通道 | 边界 |
|---|---|---|
| Desktop | Tauri updater（上述） | 激活归 L470；feed 托管为静态 CDN 对象（L515 衔接），不引入动态更新服务 |
| Web | Service Worker 更新流 | 归 L506 PWA：`registration.update()` 轮询 + 新 SW `skipWaiting` + 提示刷新；本单不动 |
| Android | 应用商店（Play 内更新） | 自建 APK 侧载更新在 Play 渠道违反政策；应用内更新提示归 L517/L471 |
| iOS | TestFlight / App Store | **平台强制**：自更新/热更新违反 App Store 审核条款，明确不做；L518 走商店通道 |

移动端核心库（mobile/）的 Rust 侧无独立更新面——版本随应用壳整体发布。

## 3. 与相邻项的分工

- **L472（自动化版本管理和热更新）**：管版本号 bump/changelog/发布
  编排的自动化；本单只管「装到用户机器上的更新通道」。feed 生成器
  是两者的接缝（L472 编排产出 → 本脚本拼清单）。
- **L470（多平台发布流水线）**：打包作业 + 签名 secret + 上传 feed 的
  执行者；本单提供 feed 格式与生成器。
- **L515（Web CDN）**：feed 与安装包同 CDN 托管；endpoint 域名随之定。

## 4. 非交互假设

1. endpoint 域名 `releases.alpha.finance.example` 为占位，L470/L515
   定托管后替换；
2. 桌面用户量级假设下整包更新可接受，差分优化登记不实现；
3. 更新弹窗走 tauri 内置 dialog（`dialog: true`），不自绘更新 UI。
