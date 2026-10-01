import { describe, expect, it } from 'vitest'
import { capRows } from './resultTable'

describe('capRows（结果表截断与空值口径）', () => {
  it('行数低于上限：全量呈现，truncated=false，列名透传', () => {
    const out = capRows(['a', 'b'], [[1, 'x'], [2, 'y']], 100)
    expect(out).toEqual({
      columns: ['a', 'b'],
      rows: [['1', 'x'], ['2', 'y']],
      total: 2,
      truncated: false,
    })
  })

  it('超上限：截断到 cap 行，total 保留全量，truncated=true', () => {
    const rows = Array.from({ length: 5 }, (_, i) => [i])
    const out = capRows(['n'], rows, 3)
    expect(out.rows).toEqual([['0'], ['1'], ['2']])
    expect(out.total).toBe(5)
    expect(out.truncated).toBe(true)
  })

  it('空值口径：null/undefined → NULL（SQL 惯例），0/false 不吞', () => {
    const out = capRows(['c'], [[null], [undefined], [0], [false], ['']], 10)
    expect(out.rows).toEqual([['NULL'], ['NULL'], ['0'], ['false'], ['']])
  })

  it('BigInt 值字符串化（Arrow 整型可能为 bigint）', () => {
    expect(capRows(['v'], [[10n]], 10).rows).toEqual([['10']])
  })
})
