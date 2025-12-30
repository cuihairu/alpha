#!/bin/bash

# Alpha Finance WASM 构建脚本

set -euo pipefail

usage() {
    cat <<'EOF'
Usage:
  ./build-wasm.sh [--serve]

Options:
  --serve    构建完成后启动静态服务器（Python）

Env:
  HOST       监听地址（默认 127.0.0.1）
  PORT       监听端口（默认 8080）
EOF
}

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WEB_DIR="$ROOT_DIR/web"
WASM_DIR="$ROOT_DIR/wasm-analyzer"

SERVE=0
for arg in "$@"; do
    case "$arg" in
        --serve) SERVE=1 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown arg: $arg"; usage; exit 2 ;;
    esac
done

echo "🚀 开始构建 Alpha Finance WASM 模块..."

# 检查 wasm-pack 是否安装
if ! command -v wasm-pack &> /dev/null; then
    echo "❌ wasm-pack 未安装，正在安装..."
    curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
    # shellcheck disable=SC1090
    source "$HOME/.cargo/env" || true
else
    echo "✅ wasm-pack 已安装"
fi

# 进入 WASM 目录
cd "$WASM_DIR"

# 构建 WASM 模块
echo "📦 构建 WASM 模块..."
wasm-pack build --target web --out-dir "$WEB_DIR/pkg"

if [ $? -eq 0 ]; then
    echo "✅ WASM 模块构建成功"
else
    echo "❌ WASM 模块构建失败"
    exit 1
fi

cd "$ROOT_DIR"

PORT="${PORT:-8080}"
HOST="${HOST:-127.0.0.1}"
echo "✅ 构建完成: $WEB_DIR/pkg"

if [ "$SERVE" -eq 1 ]; then
    echo "🌐 启动静态服务器..."
    echo "📍 访问地址: http://localhost:${PORT}"
    echo "📍 如需局域网访问: HOST=0.0.0.0 PORT=${PORT} ./build-wasm.sh --serve"
    cd "$WEB_DIR" && python3 -m http.server "${PORT}" --bind "${HOST}"
else
    echo "📝 启动方式:"
    echo "  - 开发（Node 静态服务器）: ./start-web.sh"
    echo "  - 或 Python 静态服务器: cd web && python3 -m http.server ${PORT} --bind ${HOST}"
    echo "  - 或直接构建并启动: ./build-wasm.sh --serve"
fi
