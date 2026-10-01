# Web 端 CDN 部署与静态资源优化（TODO L515）

三件套：**构建期优化**（`web/scripts/optimize-dist.mjs`，已接入
`npm run build`）、**源站**（compose `web-origin` + nginx 配置，本机
实测缓存头全绿）、**CDN/对象存储发布**（`scripts/web-cdn-deploy.sh`）。

## 1. 构建期优化（optimize-dist.mjs，零依赖 node）

1. **内容指纹**：js/css/wasm/图片重命名 `<name>.<hash8>.<ext>` 并重写
   html 引用——内容变 = 名字变，指纹资产可发一年 immutable；
2. **双格式预压缩**：文本类 + wasm 同步产 `.gz`（gzip -9）与 `.br`
   （brotli 质量 11）；实测全 dist **82MB → brotli 12.5MB（-84.8%）**，
   边缘/源站直发预压缩体，零运行时压缩 CPU；
3. **cache-manifest.json**：文件 → hash/size/Cache-Control 清单，部署
   脚本逐文件下发缓存头，策略与 nginx 配置同源。

保护约束：`desktop-shell.js` 为 `.gitignore` 入库豁免的桌面壳兜底文件
（tauri distDir 首启依赖），**不参与指纹重命名**（优化器 NO_RENAME 集）。

## 2. 缓存策略（三处同源：优化器清单 / nginx / 部署脚本）

| 资产 | Cache-Control | 理由 |
|---|---|---|
| 指纹资产 `*.hash8.*` | `public, max-age=31536000, immutable` | 内容变 = URL 变，可激进缓存一年 |
| `*.html`、`/` | `no-cache` | 入口每次协商（ETag 304），发布即生效 |
| 其余未指纹 js/css | `public, max-age=300` | 渐进增强入口的短缓存兜底 |

wasm MIME `application/wasm` 由 nginx 1.27 内建 mime.types 提供
（实测响应头正确）。

## 3. 源站（compose web-origin，本机已实测）

`docker compose up web-origin` → nginx:1.27-alpine serve `web/dist`
（端口 8088），配置 `config/nginx/web-origin.conf`：
`gzip_static on` 直发预压缩体（实测 gzip 协商命中）、指纹 regex
location 下发 immutable、html no-cache、nosniff/referrer-policy 最小
安全头。实测：`/`→no-cache、指纹 wasm→wasm MIME+immutable、
指纹 js→immutable、gzip 协商 6.8KB 传输。

nginx 踩坑（已修）：location 正则含 `{n}` 必须整体加引号；server 级
`types {}` 会整体替换 MIME 表（内建已含 wasm，不要自定义）。

## 4. CDN/对象存储发布（web-cdn-deploy.sh）

S3 兼容端点（AWS S3 / R2 / OSS 均可，aws CLI 或 rclone 自动探测）：

```bash
scripts/web-cdn-deploy.sh --bucket my-bucket \
  [--endpoint https://<s3-兼容端点>] [--prefix web] [--dry-run]
```

- 缓存头按 cache-manifest.json 逐文件下发；`.gz/.br` 带
  `Content-Encoding` 上传（CDN 按 Accept-Encoding 直发）；
- **发布原子性**：非 HTML 资产先行、`*.html` 入口最后覆盖——入口出现
  时其指纹引用必已就位；
- `--dry-run` 打印完整计划（无工具也可自测语义）。

CDN 侧配置要点：回源跟随源站缓存头（指纹 immutable 长缓存边缘命中）、
开 brotli/gzip 透传（勿边缘重压缩，预压缩体已最优）、入口 HTML 短
TTL（≤60s）或走源站 no-cache 协商。

## 5. 与相邻项

- **L519 更新 feed**：`latest.json` 与桌面安装包可托管同一 CDN
  （updater endpoint 指向同源 `/releases/` 前缀）；
- **L520 隐私政策**：政策页托管于本 CDN（App Store 提审 URL 引用）；
- **L470 发布流水线**：`npm run build` → `web-cdn-deploy.sh` 接进
  web 作业；源站容器镜像化（nginx + dist COPY）归容器化项 L468。

## 6. 非交互假设

1. S3 兼容协议为对象存储统一面（三大云均有兼容层），不逐厂商写
   SDK 集成；本机无 aws/rclone，dry-run 语义已实测、真传首次接线时跑；
2. 源站 nginx 为最小可用面（单 server 块）；TLS 终止在 CDN 边缘，
   源站间走内网；
3. dist 指纹资产与预压缩体不入库（.gitignore 既有约定，仅三件桌面壳
   兜底文件豁免入库）。
