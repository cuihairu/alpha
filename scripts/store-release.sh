#!/usr/bin/env bash
# 应用商店发布集成（L471：Google Play、App Store、Microsoft Store）
#
# 定位：商店提交的**工程面收口**——L517 产出 AAB（Play 形态）、L518 已封
# App Store 上传（ios-release.sh STAGE=upload，TestFlight/审核链）、L516 产
# 桌面安装包；本脚本补上缺口的 **Google Play 上传自动化**（Play Developer
# API v3：服务账号 JWT → edit → AAB bundle 上传 → track 指派 → commit），
# 并在 docs/store-publishing.md 登记三店矩阵与边界。
#
# Play 上传（真实现，非登记）：
#   scripts/store-release.sh play <aab-path> [--track internal|closed|production] [--dry-run]
#   - 凭据：ALPHA_PLAY_SERVICE_ACCOUNT_JSON 指向 Play 服务账号 JSON
#     （type=service_account，release 管理权限；绝不入库，经 CI secret 注入，
#     接线归 L470）。签名用 openssl RS256（仓库既有工具链，无新依赖）。
#   - 上传目标 track 缺省 internal（内测轨先验，再 closed → production 逐级
#     晋级——商店发布纪律：不直发生产）。
#   - --dry-run 只打印将执行的 API 序列与参数（无凭据也可演练）。
#   - 本地联调：ALPHA_PLAY_API_BASE / ALPHA_PLAY_TOKEN_URL 可指向 mock
#     （全流程已在本地 mock 服务上实测四端点时序，见 TODO 注记）。
#
# 边界（诚实登记，详见 docs/store-publishing.md）：
#   - App Store：复用 L518（ios-release.sh），本脚本不重复造。
#   - Microsoft Store：Partner Center 只收 MSIX；L516 产物是 msi/nsis/exe，
#     MSIX 重打包落地前无提交物，登记为桌面打包形态变更后的接线项。
#   - 国内安卓渠道（华为/小米/应用宝等）：APK 再签名 + 渠道 SDK 属运营侧，
#     工程面由 L517 渠道矩阵（play/direct）支撑。
#
# 依赖：curl、python3、openssl（JWT 签名）。
set -euo pipefail
cd "$(dirname "$0")/.."

fail() { echo "❌ $*" >&2; exit 1; }
ok()   { echo "✅ $*"; }
info() { echo "ℹ️  $*"; }

CMD="${1:-}"
[ -n "$CMD" ] || { echo "用法: $0 play <aab-path> [--track internal|closed|production] [--dry-run]" >&2; exit 2; }

case "$CMD" in
  play) shift ;;
  *) fail "未知子命令: $CMD（支持: play）" ;;
esac

# AAB 路径 = 首个非 -- 参数（缺省记空，后面统一报用法）
AAB_PATH=""
if [ $# -gt 0 ] && [ "${1#--}" = "$1" ]; then
  AAB_PATH="$1"
  shift
fi
TRACK="internal"
DRY_RUN=0
while [ $# -gt 0 ]; do
  case "$1" in
    --track) shift; TRACK="${1:-}" ;;
    --dry-run) DRY_RUN=1 ;;
    *) fail "未知参数: $1" ;;
  esac
  shift
done

[ -n "$AAB_PATH" ] || fail "缺 AAB 路径（用法: $0 play <aab-path> ...）"
[ -f "$AAB_PATH" ] || fail "AAB 不存在: $AAB_PATH"
case "$TRACK" in
  internal|closed|production) ;;
  *) fail "track 只支持 internal|closed|production，得: $TRACK" ;;
esac

