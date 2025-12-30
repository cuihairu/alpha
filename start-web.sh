#!/bin/bash

# Alpha Finance Web 应用启动脚本

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WEB_DIR="$ROOT_DIR/web"
WASM_DIR="$ROOT_DIR/wasm-analyzer"
HOST="${HOST:-127.0.0.1}"
PORT="${PORT:-8080}"

echo "🚀 启动 Alpha Finance Web 应用..."

# 检查 Node.js 是否安装
if ! command -v node &> /dev/null; then
    echo "❌ Node.js 未安装，请先安装 Node.js"
    echo "📥 下载地址: https://nodejs.org/"
    exit 1
fi

echo "✅ Node.js 已安装: $(node --version)"

cd "$WEB_DIR"

# 检查是否需要构建 WASM
if [ ! -d "pkg" ] || [ ! -f "pkg/alpha_wasm_analyzer_bg.wasm" ]; then
    echo "📦 WASM 模块未找到，开始构建..."

    # 检查 wasm-pack 是否安装
    if ! command -v wasm-pack &> /dev/null; then
        echo "🔧 安装 wasm-pack..."
        curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
        # shellcheck disable=SC1090
        source "$HOME/.cargo/env" || true
    fi

    # 构建 WASM
    echo "🔨 构建 WASM 模块..."
    cd "$WASM_DIR"
    wasm-pack build --target web --out-dir "$WEB_DIR/pkg"
    cd "$WEB_DIR"

    if [ $? -eq 0 ]; then
        echo "✅ WASM 模块构建成功"
    else
        echo "❌ WASM 模块构建失败"
        exit 1
    fi
else
    echo "✅ WASM 模块已存在"
fi

# 启动 Web 服务器
echo "🌐 启动 Web 服务器..."
echo "📍 访问地址: http://localhost:${PORT}"
if [ "$HOST" = "0.0.0.0" ]; then
    echo "📍 局域网访问: http://<你的IP>:${PORT}"
else
    echo "📍 如需局域网访问: HOST=0.0.0.0 PORT=${PORT} ./start-web.sh"
fi
echo ""
echo "📝 功能说明:"
echo "   • 股票技术分析 (RSI, MACD, 移动平均线等)"
echo "   • 实时价格监控"
echo "   • 智能推荐系统"
echo "   • 高性能 WebAssembly 计算引擎"
echo ""
echo "⏹️  按 Ctrl+C 停止服务器"
echo ""

# 启动服务器
HOST="$HOST" PORT="$PORT" node server.js
