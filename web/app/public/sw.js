/*
 * L507 PWA service worker：预缓存应用壳 + 运行时资产缓存。
 *
 * 缓存策略规则镜像 `src/lib/pwa.ts` 的 cacheStrategy（SW 入口自包含、
 * 不引共享 chunk——vite 多入口共享 chunk 带哈希，离线预缓存路径不可预知；
 * 语义由 pwa.test.ts 在 TS 侧锁定，本文件与其保持同步）。
 *
 * 离线行为：导航请求网络优先、失败回退缓存的壳（'/'）——离线时应用
 * 可整页打开，面板层各自的降级路径照常（实时看板 → 确定性模拟盘等）；
 * 同源静态资产（/assets/*、manifest、图标）缓存优先 + 后台填充。
 */
const SHELL_CACHE = 'alpha-shell-v1'
const ASSET_CACHE = 'alpha-assets-v1'
const SHELL_URLS = ['/', '/manifest.webmanifest', '/icon.svg']
const MAX_ASSET_ENTRIES = 60

/** 镜像 lib/pwa.ts cacheStrategy：同源 GET 才接管；导航网络优先，其余缓存优先 */
function cacheStrategy(request) {
  const url = new URL(request.url)
  if (request.method !== 'GET' || url.origin !== self.location.origin) return 'none'
  if (request.destination === 'document') return 'network-first'
  return 'cache-first'
}

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches
      .open(SHELL_CACHE)
      .then((cache) => cache.addAll(SHELL_URLS))
      .then(() => self.skipWaiting()),
  )
})

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(
          keys
            .filter((key) => key !== SHELL_CACHE && key !== ASSET_CACHE)
            .map((key) => caches.delete(key)),
        ),
      )
      .then(() => self.clients.claim()),
  )
})

/** 运行时缓存封顶（防无界增长）：先进先出淘汰 */
async function trimCache(cacheName, maxEntries) {
  const cache = await caches.open(cacheName)
  const keys = await cache.keys()
  if (keys.length <= maxEntries) return
  for (const key of keys.slice(0, keys.length - maxEntries)) {
    await cache.delete(key)
  }
}

self.addEventListener('fetch', (event) => {
  const request = event.request
  if (cacheStrategy(request) === 'none') return

  if (request.destination === 'document') {
    // 网络优先：在线直连（成功顺手刷新壳缓存）；离线回退缓存的壳
    event.respondWith(
      fetch(request)
        .then((response) => {
          if (response.ok) {
            const copy = response.clone()
            caches
              .open(SHELL_CACHE)
              .then((cache) => cache.put('/', copy))
              .catch(() => {})
          }
          return response
        })
        .catch(() =>
          caches
            .match('/', { ignoreSearch: true })
            .then((cached) => cached || Response.error()),
        ),
    )
    return
  }

  // 缓存优先：未命中再取网络并填充运行时缓存
  event.respondWith(
    caches.match(request).then((cached) => {
      if (cached) return cached
      return fetch(request).then((response) => {
        if (response.ok) {
          const copy = response.clone()
          caches
            .open(ASSET_CACHE)
            .then((cache) => cache.put(request, copy))
            .then(() => trimCache(ASSET_CACHE, MAX_ASSET_ENTRIES))
            .catch(() => {})
        }
        return response
      })
    }),
  )
})
