// Session/auth store (AUTHZ_CONTRACT Phase 1, frontend side).
// Keeps the access token in memory and relies on the backend HttpOnly refresh
// cookie plus a same-site CSRF companion cookie for browser refresh/logout.

const LEGACY_REFRESH_KEY = 'forge.refresh_token'
const CSRF_COOKIE = 'forge_csrf'

export type Session = {
  access_token: string
  expires_at: number
  username?: string
}

let session: Session | null = null
let refreshPromise: Promise<Session | null> | null = null

export function currentSession(): Session | null {
  return session
}

export function clearSession(): void {
  session = null
  clearLegacyRefresh()
  expireCsrfCookie()
}

/** True when the backend is expected to enforce auth. */
export async function authRequired(): Promise<boolean> {
  try {
    const res = await fetch('/api/v1/projects', { credentials: 'same-origin' })
    // Enforced API returns 401; trusted-network mode returns 200/5xx.
    return res.status === 401
  } catch {
    return false
  }
}

export async function login(username: string, password: string): Promise<Session> {
  const res = await fetch('/api/v1/auth/login', {
    method: 'POST',
    credentials: 'same-origin',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ username, password }),
  })
  if (!res.ok) throw new Error('invalid credentials')
  const pair = await res.json()
  session = { access_token: pair.access_token, expires_at: pair.expires_at, username }
  clearLegacyRefresh()
  return session
}

/** Refresh the access token using the HttpOnly refresh cookie + CSRF proof. */
export async function refresh(): Promise<Session | null> {
  const legacyRefresh = legacyStoredRefresh()
  const csrf = csrfToken()
  if (!legacyRefresh && !csrf) {
    clearSession()
    return null
  }
  if (!refreshPromise) {
    refreshPromise = (async () => {
      try {
        const res = await fetch('/api/v1/auth/refresh', authMutationInit({
          refresh_token: legacyRefresh ?? '',
        }, csrf))
        if (!res.ok) {
          clearSession()
          return null
        }
        const pair = await res.json()
        session = { access_token: pair.access_token, expires_at: pair.expires_at }
        clearLegacyRefresh()
        return session
      } finally {
        refreshPromise = null
      }
    })()
  }
  return refreshPromise
}

export async function logout(): Promise<void> {
  const legacyRefresh = legacyStoredRefresh()
  const csrf = csrfToken()
  try {
    if (legacyRefresh || csrf) {
      await fetch('/api/v1/auth/logout', authMutationInit({
        refresh_token: legacyRefresh ?? '',
      }, csrf))
    }
  } finally {
    clearSession()
  }
}

function authMutationInit(body: { refresh_token: string }, csrf: string | null): RequestInit {
  const headers = new Headers({ 'Content-Type': 'application/json' })
  if (csrf) headers.set('X-CSRF-Token', csrf)
  return {
    method: 'POST',
    credentials: 'same-origin',
    headers,
    body: JSON.stringify(body),
  }
}

function csrfToken(): string | null {
  if (typeof document === 'undefined') return null
  const prefix = `${CSRF_COOKIE}=`
  return document.cookie
    .split(';')
    .map(part => part.trim())
    .find(part => part.startsWith(prefix))
    ?.slice(prefix.length) ?? null
}

function expireCsrfCookie(): void {
  if (typeof document === 'undefined') return
  document.cookie = `${CSRF_COOKIE}=; Path=/; Max-Age=0; SameSite=Lax`
}

function legacyStoredRefresh(): string | null {
  try {
    return window.localStorage.getItem(LEGACY_REFRESH_KEY)
  } catch {
    return null
  }
}

function clearLegacyRefresh(): void {
  try {
    window.localStorage.removeItem(LEGACY_REFRESH_KEY)
  } catch {
    // Storage can be unavailable in hardened browsers; auth still works via cookies.
  }
}
