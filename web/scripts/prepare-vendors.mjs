import fs from 'node:fs/promises';
import path from 'node:path';

const WEB_ROOT = new URL('..', import.meta.url).pathname;
const NODE_MODULES = path.join(WEB_ROOT, 'node_modules');
const VENDOR_ROOT = path.join(WEB_ROOT, 'vendor');

async function ensureDir(dirPath) {
  await fs.mkdir(dirPath, { recursive: true });
}

async function copyFile(sourcePath, targetPath) {
  await ensureDir(path.dirname(targetPath));
  await fs.copyFile(sourcePath, targetPath);
}

async function copyDir(sourceDir, targetDir) {
  await ensureDir(targetDir);
  const entries = await fs.readdir(sourceDir, { withFileTypes: true });
  for (const entry of entries) {
    const from = path.join(sourceDir, entry.name);
    const to = path.join(targetDir, entry.name);
    if (entry.isDirectory()) {
      await copyDir(from, to);
    } else if (entry.isFile()) {
      await fs.copyFile(from, to);
    }
  }
}

async function prepareDuckDB() {
  const distDir = path.join(NODE_MODULES, '@duckdb', 'duckdb-wasm', 'dist');
  const outDir = path.join(VENDOR_ROOT, 'duckdb');
  await ensureDir(outDir);

  // Minimal set for the default (MVP) bundle.
  await copyFile(path.join(distDir, 'duckdb-browser.mjs'), path.join(outDir, 'duckdb-browser.mjs'));
  await copyFile(
    path.join(distDir, 'duckdb-browser-mvp.worker.js'),
    path.join(outDir, 'duckdb-browser-mvp.worker.js'),
  );
  await copyFile(path.join(distDir, 'duckdb-mvp.wasm'), path.join(outDir, 'duckdb-mvp.wasm'));
}

async function prepareApacheArrow() {
  const pkgDir = path.join(NODE_MODULES, 'apache-arrow');
  const outDir = path.join(VENDOR_ROOT, 'apache-arrow');
  await ensureDir(outDir);

  // DuckDB-WASM imports `apache-arrow` as a bare specifier; the importmap points to Arrow.dom.mjs,
  // which relies on relative imports within this package, so we copy the package directory.
  await copyDir(pkgDir, outDir);
}

async function main() {
  await ensureDir(VENDOR_ROOT);
  await Promise.all([prepareDuckDB(), prepareApacheArrow()]);
  console.log('✅ Prepared web vendor bundles in ./vendor/');
}

main().catch((err) => {
  console.error('❌ Failed to prepare vendor bundles:', err);
  process.exitCode = 1;
});

