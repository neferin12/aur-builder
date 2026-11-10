import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'
export default defineConfig({ plugins: [vue()], test: { include: ['tests/**/*.test.ts'], fileParallelism: false, testTimeout: 30000, hookTimeout: 30000 } })
