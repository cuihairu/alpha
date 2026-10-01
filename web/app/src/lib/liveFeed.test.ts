import { describe, expect, it } from 'vitest'
import {
  applyQuoteTick,
  backoffMs,
  changePct,
  CLOSES_CAP,
  createSimTicker,
  emptyQuote,
  extractQuoteTick,
  feedWsUrl,
  mergeChannelSnapshot,
  parseFeedFrame,
  pingMessage,
  subscribeMessage,
} from './liveFeed'

/**
 * 契约锚点：`packages/protocols/src/websocket.rs`（serde tag="type"）与
 * services/real-time-feed `/ws`（8082）——订阅/同步帧/RealTimeQuote 形状。
 */

describe('parseFeedFrame（协议形状）', () => {
  it('sync 帧解析 op/seq/channel/data', () => {
    const raw = JSON.stringify({
      type: 'sync',
      channel: 'real_time_quotes',
      seq: 7,
      op: 'delta',
      data: { price: 10.5 },
    })
    expect(parseFeedFrame(raw)).toEqual({
      kind: 'sync',
      channel: 'real_time_quotes',
      seq: 7,
      op: 'delta',
      data: { price: 10.5 },
    })
  })

  it('connected / data / error / 未知 type / 非法 JSON', () => {
    expect(parseFeedFrame('{"type":"connected"}')).toEqual({ kind: 'connected' })
    expect(parseFeedFrame('{"type":"data","channel":"c","data":{"a":1}}')).toEqual({
      kind: 'data',
      channel: 'c',
      data: { a: 1 },
    })
    expect(parseFeedFrame('{"type":"error","message":"boom"}')).toEqual({
      kind: 'error',
      message: 'boom',
    })
    expect(parseFeedFrame('{"type":"mystery"}')).toBeNull()
    expect(parseFeedFrame('not json')).toBeNull()
  })

  it('订阅帧形状对齐 SubscribeRequest（serde tag + channels 常量）', () => {
    expect(JSON.parse(subscribeMessage('conn-1', ['600519']))).toEqual({
      type: 'subscribe',
      id: 'conn-1',
      channels: ['real_time_quotes'],
      symbols: ['600519'],
    })
    expect(JSON.parse(pingMessage)).toEqual({ type: 'ping' })
  })
})

describe('extractQuoteTick（RealTimeQuote 校验）', () => {
  it('合法报价 → tick（volume 非法回退 0）', () => {
    expect(
      extractQuoteTick({ symbol: '600519', price: 90.5, volume: 123, timestamp: 1727856000000 }),
    ).toEqual({ symbol: '600519', price: 90.5, volume: 123, ts: 1727856000000 })
    expect(extractQuoteTick({ symbol: 'x', price: 1, volume: 'bad' })).toEqual({
      symbol: 'x',
      price: 1,
      volume: 0,
      ts: undefined,
    })
  })
  it('symbol 空 / price 非有限正数 / 非对象 → null', () => {
    expect(extractQuoteTick({ symbol: '', price: 1 })).toBeNull()
    expect(extractQuoteTick({ symbol: 'x', price: -1 })).toBeNull()
    expect(extractQuoteTick({ symbol: 'x', price: '10' })).toBeNull()
    expect(extractQuoteTick(null)).toBeNull()
    expect(extractQuoteTick('str')).toBeNull()
  })
})

