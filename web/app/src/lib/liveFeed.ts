/**
 * 实时行情接入（L499）：real-time-feed `/ws`（8082）客户端纯逻辑面。
 *
 * 协议契约对齐 `packages/protocols/src/websocket.rs`（serde tag="type"）：
 * - 客户端订阅 `{type:"subscribe", id, channels:["real_time_quotes"], symbols}`
 * - 服务端版本化同步帧 `{type:"sync", channel, seq, op:"full"|"delta", data}`
 *   （首帧 Full、后续 Delta = depth-1 字段级差集，同 `alpha_core::sync` 口径）
 * - 帧内 `data` 为 `RealTimeQuote` `{symbol, price, volume, bid?, ask?, timestamp}`
 *
 * 全部纯函数 + 确定性模拟盘（LCG，同 demoWalk 风格）：真实 feed 不可达时
 * 降级为模拟序列，界面/测试在任何环境可跑。React 侧只是薄包装
 * （hooks/useLiveQuotes），状态推进语义都在这里锁定。
 */

/** 滚动收盘窗口上限（演示口径，SMA(5) 足够） */
export const CLOSES_CAP = 30

/** 单条实时报价（RealTimeQuote 的客户端投影） */
export interface QuoteTick {
  symbol: string
  price: number
  volume: number
  /** 服务端时间戳（ms epoch）；缺省由客户端时钟补 */
  ts?: number
}

/** 行情看板单行状态（含展示推导字段） */
export interface LiveQuote {
  symbol: string
  name?: string
  price: number
  /** 会话锚定价（首个 tick），涨跌% 的基准 */
  anchor: number
  /** 滚动收盘窗口（新值在尾，超上限丢头） */
  closes: number[]
  volume: number
  updatedAt: number
  /** 最近一跳方向：1 涨 / -1 跌 / 0 平（闪烁着色） */
  tickDir: 1 | -1 | 0
}

export function emptyQuote(symbol: string, name?: string): LiveQuote {
  return {
    symbol,
    name,
    price: NaN,
    anchor: NaN,
    closes: [],
    volume: 0,
    updatedAt: 0,
    tickDir: 0,
  }
}

/**
 * 纯 reducer：把一条 tick 合入看板状态（不可变，返回新表）。
 * 首条 tick 建锚（anchor = price，涨跌% = 0）；未知 symbol 允许入场
 * （服务端广播不按订阅过滤，客户端按 symbol 归位）。
 */
export function applyQuoteTick(
  quotes: Record<string, LiveQuote>,
  tick: QuoteTick,
  names: Record<string, string> = {},
  now: number = Date.now(),
): Record<string, LiveQuote> {
  const prev = quotes[tick.symbol] ?? emptyQuote(tick.symbol, names[tick.symbol])
  const hasPrev = Number.isFinite(prev.price)
  const dir: 1 | -1 | 0 = !hasPrev || tick.price === prev.price ? 0 : tick.price > prev.price ? 1 : -1
  const closes = [...prev.closes, tick.price].slice(-CLOSES_CAP)
  return {
    ...quotes,
    [tick.symbol]: {
      ...prev,
      name: prev.name ?? names[tick.symbol],
      price: tick.price,
      anchor: hasPrev ? prev.anchor : tick.price,
      closes,
      volume: tick.volume || prev.volume,
      updatedAt: tick.ts ?? now,
      tickDir: dir,
    },
  }
}

/** 相对锚定价的涨跌百分比（无锚时 null） */
export function changePct(q: LiveQuote): number | null {
  if (!Number.isFinite(q.anchor) || q.anchor <= 0) return null
  return ((q.price - q.anchor) / q.anchor) * 100
}

/** 服务端 → 客户端帧（已解析的判别联合；不认识的帧为 null） */
export type FeedFrame =
  | { kind: 'connected' }
  | { kind: 'sync'; channel: string; seq: number; op: 'full' | 'delta'; data: unknown }
  | { kind: 'data'; channel: string; data: unknown }
  | { kind: 'pong' }
  | { kind: 'error'; message: string }

