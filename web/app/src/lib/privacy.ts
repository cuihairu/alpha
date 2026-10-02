/**
 * 数据主体权利工具（TODO L488）：GDPR「访问/可携权」与「删除权/被遗忘权」
 * 的客户端工程面。本应用无账号体系，用户维度数据全部在本地存储——
 * 导出与清除在本端即完成（义务映射见 docs/data-privacy.md §2）。
 *
 * 存储访问全部经注入的 KeyValueStore：纯函数可测（node 测试无
 * localStorage），生产用 localStorageStore 适配器。清除范围 =
 * 显式清单 ∪ `alpha.` 前缀扫描（未来新增应用键自动纳入权利面），
 * 非 `alpha.` 键一律不动。
 */

/** 应用数据键显式清单（数据清单的声明面；与 docs/data-privacy.md §3 同步） */
export const USER_DATA_KEYS = ['alpha.workspaces', 'alpha.theme'] as const

/** 应用数据键前缀（扫描面） */
export const APP_KEY_PREFIX = 'alpha.'

/** 最小存储接口（localStorage 的结构投影；测试用 Map 后备实现注入） */
export interface KeyValueStore {
  getItem(key: string): string | null
  setItem(key: string, value: string): void
  removeItem(key: string): void
  /** 全部现存键（localStorage length+key(i) 的数组投影） */
  ownKeys(): string[]
}

/** 生产适配器：window.localStorage */
export function localStorageStore(): KeyValueStore {
  return {
    getItem: (key) => localStorage.getItem(key),
    setItem: (key, value) => localStorage.setItem(key, value),
    removeItem: (key) => localStorage.removeItem(key),
    ownKeys: () =>
      Array.from({ length: localStorage.length }, (_, i) => localStorage.key(i)).filter(
        (key): key is string => key !== null,
      ),
  }
}

/** 当前应用维度的全部现存键（显式清单 ∪ 前缀扫描，按插入序去重） */
export function listUserDataKeys(storage: KeyValueStore): string[] {
  const seen = new Set<string>()
  for (const key of USER_DATA_KEYS) {
    if (storage.getItem(key) !== null) seen.add(key)
  }
  for (const key of storage.ownKeys()) {
    if (key.startsWith(APP_KEY_PREFIX)) seen.add(key)
  }
  return [...seen]
}

/** 导出载荷（schema 自描述；可携权要求的机器可读格式 = JSON） */
export interface UserDataExport {
  schema: 'alpha.user-data'
  version: 1
  exportedAt: string
  data: Record<string, string>
}

/**
 * 导出全部应用维度本地数据：键值原样快照（值就是 JSON 字符串）+
 * 导出时间。空数据照样产出合法载荷（`data: {}`）。
 */
export function exportUserData(storage: KeyValueStore, now: Date = new Date()): UserDataExport {
  const data: Record<string, string> = {}
  for (const key of listUserDataKeys(storage)) {
    const value = storage.getItem(key)
    if (value !== null) data[key] = value
  }
  return { schema: 'alpha.user-data', version: 1, exportedAt: now.toISOString(), data }
}

/**
 * 清除全部应用维度本地数据（被遗忘权）：只删显式清单 ∪ `alpha.` 前缀
 * 命中的键，返回实际删除的键清单；非应用键（同源其他应用/库的存储）
 * 一律不动。幂等：再跑一次返回空。
 */
export function clearUserData(storage: KeyValueStore): string[] {
  const removed = listUserDataKeys(storage)
  for (const key of removed) storage.removeItem(key)
  return removed
}
