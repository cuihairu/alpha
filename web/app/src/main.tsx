import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'

// L427 骨架挂载点（docs/web-framework-selection.md §4）
ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)
