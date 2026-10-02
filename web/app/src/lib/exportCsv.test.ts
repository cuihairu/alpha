import { describe, expect, it } from 'vitest'
import { csvField, downloadCsv, QUOTE_CSV_HEADER, quotesToCsv } from './exportCsv'

describe('csvField（RFC4180 最小转义）', () => {
  it('干净值不加引号；逗号/引号/换行加引号且内引号双写', () => {
    expect(csvField('600519')).toBe('600519')
    expect(csvField('贵州茅台')).toBe('贵州茅台')
    expect(csvField('a,b')).toBe('"a,b"')
    expect(csvField('say "hi"')).toBe('"say ""hi"""')
    expect(csvField('l1\nl2')).toBe('"l1\nl2"')
  })
})

describe('quotesToCsv（看板快照导出）', () => {
  it('表头 + 行：涨跌% 保留两位、时间为 ISO、空值留空', () => {
    const text = quotesToCsv([
      {
        symbol: '600519',
        name: '贵州茅台',
        price: 90.5,
        changePct: 0.555,
        volume: 12000,
        updatedAt: 1727856000000,
      },
      { symbol: 'x', price: NaN, changePct: null, volume: 0, updatedAt: 0 },
    ])
    const lines = text.split('\n')
    expect(lines[0]).toBe(QUOTE_CSV_HEADER)
    expect(lines[1]).toBe('600519,贵州茅台,90.5,0.56,12000,2024-10-02T08:00:00.000Z')
    expect(lines[2]).toBe('x,,,,0,')
    expect(text.endsWith('\n')).toBe(true)
  })

  it('空快照只回表头（与服务端 history.csv 空数据口径一致）', () => {
    expect(quotesToCsv([])).toBe(QUOTE_CSV_HEADER + '\n')
  })
})

describe('downloadCsv（非浏览器环境降级）', () => {
  it('node 无 document 时返回 false 不抛错', () => {
    expect(downloadCsv('q.csv', 'a')).toBe(false)
  })
})
