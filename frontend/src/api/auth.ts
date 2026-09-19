// Browser access tokens remain in memory; a reload re-enters Central Auth.
const LEGACY_REFRESH_KEY = 'forge.refresh_token'

export type Session = {
  access_token: string
  expires_at: number
  username?: string
}

let session: Session | null = null

export function currentSession(): Session | null {
  return session
}

export function clearSession(): void {
  session = null
  try {
    window.localStorage.removeItem(LEGACY_REFRESH_KEY)
  } catch {
    // Storage may be unavailable in hardened browsers.
  }
  if (typeof document !== 'undefined') {
    document.cookie = 'forge_csrf=; Path=/; Max-Age=0; SameSite=Lax'
  }
}

export function acceptSso(accessToken: string, expiresAt: number, username: string): Session {
  clearSession()
  session = { access_token: accessToken, expires_at: Math.floor(expiresAt / 1000), username }
  return session
}

export async function logout(): Promise<void> {
  clearSession()
}
