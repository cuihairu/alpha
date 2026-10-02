---
sidebar_position: 3
---

# 安装指南

## Web（PWA，可安装）

线上地址用浏览器打开 → 分享菜单/地址栏「安装应用」即可离线使用。
离线行为：应用壳走 Service Worker 预缓存，实时区降级为模拟盘（见 L507）。

## 桌面端

三平台安装包由 `scripts/desktop-release.sh` 产出（见 `docs/desktop-release.md`）：

| OS | 产物 |
|---|---|
| Linux | `.AppImage` / `.deb` |
| macOS | `.app` / `.dmg`（公证走 CI secret） |
| Windows | `.exe`（nsis）/ `.msi` |

桌面复用同一 Web 前端，功能与 Web 完全一致（含本手册用户指南全部条目）。
自动更新走 Tauri updater 通道（见 `docs/auto-update.md`）。

## Android

- Play 渠道：`.aab`（动态设备分包）；直发渠道：按 ABI 拆分的 `.apk` + 兜底 universal 包。
- 构建与签名见 `docs/android-release.md`；双 ABI（arm64 真机 + x86_64 模拟器）。
- 小组件与快捷方式见 `docs/android-widget.md`；通知与同步架构见 `docs/mobile-push-sync.md`。

## iOS

TestFlight / App Store 链路见 `docs/ios-release.md`（需 macOS + 签名资产，
均走 CI secret，本仓零密钥）。
