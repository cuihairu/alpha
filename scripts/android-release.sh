#!/usr/bin/env bash
# Android 分包与多渠道发布构建（L517：Android APK/AAB 分包和多渠道发布）
#
# 产物矩阵（docs/android-release.md §渠道矩阵）：
#   play   → app-play-release.aab（上架 Google Play，设备分包由 Play 动态下发）
#   play   → app-{arm64-v8a,x86_64}-release.apk（按 ABI APK，渠道外备用）
#   direct → app-universal-release.apk（官网/侧载，单文件全 ABI）
#
# 签名：设置 ALPHA_KEYSTORE_PATH/ALPHA_KEYSTORE_PASSWORD/ALPHA_KEY_ALIAS/
# ALPHA_KEY_PASSWORD 四个环境变量即签名出包；缺省构建 unsigned（不可安装，
# 仅冒烟）。keystore 绝不入库。
#
# 用法：scripts/android-release.sh [--skip-bindings]
# 前置：Android SDK（ANDROID_HOME 可覆盖，默认 ~/android-sdk）+ NDK + JDK 17+

set -euo pipefail
cd "$(dirname "$0")/.."

fail() { echo "❌ $*"; exit 1; }
ok()   { echo "✅ $*"; }
info() { echo "ℹ️  $*"; }

SKIP_BINDINGS=0
[ "${1:-}" = "--skip-bindings" ] && SKIP_BINDINGS=1

export ANDROID_HOME="${ANDROID_HOME:-$HOME/android-sdk}"
[ -d "$ANDROID_HOME" ] || fail "Android SDK 不存在: $ANDROID_HOME"
command -v java >/dev/null 2>&1 || fail "缺 java（JDK 17+）"

GRADLE_DIR=mobile/android
OUT="$GRADLE_DIR/app/build/outputs"

if [ "$SKIP_BINDINGS" = 0 ]; then
  echo "=== [1/3] 双 ABI 绑定与 .so（gen-bindings.sh）==="
  bash "$GRADLE_DIR/gen-bindings.sh"
else
  info "跳过 bindings（--skip-bindings，要求 jniLibs 已就绪）"
  ls "$GRADLE_DIR/app/src/main/jniLibs"/*/libalpha_mobile.so >/dev/null 2>&1 \
    || fail "jniLibs 无 .so——先跑 gen-bindings.sh 或去掉 --skip-bindings"
fi

echo "=== [2/3] 渠道产物构建（gradle）==="
( cd "$GRADLE_DIR" && ./gradlew \
    :app:bundlePlayRelease \
    :app:assemblePlayRelease \
    :app:assembleDirectRelease )

echo "=== [3/3] 产物清单 ==="
find "$OUT" -name '*.aab' -o -name '*release*.apk' | while read -r f; do
  sz=$(du -h "$f" | cut -f1)
  case "$f" in
    *.aab) sign="AAB（正式分发签名由 Play App Signing 托管）" ;;
    *unsigned*) sign="unsigned（设 ALPHA_KEYSTORE_* 后重构建即签名）" ;;
    *) sign="signed" ;;
  esac
  echo "  [$sz] $f — $sign"
done

ok "Android 多渠道产物构建完成（AAB → Play Console；universal APK → 官网/侧载）"
