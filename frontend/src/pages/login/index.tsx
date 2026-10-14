import { Navigate, useLocation } from 'react-router'
import { useSsoEntry } from '@sdlc/ui/auth'
import { PlatformLogin } from '@sdlc/ui/ui'
import { useAuth, ssoConfig } from '@/shared/auth/auth-provider'

export function LoginPage() {
  const location = useLocation()
  const { status } = useAuth()
  const authenticated = status === 'authenticated'
  const anonymous = status === 'anonymous'
  const returnTo = (location.state as { from?: string } | null)?.from ?? '/'
  const loggedOut = new URLSearchParams(location.search).has('logged_out')
  const entry = useSsoEntry({ config: ssoConfig, returnTo, autoStart: anonymous && !loggedOut })
  if (authenticated) return <Navigate to={returnTo} replace />
  return <PlatformLogin pending={entry.pending} error={entry.error} onSignIn={entry.signIn} />
}
