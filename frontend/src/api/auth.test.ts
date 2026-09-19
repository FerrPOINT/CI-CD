import { beforeEach, describe, expect, it, vi } from 'vitest'

beforeEach(() => {
  vi.resetModules()
  window.localStorage.clear()
})

describe('central browser session', () => {
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
