import { useCallback, type ReactNode } from 'react'
import { AccessFreshnessBoundary } from '@pdlc/ui/ui'
import { api } from '@/api/client'
import { useAuth } from './auth-provider'
const load = (signal: AbortSignal) => api<unknown>('/space-admission', { signal, cache: 'no-store' })
export function SpaceAdmissionBoundary({ children }: { children: ReactNode }) {
  const { status, accessVersion, retireAccess } = useAuth()
  const retire = useCallback(() => retireAccess(accessVersion), [accessVersion, retireAccess])
  return <AccessFreshnessBoundary enabled={status === 'authenticated'} authorityKey={accessVersion} load={load} onRetire={retire}>{children}</AccessFreshnessBoundary>
}
