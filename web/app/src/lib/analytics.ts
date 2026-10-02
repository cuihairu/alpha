/**
 * 行为埋点采集（L480）：内存事件缓冲 + 显式 opt-in 门。
 *
 * 口径（对齐 L512/L520 隐私姿态）：
 * - 缺省关闭：`enabled=false` 时 `record` 直接丢弃，零内存增长；
 * - 开关持久化 `alpha.analytics_opt_in`（应用键，前缀扫描自动纳入 L488 清除面）；
 * - 缓冲封顶丢头（默认 200），`drain` 取走并清空供发送器 flush；
 * - 发送器（flush 到哪）由调用方注入，本模块不管网络；
 * - 用户标识由调用方传入（建议本机随机匿名 id，不含个人身份）。
 */

export const ANALYTICS_OPT_IN_KEY = 'alpha.analytics_opt_in'
export const ANALYTICS_CAP = 200

export interface AnalyticsEvent {
  user: string
  name: string
  ts_ms: number
}

export interface EventBuffer {
  record: (name: string) => void
  drain: () => AnalyticsEvent[]
  pending: () => number
  setEnabled: (enabled: boolean) => void
}

export function loadOptIn(
  storage: Pick<Storage, 'getItem'> | Map<string, string>,
): boolean {
  try {
    const v =
      storage instanceof Map
        ? storage.get(ANALYTICS_OPT_IN_KEY)
        : storage.getItem(ANALYTICS_OPT_IN_KEY)
    return v === '1'
  } catch {
    return false
  }
}

export function saveOptIn(
  storage: Pick<Storage, 'setItem' | 'removeItem'> | Map<string, string>,
  enabled: boolean,
): void {
  try {
    if (storage instanceof Map) {
      if (enabled) storage.set(ANALYTICS_OPT_IN_KEY, '1')
      else storage.delete(ANALYTICS_OPT_IN_KEY)
    } else if (enabled) {
      storage.setItem(ANALYTICS_OPT_IN_KEY, '1')
    } else {
      storage.removeItem(ANALYTICS_OPT_IN_KEY)
    }
  } catch {
    // 隐私模式写失败不阻塞（与主题/工作区/语言同纪律）
  }
}

/**
 * 建缓冲：关闭时 record 为空操作；开启后按调用方时钟戳记。
 * `now` 可注入（测试定死时间）。
 */
export function createEventBuffer(
  user: string,
  initialEnabled: boolean,
  capacity: number = ANALYTICS_CAP,
  now: () => number = Date.now,
): EventBuffer {
  const queue: AnalyticsEvent[] = []
  let enabled = initialEnabled
  return {
    record: (name: string) => {
      if (!enabled) return
      queue.push({ user, name, ts_ms: now() })
      while (queue.length > capacity) queue.shift()
    },
    drain: () => queue.splice(0, queue.length),
    pending: () => queue.length,
    setEnabled: (next: boolean) => {
      enabled = next
    },
  }
}
