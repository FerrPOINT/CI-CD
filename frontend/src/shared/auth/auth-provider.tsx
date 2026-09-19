// K2: centralized auth state (FSD shared/auth slice).
// Wraps the imperative session store (api/auth) in a React context so
// ProtectedRoute/AppShell react to central login/logout instead of poking
// module state from effects.

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react'
import { onTerminalAuthError } from '@/api/client'
import { endSso, type SsoSession } from '@sdlc/ui/sso'
import {
  acceptSso as acceptApiSso,
  currentSession,
  logout as apiLogout,
  type Session,
} from '@/api/auth'

export type AuthStatus = 'checking' | 'authenticated' | 'anonymous'

export const ssoConfig = { issuer: import.meta.env.VITE_AUTH_ISSUER ?? 'http://localhost:7701', clientId: 'ci-cd' }

interface AuthContextValue {
  status: AuthStatus
  session: Session | null
  acceptSso: (sso: SsoSession) => void
  logout: () => Promise<void>
  /** Force a session re-check (e.g. after a terminal 401). */
  invalidate: () => void
}

const AuthContext = createContext<AuthContextValue | null>(null)

export function AuthProvider({ children }: { children: ReactNode }) {
  const [session, setSession] = useState<Session | null>(() => currentSession())
  const [status, setStatus] = useState<AuthStatus>(session ? 'authenticated' : 'checking')
  const [invalidateNonce, setInvalidateNonce] = useState(0)

  // Tokens stay in memory. A hard reload re-enters the central browser session.
  useEffect(() => {
    setStatus(currentSession() ? 'authenticated' : 'anonymous')
  }, [invalidateNonce])

  // Terminal 401 anywhere in the app: drop to anonymous (router redirects).
  useEffect(() => {
    onTerminalAuthError(() => {
      setSession(null)
      setStatus('anonymous')
    })
  }, [])

  const acceptSso = useCallback((sso: SsoSession) => {
    const next = acceptApiSso(sso.accessToken, sso.expiresAt, sso.name)
    setSession(next)
    setStatus('authenticated')
  }, [])

  const logout = useCallback(async () => {
    await apiLogout().catch(() => undefined)
    setSession(null)
    setStatus('anonymous')
    endSso(ssoConfig)
  }, [])

  const invalidate = useCallback(() => {
    setSession(currentSession())
    setStatus(currentSession() ? 'authenticated' : 'anonymous')
    setInvalidateNonce((n) => n + 1)
  }, [])

  const value = useMemo<AuthContextValue>(
    () => ({ status, session, acceptSso, logout, invalidate }),
    [status, session, acceptSso, logout, invalidate],
  )

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext)
  if (!ctx) throw new Error('useAuth must be used within <AuthProvider>')
  return ctx
}
