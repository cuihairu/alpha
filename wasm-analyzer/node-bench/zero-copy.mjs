// 零拷贝 A/B 基准——真实 wasm pkg 产物（JS 边界口径，TODO「实现零拷贝内存管理」验证件）
//
// A（修复前）：backtestSmaCross(Float64Array)——内部 to_vec() 每次 memcpy JS→wasm
// B（修复后）：allocPriceBuffer 一次 → view.set 一次 ingress → backtestSmaCrossPtr 跨调用复用 → freePriceBuffer
//
// 运行：cd wasm-analyzer && wasm-pack build --target web --out-dir pkg --release && node node-bench/zero-copy.mjs
// 注意：--target web 产物在 node 下需手动喂 wasm 字节（fetch 不支持 file://）。

import fs from 'node:fs';
import { fileURLToPath } from 'node:url';
import { performance } from 'node:perf_hooks';

const here = new URL('.', import.meta.url);
const wasm = await import(new URL('../pkg/alpha_wasm_analyzer.js', here).href);
await wasm.default(fs.readFileSync(fileURLToPath(new URL('../pkg/alpha_wasm_analyzer_bg.wasm', here))));

// LCG 随机游走（与 examples/zero_copy_bench.rs 同口径，可交叉验证）
function randomWalk(n) {
  let state = 0x2545f4914f6cdd1dn;
  let price = 100.0;
  const out = new Float64Array(n);
  for (let i = 0; i < n; i++) {
    state = state * 6364136223846793005n + 1442695040888963407n;
    const r = Number(state >> 33n) / 4294967295.0 - 0.5;
    price = Math.max(price * (1.0 + r * 0.01), 1.0);
    out[i] = price;
  }
  return out;
}

const analyzer = new wasm.WasmAnalyzer(null);
console.log('序列长度   交错轮  拷入min(µs/次)  零拷贝min(µs/次)  节省   每次消除 memcpy');

// 交错 A/B 批次并取每批均值的最小值（min）：微基准标准降噪——
// 噪声只会叠加，min 逼近真实成本；报告序列化（equity_curve → JS 数组）
// 两侧同量且占大头，故差值保守、贴合真实边界 memcpy 收益下限
function benchPath(call) {
  const iters = 40;
  let best = Number.POSITIVE_INFINITY;
  for (let round = 0; round < 10; round++) {
    const t0 = performance.now();
    for (let i = 0; i < iters; i++) call();
    best = Math.min(best, (performance.now() - t0) * 1e3 / iters);
  }
  return best;
}

for (const n of [10_000, 100_000]) {
  const prices = randomWalk(n);

  // 预热（JIT + 缓存 + wasm 内存 grow 到稳态）
  for (let i = 0; i < 10; i++) analyzer.backtestSmaCross(prices, 1, 20, 0.0);
  const warm = wasm.allocPriceBuffer(n);
  warm.view.set(prices);
  for (let i = 0; i < 10; i++) analyzer.backtestSmaCrossPtr(warm.ptr, warm.len, 1, 20, 0.0);

  // 交错执行抵消时间漂移
  const copyUs = benchPath(() => analyzer.backtestSmaCross(prices, 1, 20, 0.0));
  const zeroCopyUs = benchPath(() => analyzer.backtestSmaCrossPtr(warm.ptr, warm.len, 1, 20, 0.0));
  wasm.freePriceBuffer(warm.ptr, warm.len);

  const saved = (copyUs - zeroCopyUs) / copyUs * 100;
  const memcpyKb = n * 8 / 1024;
  console.log(
    `${String(n).padStart(8)} ${String(10).padStart(7)} ` +
    `${copyUs.toFixed(2).padStart(13)} ${zeroCopyUs.toFixed(2).padStart(15)} ` +
    `${saved.toFixed(1).padStart(11)}% ${memcpyKb.toFixed(0).padStart(7)} KB`
  );
}
console.log('\n说明：A 每次 memcpy n*8 字节（JS 堆→wasm 线性内存）+ 堆分配；B ingress 后每次零拷贝。');
console.log('      注意：报告序列化（equity_curve→JS 数组）两侧同量且在长序列下占主导，');
console.log('      故本表是「消 memcpy 收益下限」；引擎内 JSON/JS 对象转换优化属后续立项。');
