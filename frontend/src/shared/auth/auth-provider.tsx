// K2: centralized auth state (FSD shared/auth slice).
// Wraps the imperative session store (api/auth) in a React context so
// ProtectedRoute/AppShell react to login/logout/refresh instead of poking
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
import {
  authRequired,
  currentSession,
  login as apiLogin,
  logout as apiLogout,
  refresh as apiRefresh,
  type Session,
} from '@/api/auth'

export type AuthStatus = 'checking' | 'authenticated' | 'anonymous' | 'open-mode'

interface AuthContextValue {
  status: AuthStatus
  session: Session | null
  login: (username: string, password: string) => Promise<void>
  logout: () => Promise<void>
  /** Force a session re-check (e.g. after a terminal 401). */
  invalidate: () => void
}

const AuthContext = createContext<AuthContextValue | null>(null)

export function AuthProvider({ children }: { children: ReactNode }) {
  const [session, setSession] = useState<Session | null>(() => currentSession())
  const [status, setStatus] = useState<AuthStatus>(session ? 'authenticated' : 'checking')
  const invalidatedRef = useRef(0)

  // Bootstrap: restore session via cookie refresh, else probe whether the
  // backend enforces auth at all (trusted-network mode keeps pages public).
  useEffect(() => {
    let cancelled = false
    void (async () => {
      if (currentSession()) return // already logged in this tab
      const restored = await apiRefresh().catch(() => null)
      if (cancelled) return
      if (restored || currentSession()) {
        setSession(currentSession())
        setStatus('authenticated')
        return
      }
      const required = await authRequired().catch(() => false)
      if (cancelled) return
      setStatus(required ? 'anonymous' : 'open-mode')
    })()
    return () => {
      cancelled = true
    }
  }, [invalidatedRef.current])

  // Terminal 401 anywhere in the app: drop to anonymous (router redirects).
  useEffect(() => {
    onTerminalAuthError(() => {
      setSession(null)
      setStatus((prev) => (prev === 'open-mode' ? prev : 'anonymous'))
    })
  }, [])

  const login = useCallback(async (username: string, password: string) => {
    const next = await apiLogin(username, password)
    setSession(next)
    setStatus('authenticated')
  }, [])

  const logout = useCallback(async () => {
    await apiLogout().catch(() => undefined)
    setSession(null)
    setStatus('anonymous')
  }, [])

  const invalidate = useCallback(() => {
    setSession(currentSession())
    setStatus(currentSession() ? 'authenticated' : 'anonymous')
    invalidatedRef.current += 1
  }, [])

  const value = useMemo<AuthContextValue>(
    () => ({ status, session, login, logout, invalidate }),
    [status, session, login, logout, invalidate],
  )

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext)
  if (!ctx) throw new Error('useAuth must be used within <AuthProvider>')
  return ctx
}
