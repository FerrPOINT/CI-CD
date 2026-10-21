import { SpaceAdmissionBoundary } from './space-admission-boundary'
import { LoadingState } from '@pdlc/ui/ui'
// K2: layout route guard driven by AuthProvider.

import { Navigate, Outlet, useLocation } from 'react-router'
import { useAuth } from './auth-provider'
import { getPlatformProduct } from '@pdlc/ui/platform'

export function ProtectedRoute() {
  const { status, error, invalidate } = useAuth()
  const location = useLocation()

  if (status === 'checking') {
    return <LoadingState />
  }
  if (status === 'authenticated') {
    return <SpaceAdmissionBoundary><Outlet /></SpaceAdmissionBoundary>
  }
  if (status === 'unavailable') return <section role="alert"><p>{error}</p><button onClick={invalidate}>Повторить</button></section>
  if (getPlatformProduct()) {
    getPlatformProduct()?.requireLogin(location.pathname + location.search + location.hash)
    return null
  }
  return <Navigate to="/login" replace state={{ from: location.pathname + location.search }} />
}
