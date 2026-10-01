import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import './styles.css'

// L427 骨架挂载点（docs/web-framework-selection.md §4）；L431 起挂响应式样式
ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)
