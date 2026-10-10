// K2: layout route guard driven by AuthProvider.

import { Navigate, Outlet, useLocation } from 'react-router'
import { useAuth } from './auth-provider'
import { getPlatformProduct } from '@sdlc/ui/platform'

export function ProtectedRoute() {
  const { status, error, invalidate } = useAuth()
  const location = useLocation()

  if (status === 'checking') {
    return <div className="flex min-h-screen items-center justify-center text-sm text-text-muted">…</div>
  }
  if (status === 'authenticated') {
    return <Outlet />
  }
  if (status === 'unavailable') return <section role="alert"><p>{error}</p><button onClick={invalidate}>Повторить</button></section>
  if (getPlatformProduct()) {
    getPlatformProduct()?.requireLogin(location.pathname + location.search + location.hash)
    return null
  }
  return <Navigate to="/login" replace state={{ from: location.pathname + location.search }} />
}
