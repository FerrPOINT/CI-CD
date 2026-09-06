import { cleanup, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { ThemeProvider } from '@sdlc/ui/lib'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const authMocks = vi.hoisted(() => ({
  authRequired: vi.fn(),
  currentSession: vi.fn(),
  refresh: vi.fn(),
  login: vi.fn(),
  logout: vi.fn(),
}))
const navigateMock = vi.hoisted(() => vi.fn())

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

vi.mock('@/api/auth', () => authMocks)

vi.mock('react-router', async () => {
  const actual = await vi.importActual<typeof import('react-router')>('react-router')
  return { ...actual, Outlet: () => <div>outlet</div>, useNavigate: () => navigateMock }
})

vi.mock('@sdlc/ui/ui', async () => {
  const actual = await vi.importActual('@sdlc/ui/ui')
  return { ...actual, ThemeToggle: () => <button>theme</button> }
})

import { AppShell } from './app-shell'
import { AuthProvider } from '@/shared/auth/auth-provider'

beforeEach(() => {
  navigateMock.mockReset()
  authMocks.authRequired.mockReset()
  authMocks.currentSession.mockReset()
  authMocks.refresh.mockReset()
  authMocks.currentSession.mockReturnValue(null)
  authMocks.refresh.mockResolvedValue(null)
  authMocks.authRequired.mockResolvedValue(false)
})

afterEach(() => {
  cleanup()
})

describe('AppShell mobile navigation', () => {
  it('gives the mobile drawer trigger an accessible name', () => {
    render(
      <ThemeProvider>
        <AuthProvider>
          <MemoryRouter>
            <AppShell />
          </MemoryRouter>
        </AuthProvider>
      </ThemeProvider>,
    )

    expect(screen.getByRole('button', { name: 'navigation.toggleMenu' })).toBeDefined()
    expect(screen.getByRole('button', { name: 'navigation.toggleMenu' }).getAttribute('aria-controls')).toBe('mobile-navigation')
  })

  it('[REQ-AUTH-001] renders the logout control for an authenticated session', async () => {
    const restoredSession = {
      access_token: 'access-token',
      expires_at: Math.floor(Date.now() / 1000) + 900,
      username: 'admin',
    }
    authMocks.refresh.mockImplementation(async () => {
      authMocks.currentSession.mockReturnValue(restoredSession)
      return restoredSession
    })

    render(
      <ThemeProvider>
        <AuthProvider>
          <MemoryRouter>
            <AppShell />
          </MemoryRouter>
        </AuthProvider>
      </ThemeProvider>,
    )

    await waitFor(() => expect(screen.getByRole('button', { name: 'navigation.logout' })).toBeDefined())
  })
})
