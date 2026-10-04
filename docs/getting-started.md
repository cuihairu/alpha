---
sidebar_position: 2
---

# 快速开始

## 环境要求

| 用途 | 需要 |
|---|---|
| 只看 Web 看板 | 现代浏览器（Chrome/Edge/Firefox 近两版），无他 |
| 本地全栈运行 | Rust stable、Node 24、Redis（`scripts/check-e2e.sh` 会起真实进程 + Redis） |
| Android 构建 | Android SDK（`ANDROID_HOME`）+ NDK 27.3（`scripts/gen-bindings.sh` 默认版本，双 ABI 绑定见 `docs/android-release.md`） |
| 文档站预览 | Node 18+（`docs/` 内 `pnpm install && pnpm start`；CI 用 pnpm + Node 24，见 `.github/workflows/docs.yml`） |

## 60 秒看到实时看板

```bash
git clone https://github.com/cuihairu/alpha.git
cd alpha/web/app
npm install
npm run dev
```

打开 `http://localhost:5173`：无后端时看板自动走确定性模拟盘（同 seed 同序列，
刷新可复现）。有后端时加地址覆盖：`?feedWs=ws://<host>:8082/ws`。

## 下一步

- [安装指南](./installation.md)——桌面安装包 / Android APK / PWA 安装
- [用户指南](./user-guide.md)——工作区、导出、隐私、主题
