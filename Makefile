# Alpha Finance 跨平台构建脚本

.PHONY: help build test clean lint format clippy check

# 默认目标
help:
	@echo "Alpha Finance 跨平台构建系统"
	@echo ""
	@echo "可用命令:"
	@echo "  help         - 显示帮助信息"
	@echo "  build        - 构建所有项目"
	@echo "  test         - 运行所有测试"
	@echo "  clean        - 清理构建文件"
	@echo "  lint         - 代码风格检查"
	@echo "  format       - 代码格式化"
	@echo "  clippy       - Clippy 静态分析"
	@echo "  check        - 完整代码检查"
	@echo ""
	@echo "平台特定命令:"
	@echo "  build-web    - 构建 Web WASM"
	@echo "  build-desktop - 构建桌面应用"
	@echo "  build-mobile - 构建移动应用"
	@echo ""
	@echo "开发命令:"
	@echo "  dev-web      - 启动 Web 开发服务器"
	@echo "  dev-desktop  - 启动桌面开发环境"

# 构建所有项目
build:
	@echo "🚀 开始构建所有项目..."
	cargo build --release --workspace
	$(MAKE) build-web
	$(MAKE) build-desktop
	@echo "✅ 所有项目构建完成"

# Web 端构建
build-web:
	@echo "🌐 构建 Web WASM..."
	cd wasm-analyzer && wasm-pack build --target web --out-dir pkg --release
	@echo "✅ Web WASM 构建完成"

# 桌面端构建
build-desktop:
	@echo "🖥️ 构建桌面应用..."
	cd desktop && cargo build --release
	@echo "✅ 桌面应用构建完成"

# 移动端构建
build-mobile:
	@echo "📱 构建移动应用..."
	@echo "Android 构建:"
	cd mobile/android && cargo ndk --target arm64-v8a build --release
	@echo "iOS 构建:"
	cd mobile/ios && cargo build --target aarch64-apple-ios --release
	@echo "✅ 移动应用构建完成"

# 运行所有测试
test:
	@echo "🧪 运行所有测试..."
	cargo test --workspace --all-features
	@echo "✅ 所有测试通过"

# 代码格式化
format:
	@echo "🎨 格式化代码..."
	cargo fmt --all
	@echo "✅ 代码格式化完成"

# Clippy 静态分析
clippy:
	@echo "🔍 运行 Clippy 检查..."
	cargo clippy --workspace --all-features -- -D warnings
	@echo "✅ Clippy 检查完成"

# 完整代码检查
check: format clippy test
	@echo "✅ 完整代码检查通过"

# 清理构建文件
clean:
	@echo "🧹 清理构建文件..."
	cargo clean --workspace
	rm -rf wasm-analyzer/pkg
	rm -rf desktop/target
	rm -rf mobile/target
	@echo "✅ 清理完成"

# Web 开发环境
dev-web:
	@echo "🌐 启动 Web 开发环境..."
	cd wasm-analyzer && wasm-pack build --target web --out-dir pkg --dev
	@echo "Web WASM 构建完成，请在浏览器中打开 index.html"

# 桌面开发环境
dev-desktop:
	@echo "🖥️ 启动桌面开发环境..."
	cd desktop && cargo tauri dev

# 服务端开发环境
dev-services:
	@echo "🔧 启动后端服务..."
	cargo run --bin alpha-api-gateway
	# 在其他终端中运行:
	# cargo run --bin alpha-data-engine
	# cargo run --bin alpha-real-time-feed

# 性能测试
benchmark:
	@echo "📊 运行性能测试..."
	cd tools/benchmark && cargo run --release

# 安全检查
security-audit:
	@echo "🔒 运行安全审计..."
	cargo audit
	@echo "✅ 安全审计完成"

# 依赖检查
deps-check:
	@echo "📦 检查依赖..."
	cargo tree --duplicate
	cargo outdated
	@echo "✅ 依赖检查完成"

# 文档生成
docs:
	@echo "📚 生成文档..."
	cargo doc --workspace --no-deps --open
	@echo "✅ 文档生成完成"

# 发布准备
release-prep: check security-audit deps-check
	@echo "🚀 准备发布..."
	@echo "✅ 发布准备完成"

# Docker 构建
docker-build:
	@echo "🐳 构建 Docker 镜像..."
	docker-compose -f infrastructure/docker-compose.yml build
	@echo "✅ Docker 镜像构建完成"

# 安装依赖
install-deps:
	@echo "📦 安装依赖..."
	rustup target add wasm32-unknown-unknown
	rustup target add aarch64-apple-ios
	rustup target add arm64-v8a
	cargo install wasm-pack
	cargo install tauri-cli
	cargo install cargo-audit
	cargo install cargo-outdated
	@echo "✅ 依赖安装完成"