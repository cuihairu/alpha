# Alpha Finance 文档

基于 Docusaurus 构建的现代化文档站点，专为 Alpha Finance 金融数据分析平台设计。

## 📁 文档结构

```
docs/
├── docs/                   # 文档内容
│   ├── intro.md           # 项目介绍
│   ├── getting-started.md # 快速开始
│   ├── installation.md    # 安装指南
│   └── deployment.md      # 部署指南
├── blog/                  # 博客文章
│   ├── authors.yml        # 作者信息
│   └── 2024-01-01-alpha-finance-launch.md
├── src/                   # 源文件
│   └── css/
│       └── custom.css     # 自定义样式
├── static/                # 静态资源
├── build/                 # 构建输出目录
├── package.json           # 依赖配置
├── docusaurus.config.js   # 站点配置
└── sidebars.js           # 侧边栏配置
```

## 🚀 本地开发

```bash
# 进入文档目录
cd docs

# 安装依赖
npm install

# 启动开发服务器
npm start

# 构建静态网站
npm build

# 预览构建结果
npm run serve
```

## 📖 在线访问

- **文档网站**: https://cuihairu.github.io/alpha/
- **GitHub 仓库**: https://github.com/cuihairu/alpha

## 🛠️ 技术栈

本项目使用 [Docusaurus](https://docusaurus.io/) 作为文档生成工具：

- ✅ 现代化的 React 界面
- ✅ 全文搜索功能
- ✅ 响应式设计
- ✅ Markdown 支持
- ✅ 代码语法高亮
- ✅ 侧边栏导航
- ✅ 博客功能
- ✅ 国际化支持（中文）
- ✅ SEO 优化

## 🎨 特色功能

- **Alpha Finance 品牌定制**: 专业的金融科技风格设计
- **多端适配**: 支持桌面端、平板和移动端
- **实时预览**: 开发时支持热重载
- **自动部署**: GitHub Actions 自动构建和部署

## ⚠️ 已知问题

当前文档系统存在一些已知的链接警告，但不影响正常使用：

- 部分文档中引用了尚未创建的页面（如 API 文档、架构文档等）
- 这些链接会在未来的文档完善后自动修复
- 构建过程正常完成，静态站点可以正常部署

## 🚀 下一步计划

- [ ] 创建 API 文档 (`docs/api/overview.md`)
- [ ] 创建架构设计文档 (`docs/architecture/overview.md`)
- [ ] 创建开发指南 (`docs/development/setup.md`)
- [ ] 添加配置指南 (`docs/configuration.md`)
- [ ] 添加故障排除文档 (`docs/troubleshooting.md`)
- [ ] 创建快速开始指南 (`docs/quick-start.md`)

## 📝 编写文档

### 添加新文档

1. 在 `docs/docs/` 目录下创建 Markdown 文件
2. 在 `sidebars.js` 中配置侧边栏
3. 更新相关文档中的链接

### 撰写博客

1. 在 `docs/blog/` 目录下创建 Markdown 文件
2. 文件名格式：`YYYY-MM-DD-title.md`
3. 在 Front Matter 中设置作者和标签

### 自定义样式

编辑 `src/css/custom.css` 文件来修改样式：

```css
/* 自定义 Alpha Finance 主题色 */
:root {
  --ifm-color-primary: #1e88e5;
}
```

## 🔄 自动部署

文档会在每次推送到 `main` 分支时自动部署到 GitHub Pages：

1. 触发 GitHub Actions 工作流
2. 使用 Node.js 构建静态文件
3. 部署到 GitHub Pages
4. 自动更新在线文档站点

---

**使用 Alpha Finance 开始您的金融数据分析之旅！** 🚀