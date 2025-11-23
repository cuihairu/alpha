# Alpha Finance Web 前端

基于 Rust WebAssembly 的高性能金融数据分析平台 Web 前端。

## 🚀 快速开始

### 1. 安装依赖

#### 安装 Rust (如果还没有)
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

#### 安装 wasm-pack
```bash
curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
```

### 2. 构建 WASM 模块

```bash
# 方法1: 使用自动构建脚本
../build-wasm.sh

# 方法2: 手动构建
cd ../wasm-analyzer
wasm-pack build --target web --out-dir ../web/pkg
```

### 3. 启动 Web 服务器

```bash
# 方法1: 使用 Node.js 服务器
npm start

# 方法2: 使用 Python 服务器 (如果系统有 Python 3)
python3 -m http.server 8080

# 方法3: 直接运行构建脚本 (包含服务器启动)
../build-wasm.sh
```

### 4. 访问应用

打开浏览器访问: http://localhost:8080

## 📊 功能特性

### 🔧 核心分析引擎
- **高性能 WASM 引擎**: 基于 Rust 开发，在浏览器中提供桌面级性能
- **实时技术指标计算**: RSI, MACD, 布林带, 移动平均线等
- **股票分析**: 智能推荐系统，基于多种技术指标

### 📈 技术指标
- **RSI (相对强弱指标)**: 判断超买超卖状态
- **MACD (异同移动平均线)**: 趋势和动量分析
- **SMA/EMA (移动平均线)**: 趋势识别
- **布林带**: 价格波动范围分析

### ⚡ 实时功能
- **实时价格监控**: 多股票价格跟踪
- **自动刷新**: 每5秒更新数据
- **性能监控**: 实时性能指标展示

### 🎨 用户界面
- **现代化设计**: 响应式布局，支持移动设备
- **直观操作**: 简单易用的界面
- **实时反馈**: 加载状态和结果展示

## 🏗️ 技术架构

### 前端技术栈
- **HTML5/CSS3**: 现代化 Web 标准
- **JavaScript ES6+**: 模块化开发
- **WebAssembly**: Rust 编译的高性能引擎
- **Canvas API**: 图表绘制

### 后端引擎
- **Rust**: 高性能系统编程语言
- **WebAssembly**: 浏览器原生性能
- **Serde**: 高效序列化/反序列化

## 📁 项目结构

```
web/
├── index.html          # 主页面
├── app.js             # 主要应用逻辑
├── server.js          # Node.js 开发服务器
├── package.json       # 项目依赖配置
├── README.md          # 说明文档
└── pkg/               # WASM 构建输出
    ├── alpha_wasm_analyzer.js    # WASM JavaScript 绑定
    ├── alpha_wasm_analyzer_bg.wasm # WASM 二进制文件
    └── ...                        # 其他构建文件
```

## 🔧 开发指南

### 添加新的技术指标

1. 在 `../packages/core/src/indicators.rs` 中实现指标算法
2. 在 `../wasm-analyzer/src/lib.rs` 中添加 WASM 绑定
3. 在 `app.js` 中添加前端调用逻辑
4. 重新构建 WASM 模块

### 自定义样式

编辑 `index.html` 中的 CSS 样式，或创建单独的 CSS 文件。

### 添加新页面

1. 创建新的 HTML 文件
2. 在 `app.js` 中添加对应的 JavaScript 逻辑
3. 更新导航链接

## 🧪 测试

### 功能测试
- 股票分析功能
- 技术指标计算
- 实时数据监控
- 性能指标展示

### 浏览器兼容性
- Chrome 80+
- Firefox 75+
- Safari 13+
- Edge 80+

## 📈 性能优化

### WASM 优化
- 使用 `wasm-opt` 进行代码优化
- 启用 SIMD 指令集支持
- 减少内存分配

### 前端优化
- 代码分割和懒加载
- 图片和资源优化
- 缓存策略

## 🐛 故障排除

### 常见问题

1. **WASM 模块加载失败**
   ```bash
   # 重新构建 WASM
   npm run build:wasm
   ```

2. **服务器启动失败**
   ```bash
   # 检查端口是否被占用
   lsof -ti:8080 | xargs kill -9
   ```

3. **浏览器控制台错误**
   - 确保启用了 WebAssembly 支持
   - 检查 CORS 设置
   - 清除浏览器缓存

### 调试技巧
- 打开浏览器开发者工具查看控制台日志
- 使用 Network 面板检查资源加载
- 使用 Performance 面板分析性能

## 🤝 贡献指南

1. Fork 项目
2. 创建功能分支: `git checkout -b feature/amazing-feature`
3. 提交更改: `git commit -m 'Add amazing feature'`
4. 推送分支: `git push origin feature/amazing-feature`
5. 创建 Pull Request

## 📄 许可证

本项目采用 MIT 许可证 - 查看 [LICENSE](../LICENSE) 文件了解详情。

## 🙏 致谢

- [Rust](https://rust-lang.org/) - 系统编程语言
- [WebAssembly](https://webassembly.org/) - 高性能 Web 标准
- [wasm-pack](https://rustwasm.github.io/wasm-pack/) - Rust WebAssembly 工具链

---

⭐ 如果这个项目对你有帮助，请给我们一个 Star！