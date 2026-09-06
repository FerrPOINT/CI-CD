import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Routes, Route } from 'react-router'
import { AuthProvider, useAuth } from './auth-provider'
import { ProtectedRoute } from './protected-route'

const authMod = vi.hoisted(() => ({
  currentSession: vi.fn<() => unknown>(() => null),
  refresh: vi.fn(),
  authRequired: vi.fn(),
  login: vi.fn(),
  logout: vi.fn(),
}))

vi.mock('@/api/auth', () => authMod)

function Probe() {
  const { status } = useAuth()
  return <div data-testid="probe">{status}</div>
}

describe('AuthProvider', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    authMod.currentSession.mockReturnValue(null)
  })

  it('restores an expired-tab session via refresh', async () => {
    authMod.refresh.mockResolvedValue({ access_token: 'x', expires_at: 1 })
    render(
      <AuthProvider>
        <Probe />
      </AuthProvider>,
    )
    await waitFor(() => expect(screen.getByTestId('probe').textContent).toBe('authenticated'))
  })

  it('falls back to anonymous when refresh fails and auth is enforced', async () => {
    authMod.refresh.mockResolvedValue(null)
    authMod.authRequired.mockResolvedValue(true)
    render(
      <AuthProvider>
        <Probe />
      </AuthProvider>,
    )
    await waitFor(() => expect(screen.getByTestId('probe').textContent).toBe('anonymous'))
  })

  it('stays open-mode when the backend does not enforce auth', async () => {
    authMod.refresh.mockResolvedValue(null)
    authMod.authRequired.mockResolvedValue(false)
    render(
      <AuthProvider>
        <Probe />
      </AuthProvider>,
    )
    await waitFor(() => expect(screen.getByTestId('probe').textContent).toBe('open-mode'))
  })
})

describe('ProtectedRoute', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    authMod.currentSession.mockReturnValue(null)
  })

  it('renders children in open-mode', async () => {
    authMod.refresh.mockResolvedValue(null)
    authMod.authRequired.mockResolvedValue(false)
    render(
      <MemoryRouter initialEntries={['/secret']}>
        <AuthProvider>
          <Routes>
            <Route element={<ProtectedRoute />}>
              <Route path="/secret" element={<div>secret-content</div>} />
            </Route>
          </Routes>
        </AuthProvider>
      </MemoryRouter>,
    )
    await waitFor(() => expect(screen.getByText('secret-content')).toBeInTheDocument())
  })

  it('redirects to /login when anonymous', async () => {
    authMod.refresh.mockResolvedValue(null)
    authMod.authRequired.mockResolvedValue(true)
    render(
      <MemoryRouter initialEntries={['/secret']}>
        <AuthProvider>
          <Routes>
            <Route element={<ProtectedRoute />}>
              <Route path="/secret" element={<div>secret-content</div>} />
            </Route>
            <Route path="/login" element={<div>login-page</div>} />
          </Routes>
        </AuthProvider>
      </MemoryRouter>,
    )
    await waitFor(() => expect(screen.getByText('login-page')).toBeInTheDocument())
    expect(screen.queryByText('secret-content')).not.toBeInTheDocument()
  })
})
