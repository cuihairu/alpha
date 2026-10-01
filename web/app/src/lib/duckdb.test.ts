import { describe, expect, it } from 'vitest'
import { seedSql, tableToRows, type ArrowTable } from './duckdb'

describe('tableToRows（arrow 表 → 纯行集）', () => {
  const fake = {
    numRows: 2,
    schema: { fields: [{ name: 'a' }, { name: 'b' }] },
    toArray: () => [{ a: 1, b: 'x' }, { a: null, b: 'y' }],
  } as unknown as ArrowTable

  it('按 schema 列序取齐行值（缺列得 undefined，交给 capRows 归 NULL）', () => {
    expect(tableToRows(fake)).toEqual({
      columns: ['a', 'b'],
      rows: [[1, 'x'], [null, 'y']],
    })
  })
})

describe('seedSql（演示表初始化）', () => {
  it('建 demo_quotes 与 demo_candles 两表且含 K 线值行', () => {
    const sql = seedSql([{ time: '2026-01-05', open: 100, high: 101, low: 99, close: 100.5 }])
    expect(sql).toContain('CREATE OR REPLACE TABLE demo_quotes')
    expect(sql).toContain('CREATE OR REPLACE TABLE demo_candles')
    expect(sql).toContain("('2026-01-05',100,101,99,100.5)")
  })
})
