import { describe, expect, it } from 'vitest'
import {
  ANALYTICS_CAP,
  createEventBuffer,
  loadOptIn,
  saveOptIn,
} from './analytics'

describe('opt-in 开关（缺省关闭，显式开启才记）', () => {
  it('Map 后备：读写删回环；非法值/异常回 false', () => {
    const store = new Map<string, string>()
    expect(loadOptIn(store)).toBe(false)
    saveOptIn(store, true)
    expect(loadOptIn(store)).toBe(true)
    saveOptIn(store, false)
    expect(loadOptIn(store)).toBe(false)
    store.set('alpha.analytics_opt_in', 'yes')
    expect(loadOptIn(store)).toBe(false)
  })
})

describe('createEventBuffer（封顶丢头 + drain 清空）', () => {
  it('关闭时 record 为空操作零堆积；setEnabled 翻转即生效', () => {
    const buf = createEventBuffer('u', false, 4, () => 1000)
    buf.record('a')
    expect(buf.pending()).toBe(0)
    expect(buf.drain()).toEqual([])
    buf.setEnabled(true)
    buf.record('b')
    expect(buf.pending()).toBe(1)
    buf.setEnabled(false)
    buf.record('c')
    expect(buf.pending()).toBe(1)
  })
  it('开启后记名记时；超 cap 丢头；drain 取走清空', () => {
    let now = 1000
    const buf = createEventBuffer('u7', true, 3, () => now)
    buf.record('a')
    now = 2000
    buf.record('b')
    buf.record('c')
    buf.record('d')
    expect(buf.pending()).toBe(3)
    const got = buf.drain()
    expect(got.map((e) => e.name)).toEqual(['b', 'c', 'd'])
    expect(got[0]).toMatchObject({ user: 'u7', ts_ms: 2000 })
    expect(buf.pending()).toBe(0)
  })
  it('缺省容量 ANALYTICS_CAP', () => {
    const buf = createEventBuffer('u', true)
    for (let i = 0; i < ANALYTICS_CAP + 10; i++) buf.record('x')
    expect(buf.pending()).toBe(ANALYTICS_CAP)
  })
})
