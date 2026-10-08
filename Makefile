# Alpha Finance 跨平台构建脚本
#
# 口径对齐：
# - Rust 三关与 CI/本地门禁一致：fmt --check + clippy + test 均 --exclude alpha-desktop
#   （GUI 层归 CI macOS 作业；Linux 本机缺 webkit2gtk-4.0/libsoup-2.4 编不过）
# - WASM 产物统一落 web/pkg（build-wasm.sh 与 web/package.json build:wasm 同口径），
#   web/package.json 的 build:prod 将其拷入 web/dist 供 compose web-origin / CDN 消费
# - 移动端/桌面端构建走 scripts/ 专用脚本（依赖自检、双 ABI、签名面都在脚本里）

.PHONY: help build test clean lint format clippy check build-web build-desktop \
        build-mobile dev-web dev-desktop dev-services benchmark security-audit \
        deps-check docs release-prep docker-build install-deps

# 默认目标
help:
	@echo "Alpha Finance 跨平台构建系统"
	@echo ""
	@echo "可用命令:"
	@echo "  help         - 显示帮助信息"
	@echo "  build        - 构建所有项目（Rust workspace + WASM；不含桌面壳，见 build-desktop）"
	@echo "  test         - 运行所有测试（--exclude alpha-desktop，与 CI 同口径）"
	@echo "  clean        - 清理构建产物（不碰入库的 web/dist 兜底壳）"
	@echo "  lint         - cargo fmt --all -- --check"
	@echo "  format       - cargo fmt --all"
	@echo "  clippy       - Clippy 静态分析（--exclude alpha-desktop）"
	@echo "  check        - 完整门禁三关（lint + clippy + test）"
	@echo ""
	@echo "平台特定命令:"
	@echo "  build-web    - 构建 Web WASM（wasm-analyzer -> web/pkg）"
	@echo "  build-desktop - 桌面应用打包（scripts/desktop-release.sh，依赖自检）"
	@echo "  build-mobile - 移动端构建（scripts/android-release.sh + scripts/ios-release.sh）"
	@echo ""
	@echo "开发命令:"
	@echo "  dev-web      - 构建 WASM 后启动 React 前端 Vite 开发服务器（web/app）"
	@echo "  dev-desktop  - 桌面开发环境（cargo tauri dev）"
	@echo "  dev-services - 启动后端服务（另开终端跑注释里的其余服务）"

# 构建所有项目（Rust workspace 排除 alpha-desktop，同 CI；WASM 产物落 web/pkg）
build:
	@echo "🚀 开始构建所有项目..."
	cargo build --release --workspace --exclude alpha-desktop
	$(MAKE) build-web
	@echo "✅ 所有项目构建完成（桌面打包另行 make build-desktop）"

# Web 端构建（out-dir 对齐 build-wasm.sh：产物落 ../web/pkg，构建链从这里拷入 dist）
build-web:
	@echo "🌐 构建 Web WASM..."
	cd wasm-analyzer && wasm-pack build --target web --out-dir ../web/pkg --release
	@echo "✅ Web WASM 构建完成（web/pkg）"

# 桌面端构建（转发专用脚本：宿主 OS→bundle 映射、webkit 依赖自检、AppImage 无 FUSE 兜底）
build-desktop:
	@echo "🖥️ 构建桌面应用..."
	./scripts/desktop-release.sh
	@echo "✅ 桌面应用构建完成"

# 移动端构建（转发专用脚本：Android 双 ABI + 签名面；iOS 非 macOS 宿主自动跳过）
build-mobile:
	@echo "📱 Android 构建..."
	./scripts/android-release.sh
	@echo "📱 iOS 构建..."
	./scripts/ios-release.sh
	@echo "✅ 移动应用构建完成"

# 运行所有测试（与 CI test 作业与本地门禁同口径：排除 alpha-desktop）
test:
	@echo "🧪 运行所有测试..."
	cargo test --workspace --all-targets --exclude alpha-desktop
	@echo "✅ 所有测试通过"

# 代码格式化
format:
	@echo "🎨 格式化代码..."
	cargo fmt --all
	@echo "✅ 代码格式化完成"

# 代码风格检查（fmt --check，门禁第一关）
lint:
	cargo fmt --all -- --check
	@echo "✅ 格式检查通过"

# Clippy 静态分析（--exclude alpha-desktop，同 CI）
clippy:
	@echo "🔍 运行 Clippy 检查..."
	cargo clippy --workspace --all-targets --exclude alpha-desktop -- -D warnings
	@echo "✅ Clippy 检查通过"

# 完整门禁三关（commit 前本地必过）
check: lint clippy test
	@echo "✅ 完整门禁三关通过"

# 清理构建产物（web/dist 不清：入库的桌面兜底壳 desktop-shell.js/index.html 在其中，
# 构建覆盖属预期、清掉需 git checkout 恢复）
clean:
	@echo "🧹 清理构建文件..."
	cargo clean --workspace
	rm -rf web/pkg wasm-analyzer/pkg web/app/dist
	@echo "✅ 清理完成"

# Web 开发环境（React 前端开发服务器；WASM 需先构建一次）
dev-web: build-web
	@echo "🌐 启动 React 前端开发服务器（web/app）..."
	cd web/app && npm install && npm run dev

# 桌面开发环境（Linux 需 webkit2gtk-4.0/libsoup-2.4 系统库，缺则归 CI macOS）
dev-desktop:
	@echo "🖥️ 启动桌面开发环境..."
	cargo tauri dev

# 服务端开发环境
dev-services:
	@echo "🔧 启动后端服务..."
	cargo run --bin alpha-api-gateway
	# 在其他终端中运行:
	# cargo run --bin alpha-data-engine
	# cargo run --bin alpha-real-time-feed
	# cargo run --bin alpha-collector

# 性能基准（criterion 基线比对，转发 check-perf.sh；机器须空闲，见脚本内登记）
benchmark:
	@echo "📊 运行性能基准..."
	./scripts/check-perf.sh

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

# Docker 构建（compose v2 语法，与 docker-compose.yml 服务集一致）
docker-build:
	@echo "🐳 构建 Docker 镜像..."
	docker compose build
	@echo "✅ Docker 镜像构建完成"

# 安装依赖（rust target 名为 ABI 全称：aarch64/arm64-v8a 真机 + x86_64 模拟器）
install-deps:
	@echo "📦 安装依赖..."
	rustup target add wasm32-unknown-unknown
	rustup target add aarch64-apple-ios
	rustup target add aarch64-linux-android x86_64-linux-android
	cargo install wasm-pack
	cargo install tauri-cli
	cargo install cargo-audit
	cargo install cargo-outdated
	@echo "✅ 依赖安装完成"
