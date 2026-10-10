import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

beforeEach(() => {
  vi.resetModules()
  window.localStorage.clear()
})

afterEach(() => {
  document.cookie = 'forge_csrf=; Path=/; Max-Age=0'
  document.cookie = 'neighbor_cookie=; Path=/; Max-Age=0'
})

describe('central browser session', () => {
  it.each(['legacy-session-a', 'legacy-session-b'])(
    'preserves another legacy cookie owner during SSO and logout: %s',
    async (value) => {
      document.cookie = `forge_csrf=${value}; Path=/; SameSite=Lax`
      document.cookie = 'neighbor_cookie=retained; Path=/; SameSite=Lax'
      const { acceptSso, currentSession, logout } = await import('./auth')
      acceptSso('central-access', Date.now() + 60_000, 'qa-user')
      expect(document.cookie).toContain(`forge_csrf=${value}`)
      expect(document.cookie).toContain('neighbor_cookie=retained')
      await logout()
      expect(currentSession()).toBeNull()
      expect(document.cookie).toContain(`forge_csrf=${value}`)
      expect(document.cookie).toContain('neighbor_cookie=retained')
    },
  )
  it('purges a legacy refresh token on startup without a successful SSO callback', async () => {
    window.localStorage.setItem('forge.refresh_token', 'old-secret')
    window.localStorage.setItem('theme', 'light')

    const { currentSession } = await import('./auth')

    expect(currentSession()).toBeNull()
    expect(window.localStorage.getItem('forge.refresh_token')).toBeNull()
    expect(window.localStorage.getItem('theme')).toBe('light')
  })

  it('keeps the access token in memory and removes legacy credentials', async () => {
    window.localStorage.setItem('forge.refresh_token', 'old-secret')
    const { acceptSso, currentSession } = await import('./auth')

    const session = acceptSso('central-access', Date.now() + 60_000, 'qa-user')

    expect(currentSession()).toEqual(session)
    expect(window.localStorage.getItem('forge.refresh_token')).toBeNull()
    expect(JSON.stringify(window.localStorage)).not.toContain('central-access')
  })

  it('clears the local token without calling a legacy login/logout endpoint', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)
    const { acceptSso, currentSession, logout } = await import('./auth')
    acceptSso('central-access', Date.now() + 60_000, 'qa-user')

    await logout()

    expect(currentSession()).toBeNull()
    expect(fetchMock).not.toHaveBeenCalled()
    vi.unstubAllGlobals()
  })
})
