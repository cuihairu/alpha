#!/usr/bin/env bash
# Web dist → S3 兼容对象存储 / CDN 源发布（L515）
#
# 定位：把 web/dist（先经 optimize-dist.mjs 指纹 + 预压缩 + 缓存清单）同步到
# S3 兼容对象存储（AWS S3 / Cloudflare R2 / 阿里云 OSS S3 兼容端点均可）。
# 缓存头按 cache-manifest.json 逐文件下发；预压缩体带 Content-Encoding 上传
# （CDN/源站按 Accept-Encoding 直发，免边缘压缩）。nginx 源站形态见
# config/nginx/web-origin.conf（compose web-origin 服务），二者二选一或源站+CDN 叠加。
#
# 用法：
#   scripts/web-cdn-deploy.sh --bucket my-bucket [--endpoint https://<s3-兼容端点>] \
#     [--prefix web] [--dry-run]
# 前置：AWS CLI v2（aws）或 rclone（二选一，自动探测）；--dry-run 只打印计划。

set -euo pipefail
cd "$(dirname "$0")/.."

fail() { echo "❌ $*"; exit 1; }
ok()   { echo "✅ $*"; }
info() { echo "ℹ️  $*"; }

BUCKET="" ENDPOINT="" PREFIX="" DRY_RUN=0
while [ $# -gt 0 ]; do
  case "$1" in
    --bucket)   BUCKET="$2"; shift 2 ;;
    --endpoint) ENDPOINT="$2"; shift 2 ;;
    --prefix)   PREFIX="$2"; shift 2 ;;
    --dry-run)  DRY_RUN=1; shift ;;
    *) fail "未知参数: $1" ;;
  esac
done
[ -n "$BUCKET" ] || fail "--bucket 必填"
DIST=web/dist
[ -f "$DIST/cache-manifest.json" ] \
  || fail "缺 $DIST/cache-manifest.json——先 cd web && npm run build（含 optimize-dist）"

TARGET="$BUCKET"
[ -n "$PREFIX" ] && TARGET="$BUCKET/$PREFIX"

if command -v aws >/dev/null 2>&1; then
  TOOL=aws
elif command -v rclone >/dev/null 2>&1; then
  TOOL=rclone
elif [ "$DRY_RUN" = 1 ]; then
  TOOL=aws  # 计划模式无工具也可自测（只打印 aws 语义的计划行）
  info "未安装 aws/rclone——dry-run 以 aws 语义打印计划"
else
  fail "缺 aws CLI 或 rclone（S3 兼容上传二选一）；nginx 源站形态可走 compose up web-origin"
fi

info "发布 $DIST → $TARGET（工具=$TOOL，dry-run=$DRY_RUN）"

# python3 产两类清单：普通文件（带头）与预压缩体（带 Content-Encoding + 指定 Content-Type）
python3 - "$DIST" "$TOOL" "$TARGET" "$ENDPOINT" "$DRY_RUN" <<'PY'
import json, mimetypes, subprocess, sys

dist, tool, target, endpoint, dry = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5] == "1"
manifest = json.load(open(f"{dist}/cache-manifest.json"))

def base_args():
    a = []
    if endpoint:
        a += (["--endpoint-url", endpoint] if tool == "aws" else [])
    return a

def upload(local, remote, cache_control, encoding=None):
    ctype = mimetypes.guess_type(local)[0] or "application/octet-stream"
    if local.endswith(".wasm"):
        ctype = "application/wasm"
    ext_args = ["--content-type", ctype, "--cache-control", cache_control]
    if encoding:
        ext_args += ["--content-encoding", encoding]
    if tool == "aws":
        cmd = ["aws", "s3", "cp", f"{dist}/{local}", f"s3://{target}/{remote}", *ext_args]
        if dry:
            print("DRY", *cmd[2:])
            return
        subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL)
    else:
        # rclone: 头用 --header-upload，remote 形如 remote:bucket/key（依赖已配 remote）
        if dry:
            print(f"DRY rclone copy {dist}/{local} -> {target}/{remote} {ext_args}")
            return
        subprocess.run(
            ["rclone", "copyto", f"{dist}/{local}", f"{target}/{remote}",
             "--header-upload", f"Cache-Control: {cache_control}",
             "--header-upload", f"Content-Type: {ctype}"]
            + ([f"--header-upload", f"Content-Encoding: {encoding}"] if encoding else []),
            check=True, stdout=subprocess.DEVNULL)

n_plain = n_pre = 0
def push_entry(rel, meta):
    global n_plain, n_pre
    upload(rel, rel, meta["cache-control"])
    n_plain += 1
    for enc, suffix in (("gzip", ".gz"), ("br", ".br")):
        try:
            open(f"{dist}/{rel}{suffix}").close()
        except FileNotFoundError:
            continue
        upload(rel + suffix, rel + suffix, meta["cache-control"], encoding=enc)
        n_pre += 1

# 资产先行、HTML 入口收尾（发布原子性：入口出现时其引用的指纹资产必已就位）
for rel, meta in manifest.items():
    if not rel.endswith(".html"):
        push_entry(rel, meta)
for rel, meta in manifest.items():
    if rel.endswith(".html"):
        push_entry(rel, meta)

print(f"plain={n_plain} precompressed={n_pre}")
PY

# html 入口最后覆盖（发布原子性：先资产后入口，入口出现时资产必已就位）
ok "发布完成：入口 index.html 最后覆盖，指纹资产 immutable 长缓存"
