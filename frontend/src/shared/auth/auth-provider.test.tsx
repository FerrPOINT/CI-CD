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

  it('keeps an in-memory central session', async () => {
    authMod.currentSession.mockReturnValue({ access_token: 'x', expires_at: 1 })
    render(
      <AuthProvider>
        <Probe />
      </AuthProvider>,
    )
    await waitFor(() => expect(screen.getByTestId('probe').textContent).toBe('authenticated'))
  })

  it('requires central login after a hard reload', async () => {
    render(
      <AuthProvider>
        <Probe />
      </AuthProvider>,
    )
    await waitFor(() => expect(screen.getByTestId('probe').textContent).toBe('anonymous'))
  })

  it('does not enter open mode when central auth is unavailable', async () => {
    render(
      <AuthProvider>
        <Probe />
      </AuthProvider>,
    )
    await waitFor(() => expect(screen.getByTestId('probe').textContent).toBe('anonymous'))
  })
})

describe('ProtectedRoute', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    authMod.currentSession.mockReturnValue(null)
  })

  it('renders children with a central session', async () => {
    authMod.currentSession.mockReturnValue({ access_token: 'x', expires_at: 1 })
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
