#!/usr/bin/env bash
# 桌面自动更新 feed 生成器（L519：自动更新和增量更新机制）
#
# 生成 Tauri v1 updater 消费的 latest.json 更新清单：
#   {
#     "version": "...", "notes": "...", "pub_date": "<RFC3339 UTC>",
#     "platforms": { "<rust-target>": { "signature": "...", "url": "..." } }
#   }
#
# 用法：
#   scripts/release-update-feed.sh --version 0.2.0 \
#     --notes "修复..." \
#     --platform x86_64-unknown-linux-gnu=https://cdn.example/alpha-0.2.0.AppImage=<minisign签名> \
#     [--platform ... 可重复] [--out dist/latest.json]
#
# 签名（minisign）由 tauri signer generate 的私钥（CI secret，绝不入库）对
# 安装包产出；本脚本只做清单拼装与格式校验，不接触密钥。
# 激活链路见 docs/auto-update.md（updater active 翻真 + pubkey 入 conf）。

set -euo pipefail
cd "$(dirname "$0")/.."

fail() { echo "❌ $*"; exit 1; }
ok()   { echo "✅ $*"; }

VERSION="" NOTES="" OUT="dist/latest.json"
PLATFORMS=()

while [ $# -gt 0 ]; do
  case "$1" in
    --version)  VERSION="$2"; shift 2 ;;
    --notes)    NOTES="$2"; shift 2 ;;
    --platform) PLATFORMS+=("$2"); shift 2 ;;
    --out)      OUT="$2"; shift 2 ;;
    *) fail "未知参数: $1" ;;
  esac
done

[ -n "$VERSION" ] || fail "--version 必填（semver，如 0.2.0）"
echo "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$' \
  || fail "版本号须为 semver: $VERSION"
[ ${#PLATFORMS[@]} -gt 0 ] || fail "至少一个 --platform <rust-target>=<url>=<signature>"

mkdir -p "$(dirname "$OUT")"

python3 - "$VERSION" "$NOTES" "$OUT" "${PLATFORMS[@]}" <<'PY'
import json, re, sys, datetime

version, notes, out = sys.argv[1], sys.argv[2], sys.argv[3]
platforms = {}

for spec in sys.argv[4:]:
    parts = spec.split("=", 2)
    if len(parts) != 3 or not all(parts):
        print(f"❌ --platform 须为 <rust-target>=<url>=<signature>: {spec}", file=sys.stderr)
        sys.exit(1)
    target, url, signature = parts
    if not re.match(r"^[a-z0-9_]+(-[a-z0-9_]+)+$", target):
        print(f"❌ rust target 形如 x86_64-unknown-linux-gnu: {target}", file=sys.stderr)
        sys.exit(1)
    platforms[target] = {"signature": signature, "url": url}

feed = {
    "version": version,
    "notes": notes,
    "pub_date": datetime.datetime.now(datetime.timezone.utc)
        .strftime("%Y-%m-%dT%H:%M:%SZ"),
    "platforms": platforms,
}

with open(out, "w", encoding="utf-8") as f:
    json.dump(feed, f, ensure_ascii=False, indent=2)
    f.write("\n")
print(f"latest.json: {len(platforms)} 平台 -> {out}")
PY

python3 -c "import json,sys; json.load(open('$OUT'))" || fail "产物不是合法 JSON"
ok "更新清单生成：$OUT（version=$VERSION，上传 CDN 后即对 updater 可见）"
