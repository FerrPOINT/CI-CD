// K2: layout route guard driven by AuthProvider.

import { Navigate, Outlet, useLocation } from 'react-router'
import { useAuth } from './auth-provider'

export function ProtectedRoute() {
  const { status } = useAuth()
  const location = useLocation()

  if (status === 'checking') {
    return <div className="flex min-h-screen items-center justify-center text-sm text-text-muted">…</div>
  }
  // Trusted-network mode: backend serves data without auth; keep pages open.
  if (status === 'open-mode' || status === 'authenticated') {
    return <Outlet />
  }
  return <Navigate to="/login" replace state={{ from: location.pathname + location.search }} />
}
