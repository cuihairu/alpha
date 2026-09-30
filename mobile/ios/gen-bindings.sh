#!/usr/bin/env bash
# L301/iOS equivalent：UniFFI 绑定生成 for Swift + iOS .so 交叉编译（前置步骤）。
# 用法：mobile/ios/gen-bindings.sh
# 前置：rustup + aarch64-apple-ios target、cargo-lipo（可选）、Xcode toolchain
# 入库约定：gen/ 目录下的 Swift 绑定文件与 .so 产物会随提交可审。

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/.."
IOS_OUT="$REPO_ROOT/mobile/ios/gen"
LIB_DEBUG="$REPO_ROOT/target/debug/libalpha_mobile.so"

# 1) 生成 Swift 绑定：从 host .so 读 uniffi 元素器
#   （proc-macro-only 模式：metadata 由编期属性注入，无 UDL 副本）
mkdir -p "$IOS_OUT"
cargo build -p alpha-mobile --features bindgen
cargo run -p alpha-mobile --features bindgen --bin uniffi-bindgen -- generate \
  --library "$LIB_DEBUG" \
  --language swift --out-dir "$IOS_OUT"

echo "已生成 Swift 绑定：$IOS_OUT/alpha_mobile.swift"
echo "（将与 Xcode 项目中的 Framework 同步）"

# 2) （可选）iOS 模拟器/真机 .so：交叉编译与放置
#    需要 Xcode/NDK 环境；此处仅作占位，真机构建由 CI macOS 作业处理。
# sh -c 'cargo install cargo-lipo &&
#   CARGO_TARGET_AARCH64_APPLE_IOS_LINKER=xcrun --sdk iphoneos \
#   cargo build -p alpha-mobile --target aarch64-apple-ios &&
#   mkdir -p mobile/ios/app/libs/arm64 &&
#   cp target/aarch64-apple-ios/debug/libalpha_mobile.so mobile/ios/app/libs/arm64/'