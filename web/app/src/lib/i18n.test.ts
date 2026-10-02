import { describe, expect, it } from 'vitest'
import { intlTag, parseLocale, STRINGS, t, tf, type I18nKey } from './i18n'

describe('字典完备性（运行时 double-check，漏 key 主防线是 tsc 类型）', () => {
  it('en 与 zh 键集合一致且全非空', () => {
    const zhKeys = Object.keys(STRINGS.zh).sort()
    expect(Object.keys(STRINGS.en).sort()).toEqual(zhKeys)
    for (const k of zhKeys as I18nKey[]) {
      expect(STRINGS.zh[k].length).toBeGreaterThan(0)
      expect(STRINGS.en[k].length).toBeGreaterThan(0)
    }
  })
  it('t 缺 key 防御返 key 本身', () => {
    expect(t('zh', 'no.such.key' as I18nKey)).toBe('no.such.key')
  })
})

describe('tf 插值', () => {
  it('占位替换；未传变量留原样', () => {
    expect(tf('zh', 'ws.symbolCap', { max: 30 })).toBe('单工作区最多 30 只')
    expect(tf('en', 'ws.deleteTab', { name: 'A' })).toBe('Delete A')
    expect(tf('zh', 'ws.symbolCap', {})).toBe('单工作区最多 {max} 只')
  })
})

describe('parseLocale（?lang= > 存储 > 浏览器 > zh）', () => {
  it('查询参数最高，大小写不敏感', () => {
    expect(parseLocale('?lang=en', 'zh', 'zh-CN')).toBe('en')
    expect(parseLocale('?lang=EN', null, 'zh-CN')).toBe('en')
    expect(parseLocale('?lang=zh', 'en', 'en-US')).toBe('zh')
  })
  it('存储次之；再浏览器语言；非法全回 zh', () => {
    expect(parseLocale('', 'en', 'zh-CN')).toBe('en')
    expect(parseLocale('', null, 'en-US')).toBe('en')
    expect(parseLocale('', null, 'en-GB')).toBe('en')
    expect(parseLocale('', null, 'ja')).toBe('zh')
    expect(parseLocale('', 'fr', 'de')).toBe('zh')
    expect(parseLocale(undefined, null, undefined)).toBe('zh')
  })
})

describe('intlTag', () => {
  it('zh→zh-CN，en→en-US', () => {
    expect(intlTag('zh')).toBe('zh-CN')
    expect(intlTag('en')).toBe('en-US')
  })
})
