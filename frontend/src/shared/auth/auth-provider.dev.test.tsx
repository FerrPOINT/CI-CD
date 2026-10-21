import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const session = vi.hoisted(() => ({
  current: null as null | { access_token: string; expires_at: number },
}))
const sso = vi.hoisted(() => ({ restoreSso: vi.fn(), endSso: vi.fn() }))
vi.mock('@/api/auth', () => ({
  currentSession: () => session.current,
  acceptSso: vi.fn(),
  clearSession: () => { session.current = null },
  rotateSessionAccess: () => { session.current = session.current ? { ...session.current } : null; return session.current },
}))
vi.mock('@pdlc/ui/sso', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@pdlc/ui/sso')>()),
  ...sso,
}))

beforeEach(() => {
  vi.resetModules()
  vi.clearAllMocks()
  session.current = null
  vi.stubEnv('VITE_AUTH_ISSUER', 'http://localhost:8701')
  sso.restoreSso.mockRejectedValue(
    Object.assign(new Error('Login required'), {
      name: 'SsoLoginRequiredError',
    }),
  )
})
afterEach(() => {
  vi.unstubAllEnvs()
})

describe('deployment-owned SSO client', () => {
  it.each([
    { configured: undefined, expected: 'ci-cd' },
    { configured: 'ci-cd-dev', expected: 'ci-cd-dev' },
    { configured: 'ci-cd-dev', expected: 'ci-cd-dev', namespace: 'pdlc' },
  ])('restores SSO with $expected', async ({ configured, expected, namespace }) => {
    vi.stubEnv('VITE_AUTH_CLIENT_ID', configured)
    vi.stubEnv('VITE_AUTH_NAMESPACE', namespace)
    const { AuthProvider } = await import('./auth-provider')
    render(
      <AuthProvider>
        <div>Fixture</div>
      </AuthProvider>,
    )
    await waitFor(() =>
      expect(sso.restoreSso).toHaveBeenCalledWith({
        issuer: 'http://localhost:8701',
        clientId: expected,
        namespace: namespace ?? 'sdlc',
      }),
    )
  })

  it('uses the same configured client for logout', async () => {
    vi.stubEnv('VITE_AUTH_CLIENT_ID', 'ci-cd-dev')
    session.current = { access_token: 'synthetic', expires_at: 2_000_000_000 }
    const { AuthProvider, useAuth } = await import('./auth-provider')
    function Logout() {
      const { logout } = useAuth()
      return <button onClick={() => void logout()}>Logout</button>
    }
    render(
      <AuthProvider>
        <Logout />
      </AuthProvider>,
    )
    fireEvent.click(screen.getByRole('button', { name: 'Logout' }))
    await waitFor(() =>
      expect(sso.endSso).toHaveBeenCalledWith({
        issuer: 'http://localhost:8701',
        clientId: 'ci-cd-dev',
        namespace: 'sdlc',
      }),
    )
    expect(sso.restoreSso).not.toHaveBeenCalled()
  })
})
