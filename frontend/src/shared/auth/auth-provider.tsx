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
  useRef,
  useState,
  type ReactNode,
} from 'react'
import { onTerminalAuthError } from '@/api/client'
import { applySsoSession, endSso, restoreSso, isSsoLoginRequired, isSsoNavigationInterruption, type SsoSession } from '@sdlc/ui/sso'
import { getPlatformProduct } from '@sdlc/ui/platform'
import {
  acceptSso as acceptApiSso,
  currentSession,
  type Session,
} from '@/api/auth'

export type AuthStatus = 'checking' | 'authenticated' | 'anonymous' | 'unavailable'

export const ssoConfig = {
  issuer: import.meta.env.VITE_AUTH_ISSUER ?? 'http://localhost:7701',
  clientId: 'ci-cd',
}

interface AuthContextValue {
  error: string | null
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
  const [error, setError] = useState<string | null>(null)
  const generation = useRef(0)

  // Tokens stay in memory. A hard reload re-enters the central browser session.
  useEffect(() => {
    let active = true
    const epoch = generation.current
    if (currentSession()) { setStatus('authenticated'); return }
    if (['/login', '/sso/callback'].includes(window.location.pathname)) { setStatus('anonymous'); return }
    setStatus('checking')
    void restoreSso(ssoConfig).then(sso => {
      if (!active || epoch !== generation.current) return
      applySsoSession(sso, () => {
        const next = acceptApiSso(sso.accessToken, sso.expiresAt, sso.name, sso.subject)
        setSession(next); setStatus('authenticated')
      })
    }).catch(caught => {
      if (!active || epoch !== generation.current || isSsoNavigationInterruption(caught)) return
      if (isSsoLoginRequired(caught)) setStatus('anonymous')
      else { const message = 'Не удалось восстановить сессию CI/CD.'; setError(message); setStatus('unavailable'); getPlatformProduct()?.readiness({ ready: false, error: message }) }
    })
    return () => { active = false }
  }, [invalidateNonce])

  // Terminal 401 anywhere in the app: drop to anonymous (router redirects).
  useEffect(() => {
    onTerminalAuthError(() => {
      if (getPlatformProduct()) { getPlatformProduct()?.recheckAuthorization(); return }
      generation.current++
      setSession(null)
      setStatus('anonymous')
    })
  }, [])

  const acceptSso = useCallback((sso: SsoSession) => {
    generation.current++
    applySsoSession(sso, () => {
      const next = acceptApiSso(sso.accessToken, sso.expiresAt, sso.name, sso.subject)
      setError(null); setSession(next); setStatus('authenticated')
    })
  }, [])

  const logout = useCallback(async () => {
    // Keep pending API requests authenticated until Central Auth owns navigation.
    endSso(ssoConfig)
  }, [])

  const invalidate = useCallback(() => {
    setError(null)
    setSession(currentSession())
    setStatus(currentSession() ? 'authenticated' : 'checking')
    setInvalidateNonce((n) => n + 1)
  }, [])

  const value = useMemo<AuthContextValue>(
    () => ({ status, session, error, acceptSso, logout, invalidate }),
    [status, session, error, acceptSso, logout, invalidate],
  )

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext)
  if (!ctx) throw new Error('useAuth must be used within <AuthProvider>')
  return ctx
}
