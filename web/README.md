# Alpha Finance Web 前端

Rust WebAssembly 分析引擎驱动的行情前端：页面本体是原生 JS
（`index.html`/`app.js`），引擎侧由 Rust 经 wasm-pack 编译进 `pkg/`
（wasm-analyzer），SQL 工作台走 DuckDB-WASM。底座：Rust、wasm-pack、
DuckDB-WASM，皆为开源组件，本仓做接线与页面实现。

## 快速开始

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
python3 -m http.server 8080 --bind 127.0.0.1

# 方法3: 构建并启动 Python 静态服务器
../build-wasm.sh --serve
```

### 4. 访问应用

打开浏览器访问: http://localhost:8080

如需局域网访问（监听所有网卡）：
```bash
HOST=0.0.0.0 PORT=8080 npm start
```

## React 骨架（`app/`，TODO L427）

跨平台 UI 框架选型已定 **React 18 + TypeScript + Vite**（对比、理由与
Desktop/Mobile 边界见 `../docs/web-framework-selection.md`）。新组件化界面
在 `app/` 独立 npm 工程开发；本目录既有 vanilla 演示页**零改动**继续可用：

```bash
cd app
npm ci          # 依赖安装（lockfile 锁定）
npm run dev     # 骨架示例页（http://localhost:5173）
npm test        # vitest 单测
npm run build   # 类型检查 + 生产构建
```

门禁 `scripts/check-web.sh`（旧演示页在场冒烟 + `npm ci` + tsc + vitest +
`vite build`）已接入 CI `wasm` 作业。示例页内置 WASM 引擎探针：先
`npm run build:wasm`（本目录）再把 `pkg/` 拷入 `app/public/pkg/` 即可加载。

## 功能特性

- 技术指标在浏览器内计算：RSI、MACD、SMA/EMA、布林带，算法本体是
  `wasm-analyzer` 编译的 Rust 代码，与 `packages/core` 同源口径。
- 股票分析页按多指标综合打分排序，指标口径见指标卡片说明。
- 实时行情经 WebSocket 订阅 real-time-feed（需启动该服务并配置 WS 地址；
  后端不可达回退演示数据并在状态栏注明，断线出复位按钮）。
- 性能面板展示帧内计算耗时；布局响应式，窄屏可用。

## 技术架构

页面：原生 JS（ES6+ 模块）+ Canvas 图表，无打包器。
引擎：Rust → WebAssembly（`wasm-pack build --target web`），
serde 负责两侧数据契约。
构建产物 `dist/` 经 `optimize-dist.mjs` 做指纹重命名与 gz/br 预压缩
（桌面 distDir 与 web-origin 共用）。

## 项目结构

```
web/
├── app/                # React 骨架（L427，独立 npm 工程）
├── index.html          # 主页面
├── app.js             # 主要应用逻辑
├── style.css          # 全局样式（index.html <link> 引入，check-web 守门文件）
├── demo-data.js       # 演示数据（回退数据源，check-web 守门文件）
├── wasm-demo.html     # WASM 演示页
├── duckdb-ui.mjs      # DuckDB-WASM 读 parquet 的 legacy SQL 演示
├── server.js          # Node.js 开发服务器
├── scripts/           # 前端自检脚本
├── vendor/            # 第三方静态资源
├── dist/              # 构建产物（桌面 distDir 与 web-origin 共用）
├── package.json       # 项目依赖（唯一运行时依赖 @duckdb/duckdb-wasm）
├── README.md          # 说明文档
└── pkg/               # WASM 构建输出
    ├── alpha_wasm_analyzer.js    # WASM JavaScript 绑定
    ├── alpha_wasm_analyzer_bg.wasm # WASM 二进制文件
    └── ...                        # 其他构建文件
```

### DuckDB-WASM SQL 工作台

根页 `duckdb-ui.mjs` 与 React 端 `app/src/lib/duckdb.ts` + `SqlWorkbench.tsx`
提供浏览器内 SQL 面：读 Parquet 导出（`read_parquet()`）即席查询，
与 data-engine 的 `/query` 面互补——本地文件走 DuckDB，服务端表走 API。

## 开发指南

### 添加新的技术指标

1. 在 `../packages/core/src/indicators.rs` 中实现指标算法
2. 在 `../wasm-analyzer/src/lib.rs` 中添加 WASM 绑定
3. 在 `app.js` 中添加前端调用逻辑
4. 重新构建 WASM 模块

### 自定义样式

编辑 `style.css`（全局样式），或在 `index.html` 内联局部规则。

### 添加新页面

1. 创建新的 HTML 文件
2. 在 `app.js` 中添加对应的 JavaScript 逻辑
3. 更新导航链接

## 测试

门禁 `scripts/check-web.sh`（接 CI `wasm` 作业）：旧演示页在场 + `node --check`
语法冒烟 + React 工程结构在场断言 + `npm ci`/tsc/vitest/`vite build`。
浏览器兼容目标：Chrome 80+ / Firefox 75+ / Safari 13+ / Edge 80+
（桌面壳 WebView2/WKWebView 同此基线）。

## 性能优化

### WASM 优化
- 使用 `wasm-opt` 进行代码优化（`--enable-simd` 为 wasm-opt 层面的 SIMD 处理，
  rustc `target-feature=+simd128` 未启用）
- 减少内存分配（零拷贝 `SharedF64Buffer` 直写直读）

### 前端优化

- `dist/` 指纹资产长缓存一年 immutable，入口 HTML no-cache（三处缓存头
  同源：优化器清单 / nginx / 部署脚本，见 `../docs/web-cdn.md`）；
- 文本与 wasm 产 `.gz`/`.br` 预压缩体，源站 `gzip_static` 直发。

## 故障排除

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

## 许可证

本项目采用 MIT 许可证 - 查看 [LICENSE](../LICENSE) 文件了解详情。

## 致谢

- [Rust](https://rust-lang.org/) - 系统编程语言
- [WebAssembly](https://webassembly.org/) - 高性能 Web 标准
- [wasm-pack](https://rustwasm.github.io/wasm-pack/) - Rust WebAssembly 工具链

