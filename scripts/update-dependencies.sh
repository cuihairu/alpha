#!/bin/bash

# Alpha Finance 依赖更新脚本
# 自动更新关键依赖到安全稳定版本

set -e

echo "🔄 开始更新 Alpha Finance 项目依赖..."

# 定义要更新的关键依赖
declare -a DEPS=(
    "serde@1.0.210"      # 序列化库 - 安全更新
    "tokio@1.39.3"         # 异步运行时 - 重要更新
    "axum@0.7.7"          # Web框架 - 兼容性更新
    "tracing@0.1.40"         # 日志框架 - 特性更新
    "anyhow@1.0.86"         # 错误处理 - 补丁更新
    "uuid@1.8.0"            # UUID生成 - 安全更新
    "chrono@0.4.38"          # 时间处理 - 安全更新
)

# 记录开始时间
START_TIME=$(date +%s)

echo "📦 检查当前依赖版本..."

# 显示当前版本
for dep in "${DEPS[@]}"; do
    if [[ "$dep" == *"@"* ]]; then
        pkg_name="${dep%@*}"
        current_version="${dep#*@}"
        echo "  当前: $pkg_name@$current_version"
    fi
done

echo ""
echo "⬆️ 开始更新依赖..."

# 逐个更新依赖
SUCCESS_COUNT=0
FAILED_COUNT=0

for dep in "${DEPS[@]}"; do
    if [[ "$dep" == *"@"* ]]; then
        pkg_name="${dep%@*}"

        echo -n "🔄 更新 $pkg_name..."

        # 使用 cargo update 并捕获输出
        if cargo update -p "$pkg_name" 2>&1; then
            echo "✅ $pkg_name 更新成功"
            ((SUCCESS_COUNT++))
        else
            echo "❌ $pkg_name 更新失败"
            ((FAILED_COUNT++))

            # 对于关键依赖，如果更新失败，显示详细错误信息
            if [[ "$pkg_name" == "tokio" ]] || [[ "$pkg_name" == "serde" ]]; then
                echo "⚠️  关键依赖更新失败，建议手动检查："
                cargo update -p "$pkg_name" --verbose
            fi
        fi

        # 短暂避免API限制
        sleep 2
    fi
done

# 计算更新耗时
END_TIME=$(date +%s)
DURATION=$((END_TIME - START_TIME))

echo ""
echo "📊 更新结果统计："
echo "  ✅ 成功更新: $SUCCESS_COUNT 个包"
echo "  ❌ 更新失败: $FAILED_COUNT 个包"
echo "  ⏱️ 总耗时: ${DURATION} 秒"

if [ $FAILED_COUNT -gt 0 ]; then
    echo ""
    echo "⚠️  部分依赖更新失败，建议："
    echo "  1. 手动运行: cargo update tokio --verbose"
    echo "  2. 检查网络连接"
    echo "  3. 清理 Cargo 缓存: rm -rf ~/.cargo/registry/src"
    echo "  4. 重新运行此脚本"
    exit 1
else
    echo ""
    echo "🎉 所有依赖更新完成！"
    echo "💡 建议运行 'cargo check' 验证更新结果"
fi

echo ""
echo "🔍 检查项目编译状态..."

# 验证更新结果
if cargo check --quiet; then
    echo "✅ 项目编译检查通过"
else
    echo "❌ 项目编译检查失败，请检查错误信息"
    exit 1
fi
