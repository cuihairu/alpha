/**
 * 国际化（L479）：中英双语字典 + 语言解析。
 *
 * 口径：
 * - 缺省中文（`zh`），与既有界面一致；`?lang=en` 查询参数 > localStorage
 *  （`alpha.locale`）> `navigator.language` > 缺省 zh。
 * - 字典以 zh 为键全集：`I18nKey = keyof typeof zh`，`en` 必须
 *   `Record<I18nKey, string>`——漏 key 编译期即拦（tsc），运行时另有非空测试。
 * - 插值用 `{name}` 占位 + `tf` 替换；数字/日期格式化由调用方按 locale 选
 *   Intl tag（`zh-CN`/`en-US`），不在字典里拼字符串。
 * - 专有名词不译：标的代码/名称、SMA/EMA/DuckDB-WASM、表名 demo_quotes。
 * - 边界：异常 throw 文案（`indicators.ts` period 校验）是开发面不断言面，
 *   不进字典；CSV/JSON 导出格式是机器契约，不进字典。
 */

export type Locale = 'zh' | 'en'

export const LOCALE_STORAGE_KEY = 'alpha.locale'

const zh = {
  // —— App ——
  'app.title': 'Alpha Finance · 数据分析界面',
  'app.intro':
    'TODO L427 选型 + L428 组件化 + L429 图表库 + L430 SQL 工作台 + L431 响应式 + L432 主题系统 + L499 实时看板 + L508 多标签工作区。旧演示页（web/index.html 等）零改动并存。',
  'app.liveSection': '实时行情（L499：/ws 订阅，不可达降级模拟盘；标的集 = 活动工作区自选）',
  'app.demoSection': '行情演示（内置静态样本）',
  'app.language': '语言',
  // —— 主题 ——
  'theme.label': '主题',
  'theme.system': '跟随系统',
  'theme.light': '浅色',
  'theme.dark': '深色',
  // —— 工作区 ——
  'ws.defaultName': '默认',
  'ws.blankName': '工作区',
  'ws.symbolInvalid': '代码须为 6 位数字',
  'ws.symbolDup': '代码已存在',
  'ws.symbolCap': '单工作区最多 {max} 只',
  'ws.deleteTab': '删除 {name}',
  'ws.removeSymbol': '移除 {symbol}',
  'ws.newPlaceholder': '工作区名称',
  'ws.confirm': '确定',
  'ws.create': '＋ 新建',
  'ws.rename': '重命名',
  'ws.addSymbolPlaceholder': '加自选（6 位代码）',
  // —— 实时看板 ——
  'feed.connecting': '连接 real-time-feed…',
  'feed.feed': '实时已连接',
  'feed.simulated': '模拟行情（feed 不可达，确定性序列）',
  'feed.seq': 'sync seq {seq}',
  'feed.hint': '地址覆盖：?feedWs=ws://host:port/ws',
  'feed.export': '导出 CSV',
  'feed.colSymbol': '代码',
  'feed.colName': '名称',
  'feed.colPrice': '最新价',
  'feed.colChange': '涨跌%',
  'feed.colSma': 'SMA(5)',
  'feed.colVolume': '成交量',
  'feed.colUpdated': '更新时间',
  // —— 演示表 ——
  'feed.colSma3': 'SMA(3) 末值（纯 TS）',
  // —— 指标面板 ——
  'ind.title': '指标分析',
  'ind.symbol': '代码',
  'ind.indicator': '指标',
  'ind.period': '周期',
  'ind.sma': 'SMA 简单移动平均',
  'ind.ema': 'EMA 指数移动平均',
  'ind.last': '末值：',
  'ind.smaNote': '（SMA 前 {n} 位为 0.0 占位，非真实值）',
  'ind.colClose': '收盘',
  // —— K 线图 ——
  'chart.title': 'K 线图（lightweight-charts · Canvas，红涨绿跌）',
  'chart.caption': '示例 K 线为确定性合成行情（LCG 种子生成，跳过周末，见 src/lib/demoWalk.ts）；SMA({n}) 叠加线与指标面板同口径。',
  // —— SQL 工作台 ——
  'sql.title': 'SQL 工作台（DuckDB-WASM）',
  'sql.idleA': '引擎未启用。先在 ',
  'sql.idleB': ' 执行 ',
  'sql.idleC': '，再把 ',
  'sql.idleD': ' 拷入 ',
  'sql.idleE': '，然后加载。',
  'sql.load': '加载 SQL 引擎',
  'sql.loading': '引擎加载中…',
  'sql.errorPrefix': '引擎/查询错误：',
  'sql.retry': '重试加载',
  'sql.ready': '引擎就绪（演示表：demo_quotes / demo_candles）。编辑 SQL 后执行：',
  'sql.run': '执行查询',
  'sql.running': '执行中…',
  'sql.summary': '{total} 行 × {cols} 列 · {ms} ms',
  'sql.truncated': '（仅显示前 {n} 行）',
  // —— WASM 探针 ——
  'wasm.title': 'Rust WASM 引擎探针',
  'wasm.idle': '未加载。',
  'wasm.load': '加载 WASM 引擎',
  'wasm.loading': '加载中…',
  'wasm.loaded': '已加载。600519 SMA(3) 末值 = ',
  'wasm.sep': '，',
  'wasm.match': '与纯 TS 口径逐位一致 ✔',
  'wasm.mismatch': '与纯 TS 口径不一致 ✘（请核对指标实现）',
  'wasm.detail': 'wasm 输出 {a} 位 / 纯 TS 输出 {b} 位（口径：等长、前导 0.0 占位、4 位小数取整）',
  'wasm.missingA': '未构建：先在 ',
  'wasm.missingB': ' 执行 ',
  'wasm.missingC': '，再把 ',
  'wasm.missingD': ' 拷入 ',
  'wasm.missingE': ' 后重试。',
  'wasm.retry': '重试',
  // —— 隐私面板 ——
  'privacy.title': '数据与隐私（L488：本地数据导出 / 清除，无账号体系）',
  'privacy.descA': '应用用户数据仅存本机（键清单 ',
  'privacy.descB': ' 项：自选工作区、主题偏好），服务端不留存个人维度数据。义务映射见 ',
  'privacy.period': '。',
  'privacy.present': '（本机存在）',
  'privacy.absent': '（未使用）',
  'privacy.export': '导出我的数据（JSON）',
  'privacy.clear': '清除我的数据',
  'privacy.cleared': ' 已清除本机应用数据。',
  'privacy.confirm': '清除全部应用本地数据（自选工作区与主题偏好）？此操作不可撤销。',
} as const

