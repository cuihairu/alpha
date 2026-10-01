/**
 * PWA 支撑纯函数面（L507 Web PWA 支持和离线功能）：
 * 缓存策略判定 + service worker 注册薄壳。策略规则镜像 public/sw.js
 * 的 cacheStrategy（SW 自包含不引共享 chunk，两侧注释互指，语义由
 * pwa.test.ts 锁定）；注册薄壳沿 theme.ts 模式（异常兜底不阻塞应用）。
 */

export type CacheStrategy = 'network-first' | 'cache-first' | 'none'

export const SW_PATH = 'sw.js'

/**
 * 请求 → 缓存策略：同源 GET 才接管（'none' = SW 不 respondWith）；
 * 页面导航走网络优先（离线回退缓存的壳），其余同源资产缓存优先。
 */
export function cacheStrategy(input: { method: string; sameOrigin: boolean; destination: string }): CacheStrategy {
  if (input.method !== 'GET' || !input.sameOrigin) return 'none'
  if (input.destination === 'document') return 'network-first'
  return 'cache-first'
}

/** SW 注册条件收口：仅浏览器支持时注册（http(s) 环境，file:// 无 SW） */
export function shouldRegisterServiceWorker(navigatorLike: { serviceWorker?: unknown }): boolean {
  return navigatorLike.serviceWorker !== undefined
}

/** 注册 service worker（失败静默——离线能力是增强不是依赖） */
export function registerServiceWorker(): void {
  if (!shouldRegisterServiceWorker(navigator)) return
  navigator.serviceWorker.register(SW_PATH).catch(() => {
    // 注册失败（私有模式/存储被禁/sw.js 不可达）：应用照常运行
  })
}
