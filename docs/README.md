# Alpha Finance 文档

本目录包含 Alpha Finance 项目的完整文档。

## 📚 文档结构

```
docs/
├── README.md                 # 文档说明（本文件）
├── DEPLOYMENT.md            # 详细部署指南
├── API.md                   # API 文档
├── CONFIGURATION.md         # 配置说明
├── TROUBLESHOOTING.md       # 故障排除
├── sidebar.js               # 文档侧边栏配置
├── docusaurus.config.js     # Docusaurus 配置
├── package.json             # 文档依赖
├── static/                  # 静态资源
└── blog/                    # 博客文章
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
```

## 📖 在线访问

- **文档网站**: https://cuihairu.github.io/alpha/
- **GitHub 仓库**: https://github.com/cuihairu/alpha

## 🛠️ 文档工具

本项目使用 [Docusaurus](https://docusaurus.io/) 作为文档生成工具：

- ✅ 现代化的文档界面
- ✅ 搜索功能
- ✅ 响应式设计
- ✅ 支持 Markdown
- ✅ 代码高亮
- ✅ 版本管理
- ✅ 国际化支持

## 📝 编写文档

- 使用 Markdown 格式
- 遵循项目文档风格
- 添加代码示例
- 包含图片和图表
- 更新侧边栏配置

## 🔄 自动部署

文档会在每次推送到 `main` 分支时自动部署到 GitHub Pages。