export type I18nKey = keyof typeof zh

const en: Record<I18nKey, string> = {
  'app.title': 'Alpha Finance · Analytics',
  'app.intro':
    'TODO L427 selection + L428 components + L429 charts + L430 SQL workbench + L431 responsive + L432 theming + L499 live board + L508 workspace tabs. Legacy demo pages (web/index.html, etc.) coexist untouched.',
  'app.liveSection': 'Live quotes (L499: /ws subscription, sim fallback when unreachable; symbols = active workspace watchlist)',
  'app.demoSection': 'Quote demo (bundled static sample)',
  'app.language': 'Language',
  'theme.label': 'Theme',
  'theme.system': 'System',
  'theme.light': 'Light',
  'theme.dark': 'Dark',
  'ws.defaultName': 'Default',
  'ws.blankName': 'Workspace',
  'ws.symbolInvalid': 'Symbol must be 6 digits',
  'ws.symbolDup': 'Symbol already added',
  'ws.symbolCap': 'At most {max} symbols per workspace',
  'ws.deleteTab': 'Delete {name}',
  'ws.removeSymbol': 'Remove {symbol}',
  'ws.newPlaceholder': 'Workspace name',
  'ws.confirm': 'OK',
  'ws.create': '+ New',
  'ws.rename': 'Rename',
  'ws.addSymbolPlaceholder': 'Add symbol (6 digits)',
  'feed.connecting': 'Connecting to real-time-feed…',
  'feed.feed': 'Live connected',
  'feed.simulated': 'Simulated feed (feed unreachable, deterministic series)',
  'feed.seq': 'sync seq {seq}',
  'feed.hint': 'Override: ?feedWs=ws://host:port/ws',
  'feed.export': 'Export CSV',
  'feed.colSymbol': 'Symbol',
  'feed.colName': 'Name',
  'feed.colPrice': 'Last',
  'feed.colChange': 'Chg%',
  'feed.colSma': 'SMA(5)',
  'feed.colVolume': 'Volume',
  'feed.colUpdated': 'Updated',
  'feed.colSma3': 'SMA(3) last (pure TS)',
  'ind.title': 'Indicator analysis',
  'ind.symbol': 'Symbol',
  'ind.indicator': 'Indicator',
  'ind.period': 'Period',
  'ind.sma': 'SMA simple moving average',
  'ind.ema': 'EMA exponential moving average',
  'ind.last': 'Last: ',
  'ind.smaNote': '(first {n} SMA slots are 0.0 placeholders, not real values)',
  'ind.colClose': 'Close',
  'chart.title': 'Candlestick chart (lightweight-charts · Canvas, red-up/green-down)',
  'chart.caption': 'Sample candles are deterministic synthetic data (LCG seed, weekends skipped, see src/lib/demoWalk.ts); the SMA({n}) overlay matches the indicator panel.',
  'sql.title': 'SQL workbench (DuckDB-WASM)',
  'sql.idleA': 'Engine not enabled. First run ',
  'sql.idleB': ' under ',
  'sql.idleC': ', then copy ',
  'sql.idleD': ' into ',
  'sql.idleE': ', then load.',
  'sql.load': 'Load SQL engine',
  'sql.loading': 'Loading engine…',
  'sql.errorPrefix': 'Engine/query error: ',
  'sql.retry': 'Retry load',
  'sql.ready': 'Engine ready (demo tables: demo_quotes / demo_candles). Edit SQL then run:',
  'sql.run': 'Run query',
  'sql.running': 'Running…',
  'sql.summary': '{total} rows × {cols} cols · {ms} ms',
  'sql.truncated': '(showing first {n} rows only)',
  'wasm.title': 'Rust WASM engine probe',
  'wasm.idle': 'Not loaded. ',
  'wasm.load': 'Load WASM engine',
  'wasm.loading': 'Loading…',
  'wasm.loaded': 'Loaded. 600519 SMA(3) last = ',
  'wasm.sep': ', ',
  'wasm.match': 'matches pure-TS series element-wise ✔',
  'wasm.mismatch': 'MISMATCH with pure-TS series ✘ (check indicator implementation)',
  'wasm.detail': 'wasm output {a} slots / pure-TS output {b} slots (contract: same length, leading 0.0 placeholders, 4-decimal rounding)',
  'wasm.missingA': 'Not built: first run ',
  'wasm.missingB': ' under ',
  'wasm.missingC': ', then copy ',
  'wasm.missingD': ' into ',
  'wasm.missingE': ' and retry.',
  'wasm.retry': 'Retry',
  'privacy.title': 'Data & privacy (L488: local export / erase, no accounts)',
  'privacy.descA': 'App user data stays on this device (key list, ',
  'privacy.descB': ' entries: workspace watchlist, theme preference); the server keeps no personal data. Obligation mapping: ',
  'privacy.period': '.',
  'privacy.present': '(stored locally)',
  'privacy.absent': '(unused)',
  'privacy.export': 'Export my data (JSON)',
  'privacy.clear': 'Erase my data',
  'privacy.cleared': ' Local app data erased.',
  'privacy.confirm': 'Erase all local app data (workspace watchlist and theme preference)? This cannot be undone.',
}