# 包名与 gradle 渠道面同源（L517：applicationId 各渠道一致）
PKG="$(sed -n 's/.*applicationId = "\([^"]*\)".*/\1/p' mobile/android/app/build.gradle.kts | head -1)"
[ -n "$PKG" ] || fail "无法从 mobile/android/app/build.gradle.kts 提取 applicationId"

API_BASE="${ALPHA_PLAY_API_BASE:-https://androidpublisher.googleapis.com}"
TOKEN_URL="${ALPHA_PLAY_TOKEN_URL:-https://oauth2.googleapis.com/token}"

echo "=== Play 上传计划 ==="
echo "  aab:        $AAB_PATH ($(wc -c <"$AAB_PATH") bytes)"
echo "  package:    $PKG"
echo "  track:      $TRACK"
echo "  api base:   $API_BASE"

if [ "$DRY_RUN" = 1 ]; then
  info "dry-run：将执行 JWT 换 token → POST /edits → POST /edits/{id}/bundles → PUT /edits/{id}/tracks/$TRACK → POST /edits/{id}:commit"
  exit 0
fi

SA_FILE="${ALPHA_PLAY_SERVICE_ACCOUNT_JSON:-}"
[ -n "$SA_FILE" ] || fail "缺 ALPHA_PLAY_SERVICE_ACCOUNT_JSON（Play 服务账号 JSON 路径）"
[ -f "$SA_FILE" ] || fail "服务账号 JSON 不存在: $SA_FILE"
python3 - "$SA_FILE" 2>/dev/null <<'PYEOF' || fail "服务账号 JSON 缺 service_account 必需字段（type/client_email/private_key）"
import json, sys
sa = json.load(open(sys.argv[1], encoding="utf-8"))
assert sa.get("type") == "service_account" and sa.get("client_email") and sa.get("private_key")
PYEOF

# 全流程交python编排（JWT 签名走 openssl 子进程；REST 走 urllib 标准库）
python3 - "$SA_FILE" "$AAB_PATH" "$PKG" "$TRACK" "$API_BASE" "$TOKEN_URL" <<'PYEOF'
import base64, json, subprocess, sys, tempfile, time, urllib.request, urllib.error

sa_path, aab_path, pkg, track, api_base, token_url = sys.argv[1:7]
sa = json.load(open(sa_path, encoding="utf-8"))

def b64url(raw: bytes) -> str:
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode()

def die(msg: str) -> None:
    print(f"❌ {msg}", file=sys.stderr)
    sys.exit(1)

# 1) RS256 JWT（服务账号 → access token；签进临时 0600 私钥文件）
now = int(time.time())
header = b64url(json.dumps({"alg": "RS256", "typ": "JWT"}, separators=(",", ":")).encode())
claims = b64url(json.dumps({
    "iss": sa["client_email"],
    "scope": "https://www.googleapis.com/auth/androidpublisher",
    "aud": token_url,
    "iat": now,
    "exp": now + 3600,
}, separators=(",", ":")).encode())
signing_input = f"{header}.{claims}".encode()
with tempfile.NamedTemporaryFile("w", suffix=".pem", delete=False) as kf:
    kf.write(sa["private_key"])
    key_file = kf.name
try:
    sig = subprocess.run(
        ["openssl", "dgst", "-sha256", "-sign", key_file],
        input=signing_input, capture_output=True, check=True,
    ).stdout
except subprocess.CalledProcessError as e:
    die(f"openssl 签名失败: {e.stderr.decode(errors='replace')[:200]}")
finally:
    import os
    os.unlink(key_file)
jwt = f"{header}.{claims}.{b64url(sig)}"

def http(method: str, url: str, *, data=None, headers=None, ok_codes=(200, 201)):
    req = urllib.request.Request(url, data=data, method=method, headers=headers or {})
    try:
        with urllib.request.urlopen(req, timeout=600) as resp:
            body = resp.read()
            return resp.status, (json.loads(body) if body else {})
    except urllib.error.HTTPError as e:
        detail = e.read().decode(errors="replace")[:300]
        if e.code in ok_codes:
            return e.code, {}
        die(f"{method} {url} → HTTP {e.code}: {detail}")

# 2) token 交换（JWT bearer grant）
form = f"grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer&assertion={jwt}".encode()
_, tok = http("POST", token_url, data=form,
              headers={"Content-Type": "application/x-www-form-urlencoded"})
access = tok.get("access_token")
if not access:
    die(f"token 响应缺 access_token: {tok}")

auth = {"Authorization": f"Bearer {access}"}

# 3) edit → bundle 上传 → track 指派 → commit
_, edit = http("POST", f"{api_base}/androidpublisher/v3/applications/{pkg}/edits", headers=auth)
edit_id = edit.get("id")
if not edit_id:
    die(f"edit 响应缺 id: {edit}")
print(f"  edit: {edit_id}")

aab = open(aab_path, "rb").read()
_, bundle = http(
    "POST",
    f"{api_base}/androidpublisher/v3/applications/{pkg}/edits/{edit_id}/bundles?uploadType=media",
    data=aab,
    headers={**auth, "Content-Type": "application/octet-stream"},
)
version_code = bundle.get("versionCode")
if not version_code:
    die(f"bundle 响应缺 versionCode: {bundle}")
print(f"  bundle 上传成功: versionCode={version_code} ({len(aab)} bytes)")

http("PUT",
     f"{api_base}/androidpublisher/v3/applications/{pkg}/edits/{edit_id}/tracks/{track}",
     data=json.dumps({"track": track, "releases": [{"versionCodes": [version_code]}]}).encode(),
     headers={**auth, "Content-Type": "application/json"})
print(f"  track 指派成功: {track}")

http("POST", f"{api_base}/androidpublisher/v3/applications/{pkg}/edits/{edit_id}:commit", headers=auth)
print(f"  edit 已提交：{pkg} versionCode={version_code} 进入 track={track}")
PYEOF

ok "Play 提交完成（track=$TRACK）。晋级 closed/production 在 Play Console 逐级复核后操作。"
