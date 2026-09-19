import { useEffect, useState } from 'react'
import { Navigate, useLocation } from 'react-router'
import { beginSso } from '@sdlc/ui/sso'
import { Button, PlatformMark } from '@sdlc/ui/ui'
import { ssoConfig, useAuth } from '@/shared/auth/auth-provider'

export function LoginPage() {
  const { status } = useAuth()
  const location = useLocation()
  const [error, setError] = useState<string | null>(null)
  const returnTo = (location.state as { from?: string } | null)?.from ?? '/'
  const loggedOut = new URLSearchParams(location.search).has('logged_out')
  useEffect(() => {
    if (status !== 'anonymous' || loggedOut) return
    void beginSso(ssoConfig, returnTo).catch(() => setError('Central Auth временно недоступен.'))
  }, [status, loggedOut, returnTo])
  if (status === 'authenticated') return <Navigate to={returnTo} replace />
  return <main className="grid min-h-screen place-items-center bg-background p-4">
    <div className="w-full max-w-sm space-y-5 text-center">
      <PlatformMark withName />
      <h1 className="text-xl font-semibold">Вход в CI/CD</h1>
      {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
      <Button className="w-full" onClick={() => void beginSso(ssoConfig, returnTo).catch(() => setError('Central Auth временно недоступен.'))}>Войти через SDLC</Button>
    </div>
  </main>
}
