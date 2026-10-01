import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import { registerServiceWorker } from './lib/pwa'
import './styles.css'

// L427 骨架挂载点（docs/web-framework-selection.md §4）；L431 起挂响应式样式
ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)

// L507 PWA：离线壳 + 资产缓存（失败静默，离线能力是增强不是依赖）
registerServiceWorker()
