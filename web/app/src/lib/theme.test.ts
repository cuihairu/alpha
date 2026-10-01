import { describe, expect, it } from 'vitest'
import { parseThemePref, resolveTheme } from './theme'

describe('resolveTheme（偏好 → 生效主题）', () => {
  it('system 跟随系统深色检测的两分支', () => {
    expect(resolveTheme('system', true)).toBe('dark')
    expect(resolveTheme('system', false)).toBe('light')
  })

  it('light/dark 直取，不受系统检测影响', () => {
    expect(resolveTheme('light', true)).toBe('light')
    expect(resolveTheme('dark', false)).toBe('dark')
  })
})

describe('parseThemePref（存储读回收口）', () => {
  it('三合法值原样通过', () => {
    expect(parseThemePref('light')).toBe('light')
    expect(parseThemePref('dark')).toBe('dark')
    expect(parseThemePref('system')).toBe('system')
  })

  it('任意脏值/空值缺省跟随系统', () => {
    expect(parseThemePref('banana')).toBe('system')
    expect(parseThemePref(null)).toBe('system')
    expect(parseThemePref('')).toBe('system')
  })
})
