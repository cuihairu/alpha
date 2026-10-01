import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// L427 Web 骨架构建配置（docs/web-framework-selection.md §5）。
// 门禁 scripts/check-web.sh 第 4 步跑 `vite build`；单测经 vitest（node 环境，
// 纯函数，不引 jsdom——DOM 渲染测试随 L428 组件化引入）。
export default defineConfig({
  plugins: [react()],
  test: {
    environment: 'node',
    include: ['src/**/*.test.{ts,tsx}'],
  },
})
