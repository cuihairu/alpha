/**
 * 桌面 Tauri 桥接（docs/web-framework-selection.md §6 拍板 A，2026-10-10）。
 *
 * 桌面复用 React 产物（tauri.conf.json frontendDist → web/app/dist）：Tauri 1.x
 * 经 build.withGlobalTauri 注入 window.__TAURI__，@tauri-apps/api v1 的 invoke
 * 即转发到该全局。非 Tauri 环境（浏览器/PWA）isTauriRuntime() 为 false，包装
 * 函数在调用前拒绝——调用方据此隐藏桌面专属 UI（DesktopPanel 整面隐藏），
 * 不静默失败。
 *
 * 命令名与 desktop/src/gui.rs 的 #[tauri::command] 一一对应（一致性由
 * desktop/tests/wiring_contract.rs 锁定）；Tauri 1.x 参数键默认 camelCase，
 * 多词参数必须写 targetPrice/alertType/filePath/hasQuote/hasSymbols，写 snake
 * 会在运行期静默失配。类型只声明前端消费面（Rust 结构可能更宽，serde 多给
 * 字段不碍事）；标注「壳层演示命令」的四个命令是 L112 演示壳时代的取数/导出
 * 面，React 产品自走 WS + WASM 指标，桥面保留是为 IPC 契约完整。
 */

import { save as tauriSave } from '@tauri-apps/api/dialog'
import { invoke as tauriInvoke } from '@tauri-apps/api/tauri'

/** window.__TAURI__ 全局形状（build.withGlobalTauri 注入，v1 语义） */
interface TauriGlobal {
  invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>
  dialog?: { save?: (options?: unknown) => Promise<string | null> }
}

/** 运行时探测：窗口对象缺失（node/SSR）或未注入全局均视为非桌面；
 * 窗口形状显式入参（测试不经全局，与仓内注入式测试约定一致） */
export function isTauriRuntime(
  win: unknown = typeof window === 'undefined' ? undefined : window,
): boolean {
  const t = (win as { __TAURI__?: unknown } | undefined)?.__TAURI__
  return (
    typeof t === 'object' &&
    t !== null &&
    typeof (t as TauriGlobal).invoke === 'function'
  )
}

const NOT_RUNTIME = 'Tauri runtime not detected: desktop bridge calls are desktop-only'

/** 守卫后的 invoke：非桌面环境拒绝（恒返 Promise，调用方统一 .catch），
 * 不落到 undefined 解引用 */
export function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauriRuntime()) return Promise.reject(new Error(NOT_RUNTIME))
  return tauriInvoke<T>(cmd, args)
}

/** 原生「另存为」对话框（L113）：取消返回 null */
export function saveDialog(options?: {
  defaultPath?: string
  filters?: { name: string; extensions: string[] }[]
}): Promise<string | null> {
  if (!isTauriRuntime()) return Promise.reject(new Error(NOT_RUNTIME))
  return tauriSave(options)
}

// —— IPC 契约类型（镜像 desktop/src 框架层 serde 结构，字段名即序列化键） ——

/** `initialize_app` 应答里的配置（镜像 config::AppConfig） */
export interface DesktopAppConfig {
  api_url: string
  symbols: string[]
  /** light / dark / system */
  theme: string
  auto_update: boolean
}

/** `initialize_app` 应答（镜像 ipc::InitPayload） */
export interface InitPayload {
  config: DesktopAppConfig
  /** defaults / file / recovered */
  source: string
  /** 配置校验问题（空 = 无问题，提示但不阻断） */
  validation: string[]
}

/** `get_app_info` 应答（镜像 app::AppInfo） */
export interface AppInfo {
  name: string
  version: string
  os: string
  arch: string
  identifier: string
}

/** 行情快照（消费面子集；壳层演示命令 get_real_time_quotes 返回） */
export interface DesktopQuote {
  symbol: string
  price: number
  open: number
  volume: number
}

/** `export_symbol_to_file` 应答 */
export interface ExportOutcome {
  rows: number
  filename: string
}

/** 单条通知（镜像 notify::Notification） */
export interface DesktopNotification {
  id: string
  symbol: string
  title: string
  body: string
  /** info / warning / critical */
  level: string
  created_at: string
}

