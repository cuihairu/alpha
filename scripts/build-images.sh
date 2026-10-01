#!/usr/bin/env bash
# 跨平台 Docker 镜像构建（TODO L468，方案文档 docs/docker-deployment.md）
#
# 用法：
#   scripts/build-images.sh                          # 发现到的全部服务，当前平台本地构建
#   scripts/build-images.sh --only api-gateway       # 只构建单个服务（快速验证）
#   scripts/build-images.sh --platforms linux/amd64,linux/arm64 --push --registry ghcr.io/alpha
#                                                    # 多平台构建并推送（需 buildx）
#
# 约定：
#   - 服务清单自动发现 = services/*/Dockerfile（新增服务无需改本脚本）
#   - 镜像名 = [<registry>/]alpha-<服务短名>，tag = $TAG 或 git 短 SHA（无 git 元数据时 local）
#   - 无 buildx 时多平台请求直接失败并提示（不静默降级，避免推送了单架构镜像冒充多架构）
set -euo pipefail

cd "$(dirname "$0")/.."

ONLY=""
PLATFORMS=""
PUSH=0
REGISTRY="${REGISTRY:-}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --only) ONLY="${2:?--only 缺少服务名}"; shift 2 ;;
    --platforms) PLATFORMS="${2:?--platforms 缺少平台列表}"; shift 2 ;;
    --push) PUSH=1; shift ;;
    --registry) REGISTRY="${2:?--registry 缺少前缀}"; shift 2 ;;
    -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
    *) echo "未知参数: $1（--help 查看用法）" >&2; exit 2 ;;
  esac
done

TAG="${TAG:-$(git rev-parse --short HEAD 2>/dev/null || echo local)}"

# 发现服务 Dockerfile（跳过隐藏目录；alert-webhook 等并行新增服务存在即纳入）
mapfile -t DOCKERFILES < <(find services -mindepth 2 -maxdepth 2 -name Dockerfile | sort)
if [[ ${#DOCKERFILES[@]} -eq 0 ]]; then
  echo "未发现 services/*/Dockerfile" >&2
  exit 1
fi

if [[ -n "$ONLY" ]]; then
  filtered=()
  for f in "${DOCKERFILES[@]}"; do
    [[ "$(basename "$(dirname "$f")")" == "$ONLY" ]] && filtered+=("$f")
  done
  if [[ ${#filtered[@]} -eq 0 ]]; then
    echo "服务 $ONLY 无 Dockerfile（可用：$(printf '%s ' "${DOCKERFILES[@]}")）" >&2
    exit 1
  fi
  DOCKERFILES=("${filtered[@]}")
fi

# 多平台/推送必须走 buildx（经典 docker build 只出当前架构）
BUILDX=0
if docker buildx version >/dev/null 2>&1; then
  BUILDX=1
fi
if [[ -n "$PLATFORMS" || $PUSH -eq 1 ]] && [[ $BUILDX -eq 0 ]]; then
  echo "多平台/--push 需要 docker buildx 插件（apt: docker-buildx-plugin / docker buildx install）" >&2
  exit 1
fi
if [[ $PUSH -eq 1 && -z "$REGISTRY" ]]; then
  echo "--push 需要 --registry（或 REGISTRY 环境变量）指定仓库前缀" >&2
  exit 1
fi

failed=()
for f in "${DOCKERFILES[@]}"; do
  svc_dir="$(basename "$(dirname "$f")")"
  # api-gateway → alpha-api-gateway（与 compose 内建镜像命名一致的服务短名）
  name="alpha-${svc_dir}"
  ref="$name:$TAG"
  [[ -n "$REGISTRY" ]] && ref="${REGISTRY%/}/$ref"

  args=(build -f "$f" -t "$ref")
  if [[ $BUILDX -eq 1 ]]; then
    args=(buildx build -f "$f" -t "$ref")
    [[ -n "$PLATFORMS" ]] && args+=(--platform "$PLATFORMS")
    [[ $PUSH -eq 1 ]] && args+=(--push) || args+=(--load)
  fi

  echo "==> 构建 $ref${PLATFORMS:+（platforms=$PLATFORMS）}"
  if ! docker "${args[@]}" .; then
    failed+=("$svc_dir")
  fi
done

if [[ ${#failed[@]} -gt 0 ]]; then
  echo "构建失败: ${failed[*]}" >&2
  exit 1
fi
echo "✅ 全部镜像构建完成（tag=$TAG）"
