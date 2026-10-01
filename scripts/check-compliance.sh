#!/usr/bin/env bash
# 平台合规性检查（L520：平台合规性检查和适配——隐私政策、权限申请）
#
# 检查项：
#   1. 权限注册表（config/compliance/permissions-registry.txt）本身合法：
#      [allow] 段每条都带非空理由；
#   2. AndroidManifest.xml 的 <uses-permission> ⊆ 注册表 android 段
#      （当前骨架零权限——任何新增权限都须先登记 = conscious ack）；
#   3. desktop/tauri.conf.json allowlist 启用组与注册表 tauri 段双向对账：
#      未登记的启用组 = 失败；登记但已不再启用的陈旧条目 = 失败；
#   4. Tauri CSP 非空（webview 内容安全基线）。
#
# 用法：scripts/check-compliance.sh   （非交互；依赖 python3）

set -euo pipefail
cd "$(dirname "$0")/.."

fail() { echo "❌ $*"; exit 1; }
ok()   { echo "✅ $*"; }

REGISTRY=config/compliance/permissions-registry.txt
MANIFEST=mobile/android/app/src/main/AndroidManifest.xml
TAURI=desktop/tauri.conf.json

[ -f "$REGISTRY" ] || fail "权限注册表不存在: $REGISTRY"
[ -f "$MANIFEST" ] || fail "Android 清单不存在: $MANIFEST"
[ -f "$TAURI" ]    || fail "Tauri 配置不存在: $TAURI"

echo "=== 平台合规性检查 ==="

python3 - "$REGISTRY" "$MANIFEST" "$TAURI" <<'PY'
import json, re, sys

registry_path, manifest_path, tauri_path = sys.argv[1], sys.argv[2], sys.argv[3]
problems = []

allow = {"android": {}, "tauri": {}, "ios": {}, "web": {}}
section = None
for lineno, raw in enumerate(open(registry_path, encoding="utf-8"), 1):
    line = raw.strip()
    if not line or line.startswith("#"):
        continue
    if line == "[allow]":
        section = "allow"
        continue
    if line == "[planned]":
        section = "planned"
        continue
    if section != "allow":
        continue
    entry, _, rationale = line.partition("#")
    key = entry.strip()
    rationale = rationale.strip()
    if not key:
        problems.append(f"{registry_path}:{lineno}: 空条目")
        continue
    if not rationale:
        problems.append(f"{registry_path}:{lineno}: 条目 {key} 缺少理由（conscious ack 要求非空）")
        continue
    platform, _, perm = key.partition(":")
    if platform not in allow or not perm:
        problems.append(f"{registry_path}:{lineno}: 条目 {key} 平台段须为 android/tauri/ios/web")
        continue
    allow[platform][perm] = rationale

manifest_src = open(manifest_path, encoding="utf-8").read()
declared = re.findall(r'<uses-permission\s+android:name="([^"]+)"', manifest_src)
for perm in declared:
    if perm not in allow["android"]:
        problems.append(
            f"{manifest_path}: 权限 {perm} 未在注册表 [allow] 登记"
            "（新增权限 = conscious ack：先登记理由与隐私影响再抬基线）"
        )

tauri = json.load(open(tauri_path, encoding="utf-8"))
allowlist = tauri.get("tauri", {}).get("allowlist", {})

def enabled(value):
    if isinstance(value, bool):
        return value
    if isinstance(value, dict):
        return any(isinstance(v, bool) and v for v in value.values())
    return False

enabled_groups = {name for name, value in allowlist.items() if enabled(value)}
registered_tauri = set(allow["tauri"])
for group in sorted(enabled_groups - registered_tauri):
    problems.append(f"{tauri_path}: allowlist 启用组 {group} 未在注册表 [allow] 登记")
for group in sorted(registered_tauri - enabled_groups):
    problems.append(f"{registry_path}: tauri:{group} 已登记但配置中未启用（陈旧条目，请移除或修配置）")

csp = tauri.get("tauri", {}).get("security", {}).get("csp", "")
if not csp.strip():
    problems.append(f"{tauri_path}: security.csp 为空（webview 内容安全基线缺失）")

if problems:
    print(f"\n发现 {len(problems)} 处合规问题：")
    for p in problems:
        print(f"  - {p}")
    sys.exit(1)

print(f"注册表 allow 条目: android={len(allow['android'])} tauri={len(allow['tauri'])}"
      f"（对账通过；Android 清单声明 {len(declared)} 项权限、Tauri 启用组 {len(enabled_groups)} 个）")
PY

ok "平台合规性检查全部通过（Android 清单 / Tauri allowlist / CSP / 注册表）"
