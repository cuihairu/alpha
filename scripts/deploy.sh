#!/bin/bash

# Alpha Finance 一键部署脚本
# 用于快速部署 Alpha Finance 金融数据平台到生产环境

set -e

# 颜色输出
RED='\033[0m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# 日志函数
log_error() {
    echo -e "${RED}[ERROR]${NC} $1${NC}"
}

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1${NC}"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1${NC}"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1${NC}"
}

# 检查系统要求
check_requirements() {
    log_info "检查系统要求..."

    # 检查 Docker
    if ! command -v docker >/dev/null 2>&1; then
        log_error "Docker 未安装，请先安装 Docker"
        exit 1
    fi

    # 检查 Docker Compose
    if ! command -v docker-compose >/dev/null 2>&1; then
        log_error "Docker Compose 未安装，请先安装 Docker Compose"
        exit 1
    fi

    # 检查 Node.js (前端需要)
    if ! command -v node >/dev/null 2>&1; then
        log_warn "Node.js 未安装，前端功能可能受限"
    else
        log_success "Node.js 已安装: $(node --version)"
    fi

    # 检查 Rust
    if ! command -v cargo >/dev/null 2>&1; then
        log_error "Rust 未安装，请先安装 Rust"
        exit 1
    fi

    # 检查系统资源
    check_system_resources

    log_success "系统要求检查完成"
}

# 检查系统资源
check_system_resources() {
    log_info "检查系统资源..."

    # 检查内存（Linux: free -m）
    if command -v free >/dev/null 2>&1; then
        AVAILABLE_MEM_MB=$(free -m | awk '/MemAvailable/{print $2}')
        if [ "${AVAILABLE_MEM_MB:-0}" -lt 4096 ]; then
            log_warn "可用内存不足 (推荐至少 4GB): ${AVAILABLE_MEM_MB}MB"
        else
            log_success "内存检查通过: ${AVAILABLE_MEM_MB}MB"
        fi
    else
        log_warn "未找到 free 命令，跳过内存检查"
    fi

    # 检查磁盘空间（df -k）
    DISK_AVAILABLE_KB=$(df -k / | awk 'NR==2 {print $4}')
    if [ "${DISK_AVAILABLE_KB:-0}" -lt $((20 * 1024 * 1024)) ]; then
        log_warn "可用磁盘空间不足 (推荐至少 20GB): ${DISK_AVAILABLE_KB}KB"
    else
        log_success "磁盘空间检查通过: ${DISK_AVAILABLE_KB}KB"
    fi

    # 检查 CPU
    if command -v nproc >/dev/null 2>&1; then
        CPU_CORES=$(nproc)
    else
        CPU_CORES=$(sysctl -n hw.ncpu 2>/dev/null || echo 1)
    fi

    if [ "$CPU_CORES" -lt 4 ]; then
        log_warn "CPU 核心数不足 (推荐至少 4 核心): ${CPU_CORES}"
    else
        log_success "CPU 检查通过: ${CPU_CORES} 核心"
    fi
}

# 配置环境
setup_environment() {
    log_info "配置环境变量..."

    # 检查 .env 文件
    if [ ! -f .env ]; then
        log_warn ".env 文件不存在，从示例文件复制"
        cp .env.example .env
        log_info "已创建 .env 文件，请根据实际环境修改配置"
    else
        log_info ".env 文件已存在"
    fi

    # 创建必要的目录
    mkdir -p logs
    mkdir -p data/postgres
    mkdir -p data/redis
    mkdir -p config

    log_success "环境配置完成"
}

# 构建项目
build_project() {
    log_info "构建 Alpha Finance 项目..."

    # 构建所有服务
    if ! cargo build --release; then
        log_error "项目构建失败"
        exit 1
    fi

    log_success "项目构建完成"
}

# 启动服务
start_services() {
    log_info "启动 Alpha Finance 服务..."

    # 使用 Docker Compose 启动所有服务
    if command -v docker-compose >/dev/null 2>&1; then
        docker-compose -f docker-compose.prod.yml up -d
        log_success "服务启动完成 (Docker Compose)"
    else
        log_error "Docker Compose 未找到，请使用单独命令启动服务"

        # 逐个启动服务
        log_info "启动 Collector Service..."
        cargo run --release --bin alpha-collector &

        log_info "启动 Data Engine Service..."
        cargo run --release --bin alpha-data-engine &

        log_info "启动 Real-time Feed Service..."
        cargo run --release --bin alpha-real-time-feed &

        log_info "启动 API Gateway..."
        cargo run --release --bin alpha-api-gateway &

        log_success "所有服务启动完成"
    fi
}

# 停止服务
stop_services() {
    log_info "停止 Alpha Finance 服务..."

    if command -v docker-compose >/dev/null 2>&1; then
        docker-compose -f docker-compose.prod.yml down
        log_success "服务停止完成 (Docker Compose)"
    else
        log_info "手动停止各个服务进程..."

        # 停止服务进程
        pkill -f "alpha-collector"
        pkill -f "alpha-data-engine"
        pkill -f "alpha-real-time-feed"
        pkill -f "alpha-api-gateway"

        log_success "服务停止完成"
    fi
}

# 重启服务
restart_services() {
    log_info "重启 Alpha Finance 服务..."

    stop_services
    sleep 5
    start_services
}

# 检查服务状态
check_services() {
    log_info "检查服务状态..."

    # 检查端口占用
    COLLECTOR_PORT=8080
    DATA_ENGINE_PORT=8081
    REAL_TIME_FEED_PORT=8082
    API_GATEWAY_PORT=8083

    # 检查端口
    check_port $COLLECTOR_PORT "Collector Service"
    check_port $DATA_ENGINE_PORT "Data Engine Service"
    check_port $REAL_TIME_FEED_PORT "Real-time Feed Service"
    check_port $API_GATEWAY_PORT "API Gateway Service"
}

# 检查端口函数
check_port() {
    local port=$1
    local service_name=$2

    if lsof -Pi :"$port" -sTCP:LISTEN -t >/dev/null 2>&1; then
        log_success "$service_name: 正在监听端口 $port"
    else
        log_warn "$service_name: 未监听端口 $port"
    fi
}

# 显示帮助
show_help() {
    echo "Alpha Finance 部署脚本"
    echo ""
    echo "用法: $0 {选项}"
    echo ""
    echo "选项:"
    echo "  check     检查系统要求"
    echo "  setup     配置环境变量"
    echo "  build     构建项目"
    echo "  start     启动所有服务"
    echo "  stop      停止所有服务"
    echo "  restart    重启所有服务"
    echo "  status     检查服务状态"
    echo "  help      显示帮助信息"
    echo ""
    echo "示例:"
    echo "  $0 setup     # 配置环境"
    echo "  $0 build     # 构建并启动服务"
    echo "  $0 start     # 启动服务"
    echo "  $0 status     # 检查服务状态"
    echo ""
}

# 主程序
main() {
    case "${1:-}" in
        "check")
            check_requirements
            ;;
        "setup")
            setup_environment
            ;;
        "build")
            setup_environment
            build_project
            start_services
            ;;
        "start")
            setup_environment
            start_services
            ;;
        "stop")
            stop_services
            ;;
        "restart")
            restart_services
            ;;
        "status")
            check_services
            ;;
        "help"|*)
            show_help
            ;;
        *)
            log_error "未知选项: $1"
            show_help
            exit 1
            ;;
    esac
}

# 运行主程序
main "$@"
