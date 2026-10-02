#!/usr/bin/env bash
# 版本一致性门禁（TODO L472）：五处版本号必须同值，SW 缓存名与版本同后缀。
#
# 对账表（唯一事实源 = 根 Cargo.toml [workspace.package] version）：
#   根 Cargo.toml [workspace.package] | web/app/package.json |
#   desktop/tauri.conf.json package.version | Android versionName |
#   web/app/public/sw.js SHELL/ASSET_CACHE 后缀（alpha-shell-vX.Y.Z）
# 另：Android versionCode 只校验为正整数（Play 单调递增由 bump-version 显式给）；
# mobile/ios 属 L119 交付面，本脚本只读不写——连读都跳过（无版本文件在仓内，
# 强行校验只会误伤），提醒由 bump-version.sh 打印。
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

python3 - "$ROOT" <<'EOF'
import json, re, sys
from pathlib import Path

root = Path(sys.argv[1])
errors: list[str] = []

def workspace_version() -> str:
    text = (root / "Cargo.toml").read_text()
    m = re.search(r"\[workspace\.package\][^\[]*?^version\s*=\s*\"([^\"]+)\"", text, re.M | re.S)
    if not m:
        errors.append("根 Cargo.toml 找不到 [workspace.package] version")
        return ""
    return m.group(1)

def package_json_version() -> str:
    try:
        return json.loads((root / "web/app/package.json").read_text())["version"]
    except Exception as e:
        errors.append(f"web/app/package.json 读取失败：{e}")
        return ""

def tauri_version() -> str:
    try:
        return json.loads((root / "desktop/tauri.conf.json").read_text())["package"]["version"]
    except Exception as e:
        errors.append(f"desktop/tauri.conf.json package.version 读取失败：{e}")
        return ""

def android_versions() -> tuple[str, str]:
    try:
        text = (root / "mobile/android/app/build.gradle.kts").read_text()
        name = re.search(r'versionName\s*=\s*"([^"]+)"', text)
        code = re.search(r'versionCode\s*=\s*(\d+)', text)
        if not name:
            errors.append("build.gradle.kts 找不到 versionName")
        if not code:
            errors.append("build.gradle.kts 找不到 versionCode")
        return (name.group(1) if name else "", code.group(1) if code else "")
    except Exception as e:
        errors.append(f"build.gradle.kts 读取失败：{e}")
        return ("", "")

def sw_cache_suffix() -> str:
    try:
        text = (root / "web/app/public/sw.js").read_text()
        m = re.search(r"alpha-shell-v([0-9]+\.[0-9]+\.[0-9]+)", text)
        if not m:
            errors.append("sw.js SHELL_CACHE 未带版本后缀（期望 alpha-shell-vX.Y.Z）")
            return ""
        m2 = re.search(r"alpha-assets-v([0-9]+\.[0-9]+\.[0-9]+)", text)
        if not m2 or m2.group(1) != m.group(1):
            errors.append("sw.js SHELL/ASSET 缓存版本后缀不一致")
        return m.group(1)
    except Exception as e:
        errors.append(f"sw.js 读取失败：{e}")
        return ""

ws = workspace_version()
web = package_json_version()
tauri = tauri_version()
aname, acode = android_versions()
sw = sw_cache_suffix()

print(f"workspace={ws} web={web} tauri={tauri} android={aname} (code {acode}) sw={sw}")
for label, v in [("web", web), ("tauri", tauri), ("android versionName", aname), ("sw cache", sw)]:
    if v and v != ws:
        errors.append(f"{label} 版本 {v} 与 workspace {ws} 不一致（用 scripts/bump-version.sh {ws} 拉齐）")
if acode and not acode.isdigit():
    errors.append("Android versionCode 非数字")

if errors:
    print("版本漂移：")
    for e in errors:
        print(f"  - {e}")
    sys.exit(1)
print("版本一致。")
EOF
