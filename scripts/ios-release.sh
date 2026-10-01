#!/usr/bin/env bash
# iOS IPA 签名与 TestFlight/App Store 发布（L518）
#
# 定位：**可执行登记**——完整发布流程封装为带前置守卫的分步脚本；
# 非 macOS 环境明确跳过（exit 2），供 CI 矩阵无条件调用。真机签名/
# 上传需 Apple Developer 账号与证书（CI secret），本仓不持有任何密钥。
#
# 流程（STAGE=bindings|archive|export|upload|all）：
#   1. bindings: mobile/ios/gen-bindings.sh 产 FFI 绑定（依赖 L118 的
#      aarch64-apple-ios 静态库）
#   2. archive:  xcodebuild -workspace archive（Release +CODE_SIGNING_ALLOWED 走
#      Xcode 自动签名）
#   3. export:   exportArchive 以 ExportOptions（app-store method）出 IPA
#   4. upload:   TestFlight 上传（altool --api-key；Xcode 14 后 altool 弃用
#      时换 Transporter/App Store Connect API，见 docs/ios-release.md §4）
#
# 用法：STAGE=all APP_SPEC=com.alpha.ios PROFILE_NAME="Alpha App Store" \
#       API_KEY_ID=... API_ISSUER=... scripts/ios-release.sh
# 退出码：0 成功 / 1 步骤失败 / 2 环境不支持（Linux CI 预期值）

set -euo pipefail
cd "$(dirname "$0")/.."

fail()  { echo "❌ $*"; exit 1; }
ok()    { echo "✅ $*"; }
info()  { echo "ℹ️  $*"; }

STAGE="${STAGE:-all}"
APP_SPEC="${APP_SPEC:-com.alpha.ios}"
PROFILE_NAME="${PROFILE_NAME:-Alpha App Store}"
API_KEY_ID="${API_KEY_ID:-}"
API_ISSUER="${API_ISSUER:-}"
BUILD_DIR="${BUILD_DIR:-build/ios}"

# ---- 环境守卫：非 macOS / 缺工具链 = 跳过而非失败（Linux CI 无条件调用） ----
if [ "$(uname -s)" != "Darwin" ]; then
  info "iOS 发布需 macOS（当前 $(uname -s)）——跳过（CI 矩阵中由 macOS 作业执行）"
  exit 2
fi
for t in xcodebuild xcrun; do
  command -v "$t" >/dev/null 2>&1 || fail "缺少 $t（请安装 Xcode 命令行工具）"
done

run_bindings() {
  [ -f mobile/ios/gen-bindings.sh ] || fail "缺 mobile/ios/gen-bindings.sh（L119 交付面）"
  bash mobile/ios/gen-bindings.sh
  ok "FFI 绑定生成完成"
}

run_archive() {
  [ -d mobile/ios ] || fail "缺 mobile/ios/（L119 交付面）"
  local ws
  ws=$(ls mobile/ios/*.xcworkspace mobile/ios/*.xcodeproj 2>/dev/null | head -1) \
    || fail "mobile/ios 下无 Xcode 工程（.xcworkspace/.xcodeproj）——工程创建归 iOS 接入项（L470 发布流水线时落地）"
  mkdir -p "$BUILD_DIR"
  local ws_name
  ws_name=$(basename "$ws")
  if [[ "$ws_name" == *.xcodeproj ]]; then
    xcodebuild -project "mobile/ios/$ws_name" -scheme Alpha \
      -configuration Release -destination "generic/platform=iOS" \
      -archivePath "$BUILD_DIR/Alpha.xcarchive" archive
  else
    xcodebuild -workspace "mobile/ios/$ws_name" -scheme Alpha \
      -configuration Release -destination "generic/platform=iOS" \
      -archivePath "$BUILD_DIR/Alpha.xcarchive" archive
  fi
  ok "archive 完成: $BUILD_DIR/Alpha.xcarchive"
}

run_export() {
  local opts="$BUILD_DIR/ExportOptions.plist"
  cat > "$opts" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>method</key><string>app-store-connect</string>
    <key>signingStyle</key><string>automatic</string>
    <key>stripSwiftSymbols</key><true/>
    <key>teamID</key><string>${TEAM_ID:?TEAM_ID 必填（Apple Developer Team）}</string>
</dict>
</plist>
EOF
  xcodebuild -exportArchive -archivePath "$BUILD_DIR/Alpha.xcarchive" \
    -exportOptionsPlist "$opts" -exportPath "$BUILD_DIR"
  ok "IPA 导出完成: $BUILD_DIR/"
}

run_upload() {
  local ipa
  ipa=$(ls "$BUILD_DIR"/*.ipa | head -1) || fail "未找到 IPA"
  [ -n "$API_KEY_ID" ] && [ -n "$API_ISSUER" ] \
    || fail "TestFlight 上传需 API_KEY_ID/API_ISSUER（App Store Connect API 密钥）"
  # 私钥文件按 altool 约定放 ~/.appstoreconnect/private_keys/AuthKey_<ID>.p8
  xcrun altool --upload-app -f "$ipa" --apiKey "$API_KEY_ID" --apiIssuer "$API_ISSUER"
  ok "已上传 TestFlight（处理完成后 App Store Connect 可见，外部测试需提审）"
}

case "$STAGE" in
  bindings) run_bindings ;;
  archive)  run_archive ;;
  export)   run_export ;;
  upload)   run_upload ;;
  all)      run_bindings; run_archive; run_export; run_upload ;;
  *) fail "STAGE 须为 bindings|archive|export|upload|all" ;;
esac
