/**
 * 示例页内置静态行情样本（docs/web-framework-selection.md §7⑥）。
 * 真实数据面（REST/WS 经 services）归远端接入 TODO，骨架期不碰网络。
 */
export interface QuoteSample {
  symbol: string
  name: string
  price: number
  changePct: number
  /** 近期收盘序列（供 SMA 示例与 WASM 探针共用输入） */
  closes: number[]
}

export const demoQuotes: QuoteSample[] = [
  {
    symbol: '600519',
    name: '贵州茅台',
    price: 90.5,
    changePct: 1.23,
    closes: [88.1, 88.9, 89.4, 89.7, 90.5],
  },
  {
    symbol: '000001',
    name: '平安银行',
    price: 10.2,
    changePct: -0.45,
    closes: [10.4, 10.35, 10.3, 10.25, 10.2],
  },
  {
    symbol: '300750',
    name: '宁德时代',
    price: 187.6,
    changePct: 2.87,
    closes: [181.0, 182.4, 184.9, 186.2, 187.6],
  },
]