describe('applyQuoteTick（看板 reducer）', () => {
  it('首条建锚（涨跌% = 0），随后按跳变着色方向', () => {
    let quotes = applyQuoteTick({}, { symbol: '600519', price: 100, volume: 1 }, {}, 1000)
    expect(quotes['600519']).toMatchObject({ price: 100, anchor: 100, tickDir: 0 })
    quotes = applyQuoteTick(quotes, { symbol: '600519', price: 101, volume: 2 }, {}, 2000)
    expect(quotes['600519']).toMatchObject({ tickDir: 1, updatedAt: 2000 })
    quotes = applyQuoteTick(quotes, { symbol: '600519', price: 100.5, volume: 3 }, {}, 3000)
    expect(quotes['600519']).toMatchObject({ tickDir: -1 })
    quotes = applyQuoteTick(quotes, { symbol: '600519', price: 100.5, volume: 3 }, {}, 4000)
    expect(quotes['600519']).toMatchObject({ tickDir: 0 })
    expect(changePct(quotes['600519']!)).toBeCloseTo(0.5, 10)
  })

  it('未知 symbol 允许入场并补名（服务端广播不按订阅过滤）', () => {
    const quotes = applyQuoteTick(
      {},
      { symbol: '000001', price: 10.2, volume: 5 },
      { '000001': '平安银行' },
      1,
    )
    expect(quotes['000001']).toMatchObject({ name: '平安银行', anchor: 10.2 })
  })

  it('滚动窗口封顶 CLOSES_CAP（丢头留尾）', () => {
    let quotes: Record<string, ReturnType<typeof emptyQuote>> = {}
    for (let i = 1; i <= CLOSES_CAP + 5; i++) {
      quotes = applyQuoteTick(quotes, { symbol: 's', price: i, volume: 0 }, {}, i)
    }
    const q = quotes['s']!
    expect(q.closes).toHaveLength(CLOSES_CAP)
    expect(q.closes[0]).toBe(6) // 前 5 条被丢
    expect(q.closes.at(-1)).toBe(CLOSES_CAP + 5)
  })

  it('不可变：输入表不被改写', () => {
    const before = applyQuoteTick({}, { symbol: 'a', price: 1, volume: 1 }, {}, 1)
    const snapshot = JSON.stringify(before)
    applyQuoteTick(before, { symbol: 'a', price: 2, volume: 1 }, {}, 2)
    expect(JSON.stringify(before)).toBe(snapshot)
  })
})

describe('mergeChannelSnapshot（Full/Delta 浅合并）', () => {
  it('Full 替换；Delta 无前置快照 → null（等重同步）', () => {
    const full = mergeChannelSnapshot(null, 'full', { symbol: 'x', price: 1 })
    expect(full).toEqual({ symbol: 'x', price: 1 })
    expect(mergeChannelSnapshot(null, 'delta', { price: 2 })).toBeNull()
  })
  it('Delta 字段级覆盖前置快照（depth-1 差集口径）', () => {
    const prev = { symbol: 'x', price: 1, volume: 10 }
    expect(mergeChannelSnapshot(prev, 'delta', { price: 2 })).toEqual({
      symbol: 'x',
      price: 2,
      volume: 10,
    })
  })
})

describe('createSimTicker（确定性模拟盘）', () => {
  const symbols = [
    { symbol: '600519', base: 90.5 },
    { symbol: '000001', base: 10.2 },
  ]
  it('同 seed 同符号序 → 同一 tick 序列（可复现）', () => {
    const a = createSimTicker(symbols, 20261002)
    const b = createSimTicker(symbols, 20261002)
    for (let i = 0; i < 30; i++) {
      expect(a()).toEqual(b())
    }
  })
  it('轮转推进：三次调用覆盖全部符号一轮，价格有限为正', () => {
    const t = createSimTicker(symbols, 7)
    const ticks = [t(), t(), t()]
    expect(ticks.map((x) => x.symbol)).toEqual(['600519', '000001', '600519'])
    for (const x of ticks) {
      expect(Number.isFinite(x.price)).toBe(true)
      expect(x.price).toBeGreaterThan(0)
    }
  })
})

describe('backoffMs（重连退避，封顶 8s）', () => {
  it('500 起步指数递增后封顶', () => {
    expect([0, 1, 2, 3, 4, 5, 10].map(backoffMs)).toEqual([
      500, 1000, 2000, 4000, 8000, 8000, 8000,
    ])
  })
})

describe('feedWsUrl（地址解析）', () => {
  it('默认同主机 8082；?feedWs= 覆盖', () => {
    expect(feedWsUrl({ hostname: 'demo.example', search: '' })).toBe(
      'ws://demo.example:8082/ws',
    )
    expect(
      feedWsUrl({ hostname: 'demo.example', search: '?feedWs=ws://feed:9999/ws' }),
    ).toBe('ws://feed:9999/ws')
  })
})
