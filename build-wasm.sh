#!/bin/bash

# Alpha Finance WASM 构建脚本

echo "🚀 开始构建 Alpha Finance WASM 模块..."

# 检查 wasm-pack 是否安装
if ! command -v wasm-pack &> /dev/null; then
    echo "❌ wasm-pack 未安装，正在安装..."
    curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
    source ~/.cargo/env
else
    echo "✅ wasm-pack 已安装"
fi

# 进入 WASM 目录
cd wasm-analyzer

# 构建 WASM 模块
echo "📦 构建 WASM 模块..."
wasm-pack build --target web --out-dir ../web/pkg

if [ $? -eq 0 ]; then
    echo "✅ WASM 模块构建成功"
else
    echo "❌ WASM 模块构建失败"
    exit 1
fi

# 返回根目录
cd ..

echo "🌐 启动开发服务器..."
echo "📍 访问地址: http://localhost:8080"
cd web && python3 -m http.server 8080