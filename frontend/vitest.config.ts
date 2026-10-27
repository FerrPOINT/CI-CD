import { defineConfig, mergeConfig } from 'vitest/config'
import viteConfig from './vite.config'

export default mergeConfig(
  viteConfig,
  defineConfig({
    test: {
      environment: 'jsdom',
      maxWorkers: 4,
      include: ['src/**/*.{test,spec}.{ts,tsx}'],
      setupFiles: ['./src/test-setup.ts'],
    },
  }),
)
