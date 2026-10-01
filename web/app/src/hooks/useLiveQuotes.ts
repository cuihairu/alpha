import { useEffect, useRef, useState } from 'react'
import {
  applyQuoteTick,
  backoffMs,
  createSimTicker,
  emptyQuote,
  extractQuoteTick,
  feedWsUrl,
  mergeChannelSnapshot,
  parseFeedFrame,
  pingMessage,
  subscribeMessage,
  type LiveQuote,
  type QuoteTick,
} from '../lib/liveFeed'

/** 数据源状态：connecting 探测中 / feed 真实通道 / simulated 确定性模拟盘 */
export type FeedSource = 'connecting' | 'feed' | 'simulated'

export interface LiveQuoteState {
  /** 按传入符号序（feed 新引入的符号排后） */
  quotes: LiveQuote[]
  source: FeedSource
  /** 最近 sync 帧版本号（丢帧诊断） */
  seq: number | null
}

export interface LiveSymbol {
  symbol: string
  name?: string
  /** 模拟盘基准价（缺省 100） */
  base?: number
}

const SIM_INTERVAL_MS = 400
const SIM_SEED = 20261002
/** 真实 feed 重试次数上限，超过即降级模拟盘 */
const MAX_RECONNECT = 2
/** 心跳间隔（服务端静默超时判定之前主动 ping） */
const PING_INTERVAL_MS = 20_000

/**
 * 实时行情 hook（L499）：订阅 real-time-feed `/ws`，onopen 发送
 * subscribe、按 sync Full/Delta 帧推进看板；连接失败按 backoff 重试
 * MAX_RECONNECT 次后降级为确定性模拟盘（界面/测试任何环境可跑）。
 * 全部推进语义在 lib/liveFeed.ts（纯函数），这里只做生命周期装配。
 */
export function useLiveQuotes(symbols: LiveSymbol[]): LiveQuoteState {
  const [quotesMap, setQuotesMap] = useState<Record<string, LiveQuote>>(() => {
    const init: Record<string, LiveQuote> = {}
    for (const { symbol, name } of symbols) init[symbol] = emptyQuote(symbol, name)
    return init
  })
  const [source, setSource] = useState<FeedSource>('connecting')
  const [seq, setSeq] = useState<number | null>(null)

  // 稳定引用：符号/名称集变化才重连（序列化比较，避免每次渲染新对象）
  const symbolKey = JSON.stringify(symbols)
  const namesRef = useRef<Record<string, string>>({})
  const snapshotsRef = useRef<Record<string, Record<string, unknown> | null>>({})

  useEffect(() => {
    const symbols: LiveSymbol[] = JSON.parse(symbolKey)
    const names: Record<string, string> = {}
    for (const s of symbols) if (s.name) names[s.symbol] = s.name
    namesRef.current = names
    snapshotsRef.current = {}

    let disposed = false
    let ws: WebSocket | null = null
    let pingTimer: ReturnType<typeof setInterval> | null = null
    let retryTimer: ReturnType<typeof setTimeout> | null = null
    let simTimer: ReturnType<typeof setInterval> | null = null
    let attempt = 0
    let simStarted = false

    const pushTick = (tick: QuoteTick) =>
      setQuotesMap((prev) => applyQuoteTick(prev, tick, namesRef.current))

    const startSim = () => {
      if (simStarted || disposed) return
      simStarted = true
      setSource('simulated')
      const ticker = createSimTicker(
        symbols.map((s) => ({
          symbol: s.symbol,
          base: s.base ?? 100,
        })),
        SIM_SEED,
      )
      simTimer = setInterval(() => pushTick(ticker()), SIM_INTERVAL_MS)
    }

    const clearPing = () => {
      if (pingTimer !== null) {
        clearInterval(pingTimer)
        pingTimer = null
      }
    }

    const connect = () => {
      if (disposed) return
      try {
        ws = new WebSocket(feedWsUrl())
      } catch {
        scheduleRetry()
        return
      }
      ws.onopen = () => {
        if (disposed) return
        attempt = 0
        setSource('feed')
        ws?.send(subscribeMessage(`web-${Date.now()}`, symbols.map((s) => s.symbol)))
        clearPing()
        pingTimer = setInterval(() => ws?.send(pingMessage), PING_INTERVAL_MS)
      }
      ws.onmessage = (ev) => {
        if (disposed || typeof ev.data !== 'string') return
        const frame = parseFeedFrame(ev.data)
        if (frame === null) return
        if (frame.kind === 'sync') {
          setSeq(frame.seq)
          const merged = mergeChannelSnapshot(
            snapshotsRef.current[frame.channel] ?? null,
            frame.op,
            frame.data,
          )
          snapshotsRef.current[frame.channel] = merged
          const tick = merged === null ? null : extractQuoteTick(merged)
          if (tick !== null) pushTick(tick)
        } else if (frame.kind === 'data') {
          // 旧版兼容通道：data 即完整 RealTimeQuote
          const tick = extractQuoteTick(frame.data)
          if (tick !== null) pushTick(tick)
        }
      }
      const onDown = () => {
        clearPing()
        if (disposed) return
        ws = null
        scheduleRetry()
      }
      ws.onclose = onDown
      ws.onerror = onDown
    }

    const scheduleRetry = () => {
      if (disposed) return
      if (attempt >= MAX_RECONNECT) {
        startSim()
        return
      }
      retryTimer = setTimeout(connect, backoffMs(attempt))
      attempt += 1
    }

    connect()
    return () => {
      disposed = true
      if (ws !== null) {
        ws.onclose = null
        ws.onerror = null
        ws.close()
      }
      clearPing()
      if (retryTimer !== null) clearTimeout(retryTimer)
      if (simTimer !== null) clearInterval(simTimer)
    }
  }, [symbolKey])

  // 输出按传入符号序，feed 新引入的符号按插入序排后
  const ordered: LiveQuote[] = []
  for (const s of JSON.parse(symbolKey) as LiveSymbol[]) {
    const q = quotesMap[s.symbol]
    if (q) ordered.push(q)
  }
  for (const q of Object.values(quotesMap)) {
    if (!ordered.some((x) => x.symbol === q.symbol)) ordered.push(q)
  }
  return { quotes: ordered, source, seq }
}
