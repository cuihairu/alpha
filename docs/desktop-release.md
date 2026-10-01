# 桌面端多平台安装包（TODO L516）

编排入口 `scripts/desktop-release.sh`（按宿主 OS 出对应面；macOS dmg/app、
Windows NSIS/WiX 归 CI 矩阵对应作业，本机明确不支持跨 OS 打包——tauri
bundler 单宿主单面，接线归 L470；Linux 本机实测止步见 §6.1）。打包
配置全部由 `desktop/tauri.conf.json` `bundle` 段驱动（identifier
`com.alpha.finance`、category Finance、四平台 icon 已就位）。

## 1. 产物矩阵

| 宿主 | bundles | 说明 |
|---|---|---|
| Linux | `appimage` + `deb` | AppImage 为主分发（免安装、单文件）；deb 覆盖 Debian/Ubuntu 生态。容器/CI 无 FUSE → 脚本置 `APPIMAGE_EXTRACT_AND_RUN=1` |
| macOS | `app` + `dmg` | dmg 分发；签名/公证见 §2 |
| Windows | `nsis`（.exe）+ `msi`（WiX） | NSIS 面向最终用户安装向导；msi 面向企业批量部署。代码签名见 §2 |

`--bundles` 可任选子集，`--debug` 出调试包（体积大、无优化，不分发）。

## 2. 签名与公证（登记，随 L470 CI 落地）

- **macOS**：Developer ID Application 证书 codesign → notarytool 公证
  → staple。未签名 AppImage…未签名 .dmg 在 Gatekeeper 下需右键绕行，
  正式分发必须公证；`tauri.conf.json` `bundle.macOS.signingIdentity`
  现为 null（占位），CI 注入 secret。
- **Windows**：EV Code Signing 证书或 Azure Trusted Signing；未签名
  .exe 触发 SmartScreen 告警。`bundle.windows.certificateThumbprint`
  占位同上。
- **Linux**：无强制签名；AppImage 可选 minisign 签名——与 L519 更新
  通道共用密钥（updater 签名即 minisign）。

## 3. 与更新通道（L519）的接缝

updater 翻真（`tauri.conf.json` active + pubkey + Cargo feature）后，
`tauri build --bundles updater` 额外产出 `*.tar.gz` + `*.sig`
（minisign 对压缩包签名）——这两个文件喂给
`scripts/release-update-feed.sh` 拼装 `latest.json` 的
platforms[target].signature/url。即：**安装包构建一次，分发与自更新
两条通道同时供货**。

## 4. webview 依赖与体积

- Linux 打包机与用户机均需 webkit2gtk-4.0（脚本 pkg-config 自检提示）；
  AppImage 不捆绑 webkit（Tauri v1 策略：依赖系统 webview，与 check-desktop
  的 GUI 依赖清单一致）。
- Windows 用 WebView2（Win10+ 系统自带/自动装）；macOS 用 WKWebView
  （系统自带）——三平台均不捆绑浏览器内核，安装包体积来源主要是
  Rust 二进制与前端产物。

## 5. 边界登记

1. deb 的桌面入口/图标已由 bundler 从 conf 生成；rpm/flatpak 不做
   （用户面窄，登记不实现）；
2. 自动更新侧载渠道的 AppImage 差分（zsync）登记归 L519 复核项；
3. 跨 OS 打包作业的 runner 规格与缓存（cargo/web 产物）归 L470 流水线。

## 6. 非交互假设与实测边界

1. **本机实测止步于「conf 校验通过 + web 构建」**：开发机 Ubuntu 26.04
   只提供 webkit2gtk-4.1（soup3），tauri v1 链接需要 webkit2gtk-4.0
   （soup2）——与仓内 CI「Linux runner 缺 WebKitGTK」既有边界同因。
   真机 AppImage 验证归 L470 的 Linux 作业（老基底镜像，如
   ubuntu-22.04）或 tauri v2 升级项（v2 支持 4.1）——两者登记不实现；
   顺手修掉暴露的真 bug：`windows[0].theme: "System"` 非法（v1 合法值
   仅 Light/Dark），移除该字段（缺省即跟随系统，等价原意）——该 bug
   能存活至今正因本仓从未真跑过 tauri build；
2. 签名资产（证书/密码）全部 CI secret，本仓零密钥；
3. NSIS 与 WiX 双出（用户向 + 企业向），双维护成本可接受前不分省。
