import { describe, expect, it } from 'vitest'
import { cacheStrategy, shouldRegisterServiceWorker } from './pwa'

/**
 * PWA 策略语义（L507）：public/sw.js 的 cacheStrategy 与此镜像同步
 * （同源 GET 才接管；导航网络优先，其余缓存优先）。注册薄壳的
 * 支持判定（浏览器面 register 本体不进单测）。
 */
describe('pwa cacheStrategy', () => {
  it('非 GET 或跨源请求不接管', () => {
    expect(cacheStrategy({ method: 'POST', sameOrigin: true, destination: '' })).toBe('none')
    expect(cacheStrategy({ method: 'PUT', sameOrigin: true, destination: 'document' })).toBe('none')
    expect(cacheStrategy({ method: 'GET', sameOrigin: false, destination: 'document' })).toBe('none')
    expect(cacheStrategy({ method: 'GET', sameOrigin: false, destination: 'script' })).toBe('none')
  })

  it('页面导航网络优先，其余同源资产缓存优先', () => {
    expect(cacheStrategy({ method: 'GET', sameOrigin: true, destination: 'document' })).toBe(
      'network-first',
    )
    expect(cacheStrategy({ method: 'GET', sameOrigin: true, destination: 'script' })).toBe(
      'cache-first',
    )
    expect(cacheStrategy({ method: 'GET', sameOrigin: true, destination: 'style' })).toBe(
      'cache-first',
    )
    expect(cacheStrategy({ method: 'GET', sameOrigin: true, destination: 'image' })).toBe(
      'cache-first',
    )
    expect(cacheStrategy({ method: 'GET', sameOrigin: true, destination: '' })).toBe('cache-first')
  })

  it('注册支持判定：无 serviceWorker 属性则不注册', () => {
    expect(shouldRegisterServiceWorker({})).toBe(false)
    expect(shouldRegisterServiceWorker({ serviceWorker: {} })).toBe(true)
  })
})
