import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  analyzeSymbol,
  exportSymbolToFile,
  getContextMenu,
  getRealTimeQuotes,
  initializeApp,
  invoke,
  isTauriRuntime,
  saveDialog,
  setPriceAlert,
} from './desktop'

/**
 * 桌面桥接契约（web-framework-selection.md §6 拍板 A）：运行时探测纯函数
 * （注入式）+ 守卫先于传输 + v1 camelCase 参数塑形。传输层 @tauri-apps/api
 * v1 内部走 window.__TAURI_IPC__ + transformCallback（node 环境无 webview
 * 注入），模块边界 mock 是唯一稳定的测试缝。
 */
vi.mock('@tauri-apps/api/tauri', () => ({
  invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => ({ cmd, args })),
}))
vi.mock('@tauri-apps/api/dialog', () => ({
  save: vi.fn(async (options?: unknown) => options ?? null),
}))

import { invoke as tauriInvoke } from '@tauri-apps/api/tauri'
import { save as tauriSave } from '@tauri-apps/api/dialog'

/** 桌面窗口形状：与 build.withGlobalTauri 注入的最小全局一致 */
function desktopWindow(): unknown {
  return { __TAURI__: { invoke: () => Promise.resolve({}) } }
}

describe('desktop isTauriRuntime', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    vi.clearAllMocks()
  })

  it('无窗口 / 无注入全局 / invoke 缺失均非桌面，注入完整形状才是桌面', () => {
    expect(isTauriRuntime(undefined)).toBe(false)
    expect(isTauriRuntime({})).toBe(false)
    expect(isTauriRuntime({ __TAURI__: null })).toBe(false)
    expect(isTauriRuntime({ __TAURI__: {} })).toBe(false)
    expect(isTauriRuntime(desktopWindow())).toBe(true)
  })

  it('node 环境缺省（无 window）判定为非桌面', () => {
    expect(isTauriRuntime()).toBe(false)
  })
})

describe('desktop bridge guard and arg shaping', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    vi.clearAllMocks()
  })

  it('非桌面环境 invoke 先抛可读错误，不触达传输层', async () => {
    await expect(invoke('initialize_app')).rejects.toThrow('Tauri runtime not detected')
    expect(tauriInvoke).not.toHaveBeenCalled()
  })

  it('桌面环境下多词参数按 v1 camelCase 塑形（snake 运行期静默失配）', async () => {
    vi.stubGlobal('window', desktopWindow())
    await setPriceAlert('600519', 99.5, 'above')
    expect(tauriInvoke).toHaveBeenCalledWith('set_price_alert', {
      symbol: '600519',
      targetPrice: 99.5,
      alertType: 'above',
    })
    await exportSymbolToFile('600519', 'csv', '/tmp/a.csv')
    expect(tauriInvoke).toHaveBeenLastCalledWith('export_symbol_to_file', {
      symbol: '600519',
      format: 'csv',
      filePath: '/tmp/a.csv',
    })
    await getContextMenu(true, false)
    expect(tauriInvoke).toHaveBeenLastCalledWith('get_context_menu', {
      hasQuote: true,
      hasSymbols: false,
    })
    await getRealTimeQuotes(['600519', '000001'])
    expect(tauriInvoke).toHaveBeenLastCalledWith('get_real_time_quotes', {
      symbols: ['600519', '000001'],
    })
  })

  it('request 整体包装的命令透传结构，无参命令不携带参数', async () => {
    vi.stubGlobal('window', desktopWindow())
    const request = { symbol: '600519', timeframe: '1m', indicators: ['RSI'] }
    await analyzeSymbol(request)
    expect(tauriInvoke).toHaveBeenLastCalledWith('analyze_symbol', { request })
    await initializeApp()
    expect(tauriInvoke).toHaveBeenLastCalledWith('initialize_app', undefined)
  })

  it('saveDialog 守卫：非桌面先抛不触达对话框，桌面透传选项', async () => {
    await expect(saveDialog()).rejects.toThrow('Tauri runtime not detected')
    expect(tauriSave).not.toHaveBeenCalled()
    vi.stubGlobal('window', desktopWindow())
    const options = { defaultPath: '600519.csv' }
    await saveDialog(options)
    expect(tauriSave).toHaveBeenCalledWith(options)
  })
})
