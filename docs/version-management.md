# 版本管理与热更新

## 1. 版本唯一事实源

根 `Cargo.toml` `[workspace.package] version`（当前 0.1.0）。
全员 Rust crate `version.workspace = true`，改一处全跟。

`scripts/bump-version.sh X.Y.Z [--android-code N]` 一处输入、五处同改：

| 文件 | 字段 | 说明 |
|---|---|---|
| `Cargo.toml` | `[workspace.package] version` | 事实源 |
| `web/app/package.json` | `version` | 前端构建与 CDN 产物标识 |
| `desktop/tauri.conf.json` | `package.version` | 安装包与 updater 比对基准（`{{current_version}}` 模板是运行时占位，不动） |
| `mobile/android/app/build.gradle.kts` | `versionName` | 展示版本；`versionCode` 缺省不动，显式 `--android-code` 才动（Play 单调递增，自动化不猜数字） |
| `web/app/public/sw.js` | `SHELL_CACHE`/`ASSET_CACHE` 后缀 | `alpha-shell-vX.Y.Z`——新 SW 安装即热更新生效，旧缓存由 `activate` 清理 |

约束：严格 `X.Y.Z`（后缀会污染全链路版本比较）、只允许向前、
默认拒绝脏树、`mobile/ios` 绝不动（L119，脚本末尾打印人工提醒）。

门禁 `scripts/check-version.sh`（CI `version` 作业）：五处同值 + versionCode
为正整数，否则 exit 1。注意首推前 SW 缓存名为历史 `v1` 形态，门禁会红——
第一次 `bump-version.sh` 即拉齐。

## 2. 热更新通道（各形态已交付，本项只做版本接缝）

| 形态 | 通道 | 版本接缝 |
|---|---|---|
| 桌面 | Tauri updater（L519：`latest.json` feed + minisign 签名链） | `release-update-feed.sh` 发版时取本版本号产 feed；客户端按 `package.version` 比对 |
| Web | Service Worker（L507：预缓存壳 + 运行时填充） | SW 缓存名跟版本——发版即新 SW，`activate` 清旧缓存就是更新本身 |
| Android | 商店内更新（Play 内应用更新归 L471） | `versionCode` 递增是更新的唯一判据，bump 时显式给 |
| iOS | App Store/TestFlight（明确不做自更新，L519 边界） | L119 面手工同步 |

增量差分现实（L519 已登记）：Tauri 整包替换无内置差分，
Windows/macOS bsdiff 与 AppImage zsync 待 L516 打包形态复核——不自行开发 delta 协议。

## 3. 与 L470/L471 分工

- L470 发布流水线 = 执行者（取本版本号打产物、喂 feed、推镜像）；
- L471 商店发布 = 上架面（各店审核与分阶段发布）；
- 本项 = 版本号本身（推进工具 + 一致性门禁 + 通道接缝表）。
