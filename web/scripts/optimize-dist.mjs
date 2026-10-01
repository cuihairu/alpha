// dist/ 静态资源优化（L515：Web 端 CDN 部署和静态资源优化）
//
// 三件事（全零依赖，node 内置模块）：
//   1. 内容指纹：js/css/wasm 图片重命名 <name>.<hash8>.<ext>，并重写 html 引用
//      ——指纹文件可发 immutable 长缓存，html 改名即发布，无缓存失效问题；
//   2. 预压缩：对文本类与 wasm 产 .gz + .br 双格式（CDN/源站按 Accept-Encoding
//      直发预压缩体，省边缘 CPU）；wasm 的 .br 收益尤其大（~70%+）；
//   3. cache-manifest.json：文件 → { hash, size, cache-control }，供部署脚本
//      （scripts/web-cdn-deploy.sh）逐文件下发 Cache-Control 头。
//
// 用法：node scripts/optimize-dist.mjs [distDir]（缺省 dist/，在 build:prod 后自动跑）

import { createHash } from 'node:crypto';
import { gzipSync, brotliCompressSync, constants as zlibConstants } from 'node:zlib';
import { readFileSync, writeFileSync, readdirSync, statSync, unlinkSync } from 'node:fs';
import { join, extname, basename, dirname } from 'node:path';

const DIST = process.argv[2] ?? 'dist';
// 入库豁免的桌面壳兜底文件（.gitignore !web/dist/desktop-shell.js）：必须以
// 固定名存在（tauri distDir 首启依赖），不参与指纹重命名
const NO_RENAME = new Set(['desktop-shell.js']);
const FINGERPRINT_EXTS = new Set(['.js', '.css', '.wasm', '.png', '.svg', '.ico', '.woff2']);
const TEXT_EXTS = new Set(['.html', '.js', '.css', '.json', '.svg', '.txt']);
const PRECOMPRESS_EXTS = new Set(['.js', '.css', '.wasm', '.json', '.svg', '.html', '.txt']);

const hash8 = (buf) => createHash('sha256').update(buf).digest('hex').slice(0, 8);

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p, out);
    else out.push(p);
  }
  return out;
}

const files = walk(DIST);
const renames = new Map(); // 旧相对路径 → 新相对路径
let rawTotal = 0, brTotal = 0;

for (const abs of files) {
  const rel = abs.slice(DIST.length + 1);
  const ext = extname(abs);
  const buf = readFileSync(abs);

  if (
    FINGERPRINT_EXTS.has(ext) &&
    !NO_RENAME.has(basename(abs)) &&
    !/\.[0-9a-f]{8}\.[a-z]+$/.test(basename(abs))
  ) {
    const hashed = `${basename(abs, ext)}.${hash8(buf)}${ext}`;
    const newRel = rel === basename(abs) ? hashed : `${dirname(rel)}/${hashed}`;
    const newAbs = join(DIST, newRel);
    writeFileSync(newAbs, buf);
    unlinkSync(abs);
    renames.set(rel, newRel);
    renames.set(abs, newAbs); // 供 html 重写的双形态键
  }

  if (PRECOMPRESS_EXTS.has(ext)) {
    const cur = renames.get(abs) ?? abs;
    writeFileSync(`${cur}.gz`, gzipSync(buf, { level: 9 }));
    writeFileSync(`${cur}.br`, brotliCompressSync(buf, {
      params: { [zlibConstants.BROTLI_PARAM_QUALITY]: 11 },
    }));
    rawTotal += buf.length;
    brTotal += statSync(`${cur}.br`).size;
  }
}

// html 内引用重写（src/href 里出现旧文件名的都换新名；重复两轮覆盖链式引用）
const htmlFiles = files.filter((f) => extname(f) === '.html');
for (const html of htmlFiles) {
  let text = readFileSync(html, 'utf8');
  for (const [oldRel, newRel] of renames) {
    if (typeof oldRel !== 'string') continue;
    text = text.split(oldRel).join(newRel);
  }
  writeFileSync(html, text);
}

// 缓存策略清单：指纹文件 immutable 一年；html 入口 no-cache（每次协商）；
// 其余（server.js 等非浏览器资产）短缓存
const manifest = {};
for (const abs of walk(DIST)) {
  const rel = abs.slice(DIST.length + 1);
  if (rel.endsWith('.gz') || rel.endsWith('.br')) continue;
  const ext = extname(abs);
  const cache = /\.[0-9a-f]{8}\./.test(basename(abs))
    ? 'public, max-age=31536000, immutable'
    : ext === '.html'
      ? 'no-cache'
      : 'public, max-age=300';
  const buf = readFileSync(abs);
  manifest[rel] = {
    hash: hash8(buf),
    size: buf.length,
    'cache-control': cache,
  };
}
writeFileSync(join(DIST, 'cache-manifest.json'), JSON.stringify(manifest, null, 2) + '\n');

const kb = (n) => `${(n / 1024).toFixed(1)}KB`;
console.log(
  `optimize-dist: 指纹 ${renames.size / 2} 个资产；预压缩 ${kb(rawTotal)} → brotli ${kb(brTotal)}` +
    `（-${((1 - brTotal / rawTotal) * 100).toFixed(1)}%）；清单 ${Object.keys(manifest).length} 项`
);
