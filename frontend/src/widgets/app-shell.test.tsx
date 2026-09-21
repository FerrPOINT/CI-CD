import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { ThemeProvider } from '@sdlc/ui/lib'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const authState = vi.hoisted(() => ({
  logout: vi.fn(),
  session: {
    access_token: 'access-token',
    expires_at: 2_000_000_000,
    username: 'admin',
  },
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

vi.mock('@/shared/auth/auth-provider', () => ({
  useAuth: () => ({
    status: 'authenticated',
    session: authState.session,
    logout: authState.logout,
    acceptSso: vi.fn(),
    invalidate: vi.fn(),
  }),
}))

vi.mock('@sdlc/ui/ui', async () => {
  const actual = await vi.importActual<typeof import('@sdlc/ui/ui')>('@sdlc/ui/ui')
  return {
    ...actual,
    PlatformMark: () => <span aria-hidden>mark</span>,
    ServiceSwitcher: () => <button type="button">services</button>,
    ThemeToggle: () => <button type="button">theme</button>,
  }
})

import { AppShell } from './app-shell'

function renderShell(path = '/') {
  return render(
    <ThemeProvider>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route element={<AppShell />}>
            <Route path="*" element={<div>outlet</div>} />
          </Route>
        </Routes>
      </MemoryRouter>
    </ThemeProvider>,
  )
}

beforeEach(() => {
  authState.logout.mockReset()
  authState.logout.mockResolvedValue(undefined)
})

afterEach(() => {
  cleanup()
})

describe('AppShell navigation', () => {
  it('marks the parent section active on a direct nested route', () => {
    renderShell('/repositories/platform-core/compare')

    expect(screen.getByRole('link', { name: 'navigation.repositories' })).toHaveAttribute('aria-current', 'page')
    expect(screen.getByRole('link', { name: 'navigation.dashboard' })).not.toHaveAttribute('aria-current')
  })

  it('uses an accessible focus-managed mobile drawer', async () => {
    renderShell()
    const trigger = screen.getByRole('button', { name: 'navigation.toggleMenu' })

    fireEvent.click(trigger)

    const dialog = await screen.findByRole('dialog', { name: 'navigation.toggleMenu' })
    expect(within(dialog).getByRole('navigation', { name: 'navigation.main' })).toBeInTheDocument()
    expect(dialog.contains(document.activeElement)).toBe(true)

    fireEvent.keyDown(document, { key: 'Escape' })

    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'navigation.toggleMenu' })).not.toBeInTheDocument())
    await waitFor(() => expect(trigger).toHaveFocus())
  })

  it('[REQ-AUTH-001] exposes the current user and signs out from the header', async () => {
    renderShell()

    expect(screen.getAllByRole('img', { name: 'app.name' })).toHaveLength(2)
    expect(screen.getByText('admin')).toBeInTheDocument()
    expect(screen.getByTitle('admin')).not.toHaveAttribute('aria-label')
    fireEvent.click(screen.getByRole('button', { name: 'navigation.logout' }))

    await waitFor(() => expect(authState.logout).toHaveBeenCalledOnce())
  })
})
