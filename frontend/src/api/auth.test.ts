import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

class MemoryStorage implements Storage {
  private readonly items = new Map<string, string>()

  get length(): number {
    return this.items.size
  }

  clear(): void {
    this.items.clear()
  }

  getItem(key: string): string | null {
    return this.items.get(key) ?? null
  }

  key(index: number): string | null {
    return Array.from(this.items.keys())[index] ?? null
  }

  removeItem(key: string): void {
    this.items.delete(key)
  }

  setItem(key: string, value: string): void {
    this.items.set(key, value)
  }
}

beforeEach(() => {
  vi.resetModules()
  const storage = new MemoryStorage()
  Object.defineProperty(window, 'localStorage', {
    configurable: true,
    value: storage,
  })
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    value: storage,
  })
  document.cookie = 'forge_csrf=; Path=/; Max-Age=0; SameSite=Lax'
})

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('browser auth session', () => {
  it('[REQ-AUTH-001] logs in without storing the refresh token in localStorage', async () => {
    const fetchMock = vi.fn().mockResolvedValue(tokenPairResponse())
    const setItem = vi.spyOn(window.localStorage, 'setItem')
    vi.stubGlobal('fetch', fetchMock)

    const { currentSession, login } = await import('./auth')
    const session = await login('admin', 'IntegrationPass1!')

    expect(session.access_token).toBe('access-token')
    expect(currentSession()?.access_token).toBe('access-token')
    expect(window.localStorage.getItem('forge.refresh_token')).toBeNull()
    expect(setItem).not.toHaveBeenCalledWith('forge.refresh_token', expect.any(String))
    const init = fetchMock.mock.calls[0][1] as RequestInit
    expect(fetchMock.mock.calls[0][0]).toBe('/api/v1/auth/login')
    expect(init.credentials).toBe('same-origin')
    expect(JSON.parse(init.body as string)).toEqual({
      username: 'admin',
      password: 'IntegrationPass1!',
    })
  })

  it('[REQ-AUTH-001] refreshes with CSRF proof and an empty body token', async () => {
    document.cookie = 'forge_csrf=csrf-token; Path=/'
    const fetchMock = vi.fn().mockResolvedValue(tokenPairResponse())
    vi.stubGlobal('fetch', fetchMock)

    const { refresh } = await import('./auth')
    const session = await refresh()

    expect(session?.access_token).toBe('access-token')
    const init = fetchMock.mock.calls[0][1] as RequestInit
    expect(fetchMock.mock.calls[0][0]).toBe('/api/v1/auth/refresh')
    expect(init.credentials).toBe('same-origin')
    expect(JSON.parse(init.body as string)).toEqual({ refresh_token: '' })
    expect((init.headers as Headers).get('X-CSRF-Token')).toBe('csrf-token')
  })

  it('[REQ-AUTH-001] migrates one legacy localStorage refresh token then clears it', async () => {
    window.localStorage.setItem('forge.refresh_token', 'legacy-refresh-token')
    const fetchMock = vi.fn().mockResolvedValue(tokenPairResponse())
    vi.stubGlobal('fetch', fetchMock)

    const { refresh } = await import('./auth')
    const session = await refresh()

    expect(session?.access_token).toBe('access-token')
    const init = fetchMock.mock.calls[0][1] as RequestInit
    expect(JSON.parse(init.body as string)).toEqual({ refresh_token: 'legacy-refresh-token' })
    expect((init.headers as Headers).has('X-CSRF-Token')).toBe(false)
    expect(window.localStorage.getItem('forge.refresh_token')).toBeNull()
  })

  it('[REQ-AUTH-001] logs out with CSRF proof and clears browser session state', async () => {
    document.cookie = 'forge_csrf=logout-csrf; Path=/'
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({ revoked: true }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    }))
    vi.stubGlobal('fetch', fetchMock)

    const { currentSession, logout } = await import('./auth')
    await logout()

    const init = fetchMock.mock.calls[0][1] as RequestInit
    expect(fetchMock.mock.calls[0][0]).toBe('/api/v1/auth/logout')
    expect(init.credentials).toBe('same-origin')
    expect(JSON.parse(init.body as string)).toEqual({ refresh_token: '' })
    expect((init.headers as Headers).get('X-CSRF-Token')).toBe('logout-csrf')
    expect(currentSession()).toBeNull()
    expect(document.cookie).not.toContain('forge_csrf=')
  })

  it('[REQ-AUTH-001] skips refresh when no cookie or legacy token exists', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)

    const { refresh } = await import('./auth')

    await expect(refresh()).resolves.toBeNull()
    expect(fetchMock).not.toHaveBeenCalled()
  })
})

function tokenPairResponse(): Response {
  return new Response(JSON.stringify({
    access_token: 'access-token',
    expires_at: 1_798_759_000,
    refresh_token: 'refresh-token',
  }), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}
