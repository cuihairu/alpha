#!/usr/bin/env bash
# 发版版本号推进（TODO L472）：一处输入，五处同改。
#
# 用法：scripts/bump-version.sh 0.2.0 [--android-code 2] [--allow-dirty]
# 语义：
# - 严格 semver X.Y.Z（不带后缀——workspace 全员 version.workspace=true，
#   后缀会污染 Docker/Play/Updater 全链路的版本比较）；
# - 只允许向前（大于当前 workspace 版本，防手滑回退）；
# - 默认拒绝脏树（先 commit 再发版，版本号与代码状态一一对应）；
# - Android versionCode 缺省不动（Play 要求单调递增，自动化不猜数字，
#   需要时显式 --android-code 传入）；
# - mobile/ios 绝不动（L119 交付面），脚本末尾打印人工提醒；
# - 改后自动跑 scripts/check-version.sh 自证。
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

NEW="${1:-}"
shift || true
CODE=""
ALLOW_DIRTY=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --android-code=*) CODE="${1#*=}"; shift ;;
    --android-code) CODE="${2:-}"; shift 2 ;;
    --allow-dirty) ALLOW_DIRTY=1; shift ;;
    *) echo "未知参数：$1" >&2; exit 2 ;;
  esac
done

if ! [[ "$NEW" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "用法：$0 X.Y.Z [--android-code N] [--allow-dirty]" >&2
  exit 2
fi
if [[ -n "$CODE" ]] && ! [[ "$CODE" =~ ^[0-9]+$ ]]; then
  echo "--android-code 须为正整数" >&2
  exit 2
fi
if [[ "$ALLOW_DIRTY" != 1 ]] && [[ -n "$(git -C "$ROOT" status --porcelain)" ]]; then
  echo "工作树不干净：先提交再推进版本（或加 --allow-dirty）" >&2
  exit 1
fi
CUR="$(grep -A4 '^\[workspace\.package\]' "$ROOT/Cargo.toml" | grep -oP '^version\s*=\s*"\K[^"]+' | head -1)"
if [[ "$(printf '%s\n%s\n' "$CUR" "$NEW" | sort -V | head -1)" != "$CUR" ]] || [[ "$CUR" == "$NEW" ]]; then
  echo "只允许向前推进：当前 $CUR，目标 $NEW" >&2
  exit 1
fi

python3 - "$ROOT" "$NEW" "$CODE" <<'EOF'
import json, re, sys
from pathlib import Path

root, new, code = Path(sys.argv[1]), sys.argv[2], sys.argv[3]

def sub(path: str, pattern: str, repl: str, count: int = 1) -> None:
    p = root / path
    text = p.read_text()
    new_text, n = re.subn(pattern, repl, text, count=count, flags=re.M)
    if n != count:
        sys.exit(f"{path} 替换失败（命中 {n} 处，期望 {count} 处），已中止、未写回")
    p.write_text(new_text)
    print(f"  {path}")

print(f"{new}:")
# 1. workspace 唯一事实源（全员 version.workspace=true，改一处全跟）
sub("Cargo.toml", r'(?m)(^\[workspace\.package\][^\[]*?^version\s*=\s*")[^"]+(")', rf"\g<1>{new}\g<2>")
# 2. web
pkg = root / "web/app/package.json"
data = json.loads(pkg.read_text())
data["version"] = new
pkg.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n")
print("  web/app/package.json")
# 3. desktop（package.version；{{current_version}} 模板是运行时占位，不动）
tauri = root / "desktop/tauri.conf.json"
data = json.loads(tauri.read_text())
data["package"]["version"] = new
tauri.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n")
print("  desktop/tauri.conf.json")
# 4. android（versionName 同值；versionCode 显式才动）
sub("mobile/android/app/build.gradle.kts", r'(?m)(^\s*versionName\s*=\s*")[^"]+(")', rf"\g<1>{new}\g<2>")
if code:
    sub("mobile/android/app/build.gradle.kts", r"(?m)(^\s*versionCode\s*=\s*)\d+", rf"\g<1>{code}")
# 5. SW 缓存名跟版本（新 SW 安装即热更新生效，旧缓存由 activate 清理；
#    正则兼容历史 v1 无点分形态，首推后统一为 vX.Y.Z）
sub("web/app/public/sw.js", r"alpha-shell-v[^'\"]+", f"alpha-shell-v{new}")
sub("web/app/public/sw.js", r"alpha-assets-v[^'\"]+", f"alpha-assets-v{new}")
EOF

bash "$ROOT/scripts/check-version.sh"
echo "提醒：mobile/ios 版本号手工同步（L119 交付面，本脚本绝不动）；"
echo "提醒：updater feed 与镜像 tag 在发版时由 L470 流水线取本版本号（release-update-feed.sh 参数传入）。"