/** 托盘状态（镜像 notify::TrayState） */
export interface TrayState {
  status_text: string
  active_alerts: number
  last_trigger: string | null
}

/** 离线行情载荷（镜像 offline::QuotesPayload） */
export interface OfflineQuotesPayload {
  online: boolean
  quotes: (DesktopQuote & { source: string })[]
  missing: string[]
}

/** 增量同步报告（镜像 offline::SyncReport） */
export interface SyncReport {
  online: boolean
  applied: string[]
  unchanged: number
  failed: string[]
  synced_at: string
}

/** 右键菜单项（镜像 shortcuts::ContextMenuItem） */
export interface ContextMenuItem {
  id: string
  label: string
  hint: string | null
  enabled: boolean
}

/** 分析结果（消费面子集；壳层演示命令 analyze_symbol 返回） */
export interface DesktopAnalysis {
  symbol: string
  recommendation: string
  confidence: number
  risk_metrics: { volatility: number }
  indicators: { name: string; values: number[] }[]
}

// —— 命令包装（名字与 gui.rs 一一对应；参数键 camelCase） ——

/** 启动自举：解析平台目录 → 框架层配置自举 → 注入状态 */
export function initializeApp(): Promise<InitPayload> {
  return invoke('initialize_app')
}

/** 应用元信息 */
export function getAppInfo(): Promise<AppInfo> {
  return invoke('get_app_info')
}

/** 批量快照行情（壳层演示命令：React 产品行情走 real-time-feed WS） */
export function getRealTimeQuotes(symbols: string[]): Promise<DesktopQuote[]> {
  return invoke('get_real_time_quotes', { symbols })
}

/** 分析单个标的（壳层演示命令：React 产品指标走 WASM 探针） */
export function analyzeSymbol(request: {
  symbol: string
  timeframe: string
  indicators: string[]
}): Promise<DesktopAnalysis> {
  return invoke('analyze_symbol', { request })
}

/** 新增价格告警，返回告警 id */
export function setPriceAlert(
  symbol: string,
  targetPrice: number,
  alertType: string,
): Promise<string> {
  return invoke('set_price_alert', { symbol, targetPrice, alertType })
}

/** 导出数据（壳层演示命令：确定性引擎全量导出，返回文件名列表） */
export function exportData(request: {
  symbols: string[]
  format: string
}): Promise<string[]> {
  return invoke('export_data', { request })
}

/**
 * 导出单个标的到用户自选路径（前端先经 saveDialog 拿路径）。
 * 目标文件已存在时 Rust 侧弹原生覆盖确认；用户取消返回 `null`（同 saveDialog
 * 取消口径，不落盘）。
 */
export function exportSymbolToFile(
  symbol: string,
  format: string,
  filePath: string,
): Promise<ExportOutcome | null> {
  return invoke('export_symbol_to_file', { symbol, format, filePath })
}

/** 发送系统通知（框架层校验 + 入队） */
export function sendNotification(request: {
  symbol: string
  title: string
  body: string
  level: string
}): Promise<DesktopNotification> {
  return invoke('send_notification', { request })
}

/** 最近通知队列 */
export function listNotifications(limit: number): Promise<DesktopNotification[]> {
  return invoke('list_notifications', { limit })
}

/** 更新托盘 tooltip，返回托盘状态 */
export function setTrayStatus(): Promise<TrayState> {
  return invoke('set_tray_status')
}

/** 检查告警并弹出触发通知，返回本次触发的通知 */
export function checkAlerts(): Promise<DesktopNotification[]> {
  return invoke('check_alerts')
}

/** 离线感知行情读取（降级矩阵全在 Rust 框架层） */
export function getOfflineQuotes(symbols: string[]): Promise<OfflineQuotesPayload> {
  return invoke('get_offline_quotes', { symbols })
}

/** 联网恢复增量同步（离线返回空报告，降级非错误） */
export function syncOfflineData(symbols: string[]): Promise<SyncReport> {
  return invoke('sync_offline_data', { symbols })
}

/** 右键菜单模型（按界面状态算可用性/加速器提示） */
export function getContextMenu(
  hasQuote: boolean,
  hasSymbols: boolean,
): Promise<ContextMenuItem[]> {
  return invoke('get_context_menu', { hasQuote, hasSymbols })
}
