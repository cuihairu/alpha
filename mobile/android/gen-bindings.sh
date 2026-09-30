#!/usr/bin/env bash
# L301：UniFFI 绑定生成 + Android .so 交叉编译与放置（真机/模拟器构建的前置步骤）。
# 用法：mobile/android/gen-bindings.sh
# 前置：rustup + aarch64-linux-android target（脚本内自动补装）、Android NDK
# （r27+，llvm-clang 包装器；ANDROID_NDK_HOME 可覆盖，默认取 ~/android-sdk）。
# 入库约定：uniffi/*.kt 绑定入库（消费面随提交可审）；.so 不入库（见 .gitignore）。
set -euo pipefail

ANDROID_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)" # mobile/android
REPO_ROOT="$(cd "$ANDROID_ROOT/../.." && pwd)"
OUT_JAVA="$ANDROID_ROOT/app/src/main/java"
JNI_LIBS="$ANDROID_ROOT/app/src/main/jniLibs/arm64-v8a"
ABI=aarch64-linux-android
API_LEVEL=26 # 与 app/build.gradle.kts minSdk 一致
NDK_ROOT="${ANDROID_NDK_HOME:-$HOME/android-sdk/ndk/27.3.13750724}"
LINKER="$NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin/${ABI}${API_LEVEL}-clang"

# 1) Kotlin 绑定：从 host 编译产物读 uniffi 元数据（scaffolding 已由
#    setup_scaffolding! 在编译期进库，proc-macro-only 无 UDL 副本）
cargo build -p alpha-mobile --features bindgen
cargo run -p alpha-mobile --features bindgen --bin uniffi-bindgen -- generate \
  --library "$REPO_ROOT/target/debug/libalpha_mobile.so" \
  --language kotlin --out-dir "$OUT_JAVA"

# 2) Android arm64 .so：NDK clang 作 linker，产物进 jniLibs（ABI 过滤见
#    app/build.gradle.kts ndk.abiFilters）
rustup target add "$ABI" >/dev/null
CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$LINKER" \
  cargo build -p alpha-mobile --target "$ABI"
mkdir -p "$JNI_LIBS"
cp "$REPO_ROOT/target/$ABI/debug/libalpha_mobile.so" "$JNI_LIBS/"

echo "已生成：$OUT_JAVA/uniffi/alpha_mobile/alpha_mobile.kt + $JNI_LIBS/libalpha_mobile.so"
echo "（可选）安装 ktlint 后 uniffi 会自动格式化生成的 Kotlin。"
