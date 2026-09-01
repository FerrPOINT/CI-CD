import { cleanup, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const authMocks = vi.hoisted(() => ({
  authRequired: vi.fn(),
  currentSession: vi.fn(),
  refresh: vi.fn(),
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

vi.mock('@/shared/ui/theme-toggle', () => ({ ThemeToggle: () => <button>theme</button> }))

import { AppShell } from './app-shell'

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
    render(<MemoryRouter><AppShell /></MemoryRouter>)

    expect(screen.getByRole('button', { name: 'navigation.toggleMenu' })).toBeDefined()
    expect(screen.getByRole('button', { name: 'navigation.toggleMenu' }).getAttribute('aria-controls')).toBe('mobile-navigation')
  })

  it('[REQ-AUTH-001] restores a refresh session before redirecting to login', async () => {
    authMocks.refresh.mockResolvedValue({
      access_token: 'access-token',
      expires_at: Math.floor(Date.now() / 1000) + 900,
      username: 'admin',
    })
    authMocks.authRequired.mockResolvedValue(true)

    render(<MemoryRouter><AppShell /></MemoryRouter>)

    await waitFor(() => expect(authMocks.refresh).toHaveBeenCalledTimes(1))
    expect(authMocks.authRequired).not.toHaveBeenCalled()
    expect(navigateMock).not.toHaveBeenCalled()
  })

  it('[REQ-AUTH-001] redirects to login when auth is required and refresh is unavailable', async () => {
    authMocks.refresh.mockResolvedValue(null)
    authMocks.authRequired.mockResolvedValue(true)

    render(<MemoryRouter><AppShell /></MemoryRouter>)

    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/login', { replace: true }))
  })
})
