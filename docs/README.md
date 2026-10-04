# Alpha 文档站

Docusaurus 3 文档站，内容即 `docs/` 根下的 42 篇工程文档（`docusaurus.config.js`
中 `docs.path: '.'`——不再维护 `docs/docs/` 子树）。站点由 GitHub Pages 托管：
`.github/workflows/docs.yml` 在 `main` 分支的 `docs/**` 或 `README.md`（仓库根）
变更时自动构建部署，地址 https://cuihairu.github.io/alpha/ 。

```
docs/
├── *.md               # 全部文档内容（42 篇，与 sidebars.js 一一登记）
├── docusaurus.config.js
├── sidebars.js        # 分类侧边栏：新文档写完在此登记，未登记成 orphan（构建警告）
├── src/css/custom.css # 主题色
├── static/            # 静态资源（当前为空）
├── build/             # 构建产物（gitignore，CI 内现构建现发布）
└── .gitignore
```

## 本地预览

```bash
cd docs
pnpm install
pnpm start     # 开发热重载
pnpm build     # 产出 build/（同时校验死链）
```

Node 18+ 起站；CI（`.github/workflows/docs.yml`）用 pnpm + Node 24。

## 写作约定

- 标题不带内部条目编号（`（TODO Lxxx）` 不出现在 H1）；正文中的设计登记
  引用（`docs/xxx.md §n`、L 编号）保留，是本仓设计文档的溯源口径。
- 只写已存在、可验证的事实；未落地的能力写明「未立项/边界登记」，
  不预写占位功能与虚构数据。
- 文档间链接用相对路径（`./xxx.md`），构建时会校验。
