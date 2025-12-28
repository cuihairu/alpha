import * as duckdb from './vendor/duckdb/duckdb-browser.mjs';

let db = null;
let conn = null;

function setStatus(type, message) {
  const element = document.getElementById('duckdb-status');
  if (!element) return;
  element.innerHTML = `<div class="status ${type}">${message}</div>`;
}

function escapeIdentifier(name) {
  // DuckDB supports quoting identifiers with double-quotes.
  return `"${String(name).replaceAll('"', '""')}"`;
}

function guessFormat(filename) {
  const lower = filename.toLowerCase();
  if (lower.endsWith('.parquet')) return 'parquet';
  if (lower.endsWith('.csv')) return 'csv';
  if (lower.endsWith('.json')) return 'json';
  return 'unknown';
}

function arrowTableToRows(table, maxRows = 50) {
  const fields = table.schema.fields.map((f) => f.name);
  const rows = [];

  let emitted = 0;
  for (const batch of table.batches) {
    const columns = fields.map((_, i) => batch.getChildAt(i));
    for (let r = 0; r < batch.numRows && emitted < maxRows; r += 1) {
      const row = {};
      for (let c = 0; c < fields.length; c += 1) {
        row[fields[c]] = columns[c]?.get(r);
      }
      rows.push(row);
      emitted += 1;
    }
    if (emitted >= maxRows) break;
  }

  return { fields, rows, totalRows: table.numRows };
}

function renderRows(targetId, result) {
  const target = document.getElementById(targetId);
  if (!target) return;

  const { fields, rows, totalRows } = result;
  if (fields.length === 0) {
    target.innerHTML = '<div class="status info">无返回列</div>';
    return;
  }

  const header = fields.map((f) => `<th>${f}</th>`).join('');
  const body = rows
    .map((row) => {
      const tds = fields
        .map((f) => `<td>${row[f] === null || row[f] === undefined ? '' : String(row[f])}</td>`)
        .join('');
      return `<tr>${tds}</tr>`;
    })
    .join('');

  target.innerHTML = `
    <div style="margin-bottom: 8px;">
      <strong>Rows:</strong> ${totalRows} (showing ${rows.length})
    </div>
    <div style="overflow:auto; max-height: 420px;">
      <table style="width:100%; border-collapse: collapse;">
        <thead><tr>${header}</tr></thead>
        <tbody>${body}</tbody>
      </table>
    </div>
  `;
}

async function initDuckDB() {
  if (db && conn) return;

  setStatus('loading', '🦆 正在加载 DuckDB (WASM)...');
  const worker = await duckdb.createWorker('./vendor/duckdb/duckdb-browser-mvp.worker.js');
  db = new duckdb.AsyncDuckDB(new duckdb.ConsoleLogger(), worker);
  await db.instantiate('./vendor/duckdb/duckdb-mvp.wasm', null);
  conn = await db.connect();

  setStatus('success', '✅ DuckDB 已就绪（临时内存库）');
}

async function loadParquetFromUrl() {
  await initDuckDB();

  const urlInput = document.getElementById('duckdb-parquet-url');
  const tableInput = document.getElementById('duckdb-parquet-table');
  const url = urlInput?.value?.trim();
  const table = tableInput?.value?.trim() || 'server_data';

  if (!url) {
    setStatus('error', '❌ 请输入 Parquet URL');
    return;
  }

  setStatus('loading', '⬇️ 正在拉取 Parquet 并加载到 DuckDB...');
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  const bytes = new Uint8Array(await response.arrayBuffer());
  const filename = `${table}.parquet`;
  await db.registerFileBuffer(filename, bytes);

  const tableName = escapeIdentifier(table);
  await conn.query(`CREATE OR REPLACE TABLE ${tableName} AS SELECT * FROM read_parquet('${filename}')`);
  setStatus('success', `✅ 已加载到表 ${table}`);
}

async function loadLocalFile() {
  await initDuckDB();

  const fileInput = document.getElementById('duckdb-file');
  const tableInput = document.getElementById('duckdb-file-table');
  const table = tableInput?.value?.trim() || 'local_data';

  const file = fileInput?.files?.[0];
  if (!file) {
    setStatus('error', '❌ 请选择本地文件（CSV/Parquet）');
    return;
  }

  const fmt = guessFormat(file.name);
  if (fmt === 'unknown') {
    setStatus('error', '❌ 不支持的文件类型（仅 CSV/Parquet/JSON）');
    return;
  }

  setStatus('loading', `📥 正在加载本地文件到表 ${table}...`);
  const bytes = new Uint8Array(await file.arrayBuffer());
  await db.registerFileBuffer(file.name, bytes);

  const tableName = escapeIdentifier(table);
  if (fmt === 'parquet') {
    await conn.query(`CREATE OR REPLACE TABLE ${tableName} AS SELECT * FROM read_parquet('${file.name}')`);
  } else if (fmt === 'csv') {
    await conn.query(`CREATE OR REPLACE TABLE ${tableName} AS SELECT * FROM read_csv_auto('${file.name}', header=true)`);
  } else if (fmt === 'json') {
    await conn.query(`CREATE OR REPLACE TABLE ${tableName} AS SELECT * FROM read_json_auto('${file.name}')`);
  }

  setStatus('success', `✅ 已加载到表 ${table}`);
}

async function runSql() {
  await initDuckDB();

  const sqlInput = document.getElementById('duckdb-sql');
  const sql = sqlInput?.value?.trim();
  if (!sql) {
    setStatus('error', '❌ 请输入 SQL');
    return;
  }

  setStatus('loading', '▶️ 正在执行 SQL...');
  const start = performance.now();
  const table = await conn.query(sql);
  const elapsedMs = Math.round(performance.now() - start);

  renderRows('duckdb-results', arrowTableToRows(table, 80));
  setStatus('success', `✅ SQL 执行完成（${elapsedMs} ms）`);
}

window.duckdbLoadParquetFromUrl = () =>
  loadParquetFromUrl().catch((e) => setStatus('error', `❌ 加载失败: ${e.message || e}`));
window.duckdbLoadLocalFile = () =>
  loadLocalFile().catch((e) => setStatus('error', `❌ 加载失败: ${e.message || e}`));
window.duckdbRunSql = () => runSql().catch((e) => setStatus('error', `❌ SQL 失败: ${e.message || e}`));

document.addEventListener('DOMContentLoaded', () => {
  initDuckDB().catch((e) => setStatus('error', `❌ DuckDB 初始化失败: ${e.message || e}`));
});