/** 解析一帧服务端文本（非法 JSON / 未知 type → null，调用方跳过） */
export function parseFeedFrame(raw: string): FeedFrame | null {
  let msg: { type?: unknown; [k: string]: unknown }
  try {
    msg = JSON.parse(raw)
  } catch {
    return null
  }
  switch (msg.type) {
    case 'connected':
      return { kind: 'connected' }
    case 'pong':
      return { kind: 'pong' }
    case 'sync':
      return {
        kind: 'sync',
        channel: String(msg.channel ?? ''),
        seq: Number(msg.seq ?? 0),
        op: msg.op === 'delta' ? 'delta' : 'full',
        data: msg.data,
      }
    case 'data':
      return { kind: 'data', channel: String(msg.channel ?? ''), data: msg.data }
    case 'error':
      return { kind: 'error', message: String(msg.message ?? 'unknown') }
    default:
      return null
  }
}

/** 订阅帧文本（channels 对齐 protocols::channels::REAL_TIME_QUOTES） */
export const QUOTE_CHANNEL = 'real_time_quotes'

export function subscribeMessage(id: string, symbols: string[]): string {
  return JSON.stringify({ type: 'subscribe', id, channels: [QUOTE_CHANNEL], symbols })
}

/** 心跳帧（服务端 30s 静默判定前的客户端主动 pong） */
export const pingMessage = JSON.stringify({ type: 'ping' })

/**
 * 从 sync/data 帧的 data 里提取报价 tick（RealTimeQuote 形状校验：
 * symbol 非空字符串、price 为有限正数）。
 */
export function extractQuoteTick(data: unknown): QuoteTick | null {
  if (typeof data !== 'object' || data === null) return null
  const d = data as Record<string, unknown>
  if (typeof d.symbol !== 'string' || d.symbol === '') return null
  if (typeof d.price !== 'number' || !Number.isFinite(d.price) || d.price <= 0) return null
  return {
    symbol: d.symbol,
    price: d.price,
    volume: typeof d.volume === 'number' && Number.isFinite(d.volume) ? d.volume : 0,
    ts: typeof d.timestamp === 'number' ? d.timestamp : undefined,
  }
}

/**
 * 通道快照合并：Full 直接替换；Delta 做字段级浅合并（与
 * alpha_core::sync 的 depth-1 差集口径对齐）。无前置快照时 Delta
 * 无法合并（返回 null，等下一帧 Full 重同步）。
 */
export function mergeChannelSnapshot(
  prev: Record<string, unknown> | null,
  op: 'full' | 'delta',
  data: unknown,
): Record<string, unknown> | null {
  if (op === 'full') {
    return typeof data === 'object' && data !== null ? (data as Record<string, unknown>) : null
  }
  if (prev === null || typeof data !== 'object' || data === null) return null
  return { ...prev, ...(data as Record<string, unknown>) }
}

/** 重连退避：500ms 起步指数递增，封顶 8s（确定性，无抖动） */
export function backoffMs(attempt: number): number {
  return Math.min(500 * 2 ** Math.max(0, attempt), 8000)
}

/**
 * 确定性模拟行情发生器（LCG，同 demoWalk 风格）：真实 feed 不可达时
 * 的降级数据源。同 seed + 同符号序列 → 同一 tick 序列（可复现）。
 * 价格微幅随机游走（±0.3% 内），成交量缓慢漂移。
 */
export function createSimTicker(
  symbols: { symbol: string; base: number }[],
  seed: number,
): () => QuoteTick {
  let s = seed >>> 0
  const rand = () => {
    s = (s * 1664525 + 1013904223) >>> 0
    return s / 0x100000000
  }
  const prices = symbols.map((x) => x.base)
  const volumes = symbols.map(() => 10_000)
  let cursor = 0
  return () => {
    // 轮转推进每个符号，保证等间隔更新
    const i = cursor % symbols.length
    cursor += 1
    const drift = (rand() - 0.5) * 0.006 // ±0.3%
    prices[i] = Math.round(prices[i]! * (1 + drift) * 100) / 100
    volumes[i] = Math.max(1, volumes[i]! + Math.round((rand() - 0.5) * 2000))
    return { symbol: symbols[i]!.symbol, price: prices[i]!, volume: volumes[i]! }
  }
}

/** feed WS 地址：`?feedWs=` 查询参数覆盖 → 默认同主机 8082（服务约定端口） */
export function feedWsUrl(loc: { hostname: string; search?: string } = location): string {
  const override = new URLSearchParams(loc.search ?? '').get('feedWs')
  return override ?? `ws://${loc.hostname}:8082/ws`
}
