---
sidebar_position: 1
---

# Alpha Finance 简介

Alpha Finance 是 A 股低延迟行情数据与分析平台：一套 Rust 核心（指标、
形态、回测、风控），多端消费——Web 实时看板、桌面应用（Tauri 窗口装同一
web/dist 静态页，另带原生导出/通知/托盘）、Android（alpha-mobile UniFFI 骨架）。

## 你能用它做什么

- **看实时行情**：看板订阅 `real-time-feed` WebSocket 推送，价格与成交量实时刷新；
  后端不可达回退演示数据并在状态栏注明，断线出复位按钮可重连。
- **做技术分析**：SMA/EMA/RSI/Bollinger/MACD、K 线与线形形态、Elliott 波浪校验、
  网格寻优 + 滚动前推回测、VaR/夏普/索提诺等风险度量。
- **管自选与工作区**：多工作区标签页，各存一套自选标的集，浏览器本地持久化。
- **导数据**：看板快照一键导出 CSV；单标历史序列走市场数据 API（另有 CSV 端点）。

## 下一步

- 第一次用 → [快速开始](./getting-started.md)
- 装到本机/手机 → [安装指南](./installation.md)
- 自己部署服务 → [部署指南](./deployment.md)
- 日常操作细节 → [用户指南](./user-guide.md)

工程深水区（架构、协议、合规）同样在本手册内，与用户指南分栏，互不掺杂。
