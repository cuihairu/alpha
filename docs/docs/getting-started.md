---
title: 环境要求
---

# 环境要求

在开始使用 Alpha Finance 之前，请确保您的开发环境满足以下要求。

## 🖥️ 硬件要求

### 最低配置
- **CPU**: 2核心 2.0GHz
- **内存**: 4GB RAM
- **存储**: 20GB 可用空间
- **网络**: 宽带互联网连接

### 推荐配置
- **CPU**: 4核心 3.0GHz 或更高
- **内存**: 16GB RAM 或更高
- **存储**: 100GB SSD
- **网络**: 稳定的千兆网络

### 生产环境
- **CPU**: 8核心 3.0GHz 或更高
- **内存**: 32GB RAM 或更高
- **存储**: 500GB NVMe SSD
- **网络**: 企业级网络连接

## 🐧 操作系统支持

### Linux (推荐)
- **Ubuntu**: 20.04 LTS 或更高版本
- **CentOS**: 7 或更高版本
- **Fedora**: 35 或更高版本
- **Debian**: 10 或更高版本

### Windows
- **Windows 10**: 版本 2004 或更高
- **Windows 11**: 所有版本
- **Windows Server**: 2019 或更高版本

### macOS
- **macOS Big Sur**: 11.0 或更高版本
- **macOS Monterey**: 12.0 或更高版本
- **macOS Ventura**: 13.0 或更高版本

## 🔧 软件依赖

### 必需工具

| 工具 | 版本要求 | 用途 |
|------|----------|------|
| **Git** | 2.25+ | 版本控制 |
| **Rust** | 1.70+ | 核心开发语言 |
| **Node.js** | 16.0+ | 前端构建工具 |
| **Docker** | 20.10+ | 容器化部署 |
| **Docker Compose** | 2.0+ | 多容器编排 |

### 可选工具

| 工具 | 版本要求 | 用途 |
|------|----------|------|
| **PostgreSQL** | 13+ | 开发测试数据库 |
| **Redis** | 6.0+ | 缓存和会话存储 |
| **Nginx** | 1.18+ | 反向代理 |
| **ClickHouse** | 22.8+ | 生产数据库 |

## 📦 详细安装指南

### Linux/macOS

```bash
# 1. 更新系统包管理器
sudo apt update && sudo apt upgrade -y  # Ubuntu/Debian
# 或
sudo yum update -y                      # CentOS/RHEL

# 2. 安装 Git
sudo apt install -y git                 # Ubuntu/Debian
# 或
sudo yum install -y git                 # CentOS/RHEL

# 3. 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# 4. 安装 Node.js
curl -fsSL https://deb.nodesource.com/setup_18.x | sudo -E bash -
sudo apt-get install -y nodejs            # Ubuntu/Debian
# 或
curl -fsSL https://rpm.nodesource.com/setup_18.x | sudo bash -
sudo yum install -y nodejs npm           # CentOS/RHEL

# 5. 安装 Docker
curl -fsSL https://get.docker.com -o get-docker.sh
sudo sh get-docker.sh

# 6. 安装 Docker Compose
sudo curl -L "https://github.com/docker/compose/releases/latest/download/docker-compose-$(uname -s)-$(uname -m)" -o /usr/local/bin/docker-compose
sudo chmod +x /usr/local/bin/docker-compose

# 7. 将用户添加到 docker 组
sudo usermod -aG docker $USER
newgrp docker
```

### Windows

使用 **WSL2** (Windows Subsystem for Linux 2) 获得最佳体验：

```powershell
# 1. 启用 WSL2
dism.exe /online /enable-feature /featurename:Microsoft-Windows-Subsystem-Linux /all /norestart
dism.exe /online /enable-feature /featurename:VirtualMachinePlatform /all /norestart

# 2. 下载并安装 WSL2 内核更新
wsl --install

# 3. 在 WSL2 Ubuntu 中安装依赖（参考 Linux 指南）
```

或者直接安装原生工具：

1. **Git**: [git-scm.com](https://git-scm.com/)
2. **Rust**: [rustup.rs](https://rustup.rs/)
3. **Node.js**: [nodejs.org](https://nodejs.org/)
4. **Docker Desktop**: [docker.com/products/docker-desktop](https://www.docker.com/products/docker-desktop)

### macOS

```bash
# 1. 安装 Homebrew
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# 2. 安装依赖
brew install git rust node docker docker-compose

# 3. 启动 Docker Desktop
open /Applications/Docker.app
```

## 🧪 环境验证

安装完成后，运行以下命令验证环境：

```bash
# 检查 Git 版本
git --version

# 检查 Rust 版本
rustc --version
cargo --version

# 检查 Node.js 版本
node --version
npm --version

# 检查 Docker 版本
docker --version
docker-compose --version

# 验证 Rust 目标
rustup target list --installed
rustup target add wasm32-unknown-unknown  # WebAssembly 目标

# 验证 Rust 组件
rustup component add clippy rustfmt
```

### 预期输出示例

```
git version 2.40.1
rustc 1.73.0 (cc66ad468 2023-10-03)
cargo 1.73.0 (cc66ad468 2023-10-03)
v18.18.0
9.8.1
Docker version 24.0.6, build ed223bc
Docker Compose version v2.21.0
```

## 🔧 IDE 推荐配置

### VS Code
推荐安装以下扩展：

1. **rust-analyzer** - Rust 语言支持
2. **Even Better TOML** - TOML 文件高亮
3. **Docker** - Docker 集成
4. **ESLint** - JavaScript/TypeScript 语法检查
5. **Prettier** - 代码格式化

### 配置文件

**`.vscode/settings.json`**:
```json
{
  "rust-analyzer.cargo.loadOutDirsFromCheck": true,
  "rust-analyzer.checkOnSave.command": "clippy",
  "rust-analyzer.procMacro.enable": true,
  "editor.formatOnSave": true,
  "editor.codeActionsOnSave": {
    "source.fixAll.eslint": true
  }
}
```

**`.vscode/extensions.json`**:
```json
{
  "recommendations": [
    "rust-lang.rust-analyzer",
    "tamasfe.even-better-toml",
    "ms-azuretools.vscode-docker",
    "dbaeumer.vscode-eslint",
    "esbenp.prettier-vscode"
  ]
}
```

### JetBrains IDEs

- **CLion**: 安装 Rust 插件
- **IntelliJ IDEA**: 安装 Rust 插件
- **RustRover**: JetBrains 官方 Rust IDE (测试版)

## 🚀 下一步

环境配置完成后，您可以：

1. [📖 查看安装指南](./installation.md)
2. [⚡ 快速启动项目](./quick-start.md)
3. [🔧 了解配置选项](./configuration.md)
4. [📊 查看 API 文档](./api/overview.md)

## ❓ 常见问题

### Q: Rust 编译太慢怎么办？
A: 可以尝试以下优化：
- 使用 `sccache` 缓存编译结果
- 安装 `lld` 链接器
- 使用更多的编译并行数

### Q: Docker 构建失败怎么办？
A: 检查以下几点：
- 确保镜像源可用
- 检查磁盘空间是否充足
- 验证 Docker 服务状态

### Q: WSL2 性能问题？
A: 优化建议：
- 将项目放在 WSL2 文件系统中
- 使用 `.wslconfig` 配置资源限制
- 考虑使用 Windows 原生 Docker

如果遇到其他问题，请查看 [故障排除指南](./troubleshooting.md) 或在 [GitHub Issues](https://github.com/cuihairu/alpha/issues) 中反馈。