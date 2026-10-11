// Browser access tokens remain in memory; a reload re-enters Central Auth.
const LEGACY_REFRESH_KEY = 'forge.refresh_token'

export type Session = {
  access_token: string
  expires_at: number
  username?: string
  subject?: string
}

let session: Session | null = null

function purgeLegacyRefreshToken(): void {
  try {
    window.localStorage.removeItem(LEGACY_REFRESH_KEY)
  } catch {
    // Storage may be unavailable in hardened browsers.
  }
}

purgeLegacyRefreshToken()

export function currentSession(): Session | null {
  return session
}

export function clearSession(): void {
  session = null
  purgeLegacyRefreshToken()
  // Central SSO owns only the in-memory session. A host-wide legacy cookie
  // may still belong to another deployment on this hostname.
}

export function acceptSso(
  accessToken: string,
  expiresAt: number,
  username: string,
  subject?: string,
): Session {
  clearSession()
  session = {
    access_token: accessToken,
    expires_at: Math.floor(expiresAt / 1000),
    username,
    subject,
  }
  return session
}

export async function logout(): Promise<void> {
  clearSession()
}