export const STRINGS: Record<Locale, Record<I18nKey, string>> = { zh, en }

/** 查字典（缺 key 在类型层已不可能；运行时防御返 key 本身） */
export function t(locale: Locale, key: I18nKey): string {
  return STRINGS[locale][key] ?? key
}

/** 插值替换 `{name}` 占位（未传变量留占位原样，不抛错） */
export function tf(
  locale: Locale,
  key: I18nKey,
  vars: Record<string, string | number>,
): string {
  let s: string = STRINGS[locale][key] ?? key
  for (const [name, value] of Object.entries(vars)) {
    s = s.replaceAll(`{${name}}`, String(value))
  }
  return s
}

/**
 * 语言解析：`?lang=`（显式最高）→ localStorage（上次选择）→
 * `navigator.language`（`en*` 切英文，其余回中文）→ 缺省 zh。
 * 非法值一律回退不抛错（与主题/工作区解析收口同纪律）。
 */
export function parseLocale(
  search: string | undefined,
  stored: string | null,
  navigatorLanguage?: string,
): Locale {
  const pick = (raw: string | null | undefined): Locale | null => {
    const v = (raw ?? '').trim().toLowerCase()
    if (v === 'zh' || v.startsWith('zh')) return 'zh'
    if (v === 'en' || v.startsWith('en')) return 'en'
    return null
  }
  try {
    const fromQuery = pick(new URLSearchParams(search ?? '').get('lang'))
    if (fromQuery !== null) return fromQuery
  } catch {
    // search 非法直接走后续
  }
  return pick(stored) ?? pick(navigatorLanguage) ?? 'zh'
}

/** Intl 标签（数字/时间本地化用，不在字典里拼） */
export function intlTag(locale: Locale): string {
  return locale === 'en' ? 'en-US' : 'zh-CN'
}
