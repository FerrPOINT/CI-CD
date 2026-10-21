// @vitest-environment node
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// This suite checks proxy configuration. The real compiler plugins are covered
// by the production build and need not initialize for each environment case.
vi.mock('@vitejs/plugin-react', () => ({ default: () => ({ name: 'fixture-react-plugin' }) }))
vi.mock('@tailwindcss/vite', () => ({ default: () => ({ name: 'fixture-tailwind-plugin' }) }))

beforeEach(() => vi.resetModules())
afterEach(() => vi.unstubAllEnvs())

describe('workspace dev API proxy', () => {
  it.each([
    { configured: undefined, expected: 'http://localhost:22801' },
    { configured: 'http://localhost:8711', expected: 'http://localhost:8711' },
    {
      configured: 'http://localhost:8711/api/v1/',
      expected: 'http://localhost:8711',
    },
  ])('uses $expected for $configured', async ({ configured, expected }) => {
    vi.stubEnv('VITE_API_BASE_URL', configured)
    const { default: config } = await import('../vite.config')
    const proxy = config.server?.proxy?.['/api']
    const target = typeof proxy === 'string' ? proxy : proxy?.target
    expect(String(target)).toBe(expected)
  })
})
