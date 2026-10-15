// @vitest-environment node
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

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
