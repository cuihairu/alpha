#!/usr/bin/env bash
# L301：UniFFI 绑定生成 + Android .so 交叉编译与放置（真机/模拟器构建的前置步骤）。
# 用法：mobile/android/gen-bindings.sh
# 前置：rustup + android targets（脚本内自动补装）、Android NDK（r27+，
# llvm-clang 包装器；ANDROID_NDK_HOME 可覆盖，默认取 ~/android-sdk）。
# L517：双 ABI（arm64-v8a 真机 + x86_64 模拟器），分包与多渠道见 app/build.gradle.kts。
# 入库约定：uniffi/*.kt 绑定入库（消费面随提交可审）；.so 不入库（见 .gitignore）。
set -euo pipefail

ANDROID_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)" # mobile/android
REPO_ROOT="$(cd "$ANDROID_ROOT/../.." && pwd)"
OUT_JAVA="$ANDROID_ROOT/app/src/main/java"
API_LEVEL=26 # 与 app/build.gradle.kts minSdk 一致
NDK_ROOT="${ANDROID_NDK_HOME:-$HOME/android-sdk/ndk/27.3.13750724}"

# 1) Kotlin 绑定：从 host 编译产物读 uniffi 元数据（scaffolding 已由
#    setup_scaffolding! 在编译期进库，proc-macro-only 无 UDL 副本）
cargo build -p alpha-mobile --features bindgen
cargo run -p alpha-mobile --features bindgen --bin uniffi-bindgen -- generate \
  --library "$REPO_ROOT/target/debug/libalpha_mobile.so" \
  --language kotlin --out-dir "$OUT_JAVA"

# 2) Android .so（L517 分包前提——多 ABI）：NDK clang 作 linker，产物进
#    jniLibs/<abi>（打包过滤见 app/build.gradle.kts ndk.abiFilters + splits）。
#    arm64-v8a = 主流真机；x86_64 = 模拟器/Chromebook。新增 ABI 在此循环加行。
for pair in aarch64-linux-android:arm64-v8a x86_64-linux-android:x86_64; do
  ABI="${pair%%:*}"
  JNI_ABI="${pair##*:}"
  LINKER="$NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin/${ABI}${API_LEVEL}-clang"
  [ -x "$LINKER" ] || { echo "缺链接器 $LINKER（NDK 不完整？）" >&2; exit 1; }
  rustup target add "$ABI" >/dev/null
  TARGET_UPPER="$(echo "$ABI" | tr 'a-z-' 'A-Z_')"
  env "CARGO_TARGET_${TARGET_UPPER}_LINKER=$LINKER" \
    cargo build -p alpha-mobile --target "$ABI"
  JNI_DIR="$ANDROID_ROOT/app/src/main/jniLibs/$JNI_ABI"
  mkdir -p "$JNI_DIR"
  cp "$REPO_ROOT/target/$ABI/debug/libalpha_mobile.so" "$JNI_DIR/"
  echo "已放置 $JNI_DIR/libalpha_mobile.so"
done
