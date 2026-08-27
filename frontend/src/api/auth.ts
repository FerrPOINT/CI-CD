// Session/auth store (AUTHZ_CONTRACT Phase 1, frontend side).
// Keeps the access token in memory + refresh token in localStorage; wires the
// api client to attach Bearer headers and transparently refresh on 401.

const REFRESH_KEY = 'forge.refresh_token'

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

export function saveRefresh(token: string): void {
  localStorage.setItem(REFRESH_KEY, token)
}

function storedRefresh(): string | null {
  return localStorage.getItem(REFRESH_KEY)
}

export function clearSession(): void {
  session = null
  localStorage.removeItem(REFRESH_KEY)
}

/** True when the backend is expected to enforce auth. */
export async function authRequired(): Promise<boolean> {
  try {
    const res = await fetch('/api/v1/projects')
    // Enforced API returns 401; trusted-network mode returns 200/5xx.
    return res.status === 401
  } catch {
    return false
  }
}

export async function login(username: string, password: string): Promise<Session> {
  const res = await fetch('/api/v1/auth/login', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ username, password }),
  })
  if (!res.ok) throw new Error('invalid credentials')
  const pair = await res.json()
  session = { access_token: pair.access_token, expires_at: pair.expires_at, username }
  saveRefresh(pair.refresh_token)
  return session
}

/** Refresh the access token using the stored refresh token. */
export async function refresh(): Promise<Session | null> {
  const token = storedRefresh()
  if (!token) return null
  if (!refreshPromise) {
    refreshPromise = (async () => {
      try {
        const res = await fetch('/api/v1/auth/refresh', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ username: '', password: '', refresh_token: token }),
        })
        if (!res.ok) {
          clearSession()
          return null
        }
        const pair = await res.json()
        session = { access_token: pair.access_token, expires_at: pair.expires_at }
        saveRefresh(pair.refresh_token)
        return session
      } finally {
        refreshPromise = null
      }
    })()
  }
  return refreshPromise
}

export async function logout(): Promise<void> {
  // Server-side session revocation is Phase 2; locally we drop the session.
  clearSession()
}
