import { defineConfig } from '@playwright/test'

process.env.FORGE_SYNC_HISTORY_FIXTURE = '1'
const baseURL = new URL(process.env.E2E_BASE_URL ?? 'http://127.0.0.1:18935')
if (baseURL.protocol !== 'http:' || baseURL.hostname !== '127.0.0.1'
  || !/^\d+$/.test(baseURL.port) || Number(baseURL.port) < 1024) {
  throw new Error('An explicit unoccupied loopback fixture port is required')
}

export default defineConfig({
  testDir: './e2e', testMatch: ['repository-sync-history.spec.ts', 'repository-sync-refs.spec.ts'], workers: 1,
  timeout: 45_000, expect: { timeout: 10_000 }, reporter: 'list',
  outputDir: 'test-results/sync-history',
  use: { baseURL: baseURL.origin, locale: 'ru-RU', trace: 'retain-on-failure' },
  webServer: {
    command: `pnpm exec vite --host 127.0.0.1 --port ${baseURL.port} --strictPort`,
    url: baseURL.origin + '/e2e/fixtures/repository-sync-history.html',
    reuseExistingServer: false,
    timeout: 60_000,
    env: { VITE_API_BASE_URL: 'http://127.0.0.1:1' },
  },
  projects: ['chromium', 'firefox', 'webkit'].map(browserName => ({
    name: browserName, use: { browserName: browserName as 'chromium' | 'firefox' | 'webkit' },
  })),
})
