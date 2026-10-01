#!/usr/bin/env bash
# Web 前端门禁（L427：React 骨架落地 + 旧演示页行为不回退守门）。
# 与 check-lint.sh / check-cross-platform.sh / check-desktop.sh 并列，
# 接入 CI `wasm` 作业尾部（ubuntu-latest 自带 node）；本地全仓门禁之一。
set -euo pipefail
cd "$(dirname "$0")/.."

echo "=== Web 前端检查 ==="

echo "--- [1/4] 旧演示页在场 + 语法冒烟（行为不回退守门）"
# web/ 既有 vanilla 演示（L427 选型时零改动，见 docs/web-framework-selection.md §3）
for f in web/index.html web/app.js web/demo-data.js web/server.js web/wasm-demo.html web/style.css; do
  if [ ! -f "$f" ]; then
    echo "缺失旧演示页文件: $f"
    exit 1
  fi
done
node --check web/server.js
node --check web/app.js
node --check web/demo-data.js

echo "--- [2/4] React 骨架工程结构在场"
for f in web/app/package.json web/app/package-lock.json web/app/index.html \
         web/app/src/main.tsx web/app/src/App.tsx web/app/src/lib/indicators.ts \
         web/app/src/lib/demoWalk.ts web/app/src/lib/resultTable.ts web/app/src/lib/duckdb.ts \
         web/app/src/components/QuoteTable.tsx web/app/src/components/IndicatorPanel.tsx \
         web/app/src/components/PriceChart.tsx web/app/src/components/SqlWorkbench.tsx \
         web/app/src/components/ResultGrid.tsx web/app/src/components/WasmProbe.tsx \
         web/app/src/styles.css \
         web/app/vite.config.ts; do
  if [ ! -f "$f" ]; then
    echo "缺失骨架文件: $f"
    exit 1
  fi
done
node -e "
  const p = require('./web/app/package.json');
  for (const d of ['react', 'react-dom', 'lightweight-charts']) {
    if (!p.dependencies[d]) { console.error('缺运行时依赖: ' + d); process.exit(1); }
  }
  for (const s of ['typecheck', 'test', 'build']) {
    if (!p.scripts[s]) { console.error('缺 npm 脚本: ' + s); process.exit(1); }
  }
"
# 响应式三要素（L431）：viewport meta、样式挂载、移动断点在场
grep -q 'name="viewport"' web/app/index.html || { echo "index.html 缺 viewport meta"; exit 1; }
grep -q 'styles.css' web/app/src/main.tsx || { echo "main.tsx 未挂载 styles.css"; exit 1; }
grep -q '@media (max-width: 720px)' web/app/src/styles.css || { echo "styles.css 缺移动断点"; exit 1; }
grep -q '@media (max-width: 1024px)' web/app/src/styles.css || { echo "styles.css 缺平板断点"; exit 1; }

echo "--- [3/4] 依赖安装（npm ci，lockfile 锁定）"
cd web/app
npm ci --no-audit --no-fund

echo "--- [4/4] 类型检查 + 单测 + 构建"
npm run typecheck
npm test
npm run build

echo "=== Web 前端检查全部通过 ==="
