#!/usr/bin/env bash
# 桌面端多平台安装包构建（L516：Windows .exe、macOS .dmg、Linux .AppImage）
#
# 定位：按宿主 OS 出对应安装包（tauri bundler 单宿主单面）；跨 OS 包由 CI
# 矩阵的对应作业执行（scripts/check-desktop.sh 同款分工——本机 Linux 只出
# AppImage/deb，.dmg 归 macOS 作业、.exe/.msi 归 Windows 作业，接线归 L470）。
#
# 产物矩阵（tauri.conf.json bundle 配置驱动）：
#   Linux   → .AppImage（主分发，x11）+ .deb（Debian 系）
#   macOS   → .dmg + .app（签名/公证见 docs/desktop-release.md §macOS）
#   Windows → .exe（NSIS）+ .msi（WiX，二选一或全出）
#
# 与 L519 更新通道的接缝：updater 翻真后 `tauri build` 额外产出
# *.AppImage.tar.gz + *.AppImage.sig（minisign），喂给
# scripts/release-update-feed.sh 拼装 latest.json。
#
# 用法：scripts/desktop-release.sh [--debug] [bundles...]（缺省=宿主全量）

set -euo pipefail
cd "$(dirname "$0")/.."

fail() { echo "❌ $*"; exit 1; }
ok()   { echo "✅ $*"; }
info() { echo "ℹ️  $*"; }

DEBUG_FLAG=""
BUNDLES=()
while [ $# -gt 0 ]; do
  case "$1" in
    --debug) DEBUG_FLAG="--debug" ;;
    appimage|deb|dmg|app|msi|nsis|updater) BUNDLES+=("$1") ;;
    *) fail "未知参数: $1（支持 --debug 与 bundle 名 appimage|deb|dmg|app|msi|nsis|updater）" ;;
  esac
  shift
done

command -v cargo >/dev/null 2>&1 || fail "缺 cargo"
command -v cargo-tauri >/dev/null 2>&1 \
  || fail "缺 tauri CLI——安装：cargo install tauri-cli --version '^1.5' --locked"

OS="$(uname -s)"
case "$OS" in
  Linux)  DEFAULT_BUNDLES=(appimage deb) ;;
  Darwin) DEFAULT_BUNDLES=(app dmg) ;;
  MINGW*|MSYS*|CYGWIN*) DEFAULT_BUNDLES=(nsis msi) ;;
  *) fail "不支持的宿主: $OS（跨 OS 包归 CI 矩阵对应作业，L470）" ;;
esac
[ ${#BUNDLES[@]} -eq 0 ] && BUNDLES=("${DEFAULT_BUNDLES[@]}")

# AppImage 工具在无 FUSE 环境（容器/CI）以 extract-and-run 模式工作
if [ "$OS" = "Linux" ]; then
  export APPIMAGE_EXTRACT_AND_RUN=1
  # webview 依赖自检（check-desktop.sh 同清单）：缺则提示而非失败到深处
  for lib in gtk+-3.0 webkit2gtk-4.0; do
    pkg-config --exists "$lib" 2>/dev/null || \
      info "缺 $lib 开发包——构建可能失败（Linux 需 libwebkit2gtk-4.0-dev 等）"
  done
fi

echo "=== 桌面安装包构建（host=$OS，bundles=${BUNDLES[*]}）==="
# tauri CLI 须从 app 目录调用：beforeBuildCommand 的 `cd ../web` 相对 desktop/ 解析，
# bundle 产物落 workspace 共享 target/release/bundle/
( cd desktop && cargo tauri build $DEBUG_FLAG ${BUNDLES[*]/#/--bundles } )

echo "=== 产物 ==="
find target/release/bundle -maxdepth 2 -type f \( -name '*.AppImage' -o -name '*.deb' \
  -o -name '*.dmg' -o -name '*.app' -o -name '*.msi' -o -name '*.exe' \) 2>/dev/null \
  | while read -r f; do echo "  [$(du -h "$f" | cut -f1)] $f"; done
find target/release -maxdepth 1 -name '*.tar.gz' -o -maxdepth 1 -name '*.sig' 2>/dev/null \
  | while read -r f; do echo "  [$(du -h "$f" | cut -f1)] $f — updater feed 素材（L519）"; done

ok "桌面安装包构建完成"
