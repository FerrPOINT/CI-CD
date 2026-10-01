import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
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
  authState.session.username = 'admin'
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('AppShell navigation', () => {
  it('marks the parent section active on a direct nested route', () => {
    renderShell('/repositories/platform-core/compare')

    expect(screen.getByRole('link', { name: 'navigation.repositories' })).toHaveAttribute(
      'aria-current',
      'page',
    )
    expect(screen.getByRole('link', { name: 'navigation.dashboard' })).not.toHaveAttribute(
      'aria-current',
    )
    expect(screen.getByText('outlet').parentElement).toHaveAttribute('data-page-layout', 'wide')
  })

  it('uses the shared readable work-area mode for settings', () => {
    renderShell('/settings')

    expect(screen.getByText('outlet').parentElement).toHaveAttribute('data-page-layout', 'reading')
  })

  it.each([
    ['/pipelines/pipeline-1', 'wide'],
    ['/repositories/platform/pulls/1', 'detail-with-aside'],
    ['/repositories/platform/pulls/1?view=diff', 'wide'],
    ['/repositories/platform/pulls/1?view=summary', 'detail-with-aside'],
    ['/repositories/platform/pulls', 'wide'],
    ['/projects/project-1/pipelines', 'wide'],
  ])('uses the semantic mode %s -> %s', (path, mode) => {
    renderShell(path)

    expect(screen.getByText('outlet').parentElement).toHaveAttribute('data-page-layout', mode)
  })

  it('uses an accessible focus-managed mobile drawer', async () => {
    renderShell()
    const trigger = screen.getByRole('button', {
      name: 'navigation.toggleMenu',
    })

    fireEvent.click(trigger)

    const dialog = await screen.findByRole('dialog', {
      name: 'navigation.toggleMenu',
    })
    expect(within(dialog).getByRole('navigation', { name: 'navigation.main' })).toBeInTheDocument()
    expect(dialog.contains(document.activeElement)).toBe(true)

    fireEvent.keyDown(document, { key: 'Escape' })

    await waitFor(() =>
      expect(
        screen.queryByRole('dialog', { name: 'navigation.toggleMenu' }),
      ).not.toBeInTheDocument(),
    )
    await waitFor(() => expect(trigger).toHaveFocus())
  })

  it('[REQ-AUTH-001] exposes the current user once in the account menu and signs out', () => {
    renderShell()
    expect(screen.queryByText('admin')).not.toBeInTheDocument()
    fireEvent.keyDown(screen.getByRole('button', { name: 'navigation.account' }), {
      key: 'ArrowDown',
    })
    const menu = screen.getByRole('menu', { hidden: true })
    expect(menu).toHaveAttribute('data-state', 'open')
    expect(within(menu).getAllByText('admin')).toHaveLength(1)
    fireEvent.click(within(menu).getByRole('menuitem', { name: 'navigation.logout' }))
    expect(authState.logout).toHaveBeenCalledOnce()
  })

  it('closes the drawer after following a navigation link', async () => {
    renderShell()
    const trigger = screen.getByRole('button', { name: 'navigation.toggleMenu' })
    fireEvent.click(trigger)
    const dialog = await screen.findByRole('dialog')
    fireEvent.click(within(dialog).getByRole('link', { name: 'navigation.projects' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(screen.getByRole('link', { name: 'navigation.projects' })).toHaveAttribute(
      'aria-current',
      'page',
    )
    expect(trigger).toHaveFocus()
  })

  it('closes the drawer on desktop resize and removes its media listener on unmount', async () => {
    const media = { matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() }
    vi.stubGlobal(
      'matchMedia',
      vi.fn(() => media),
    )
    const { unmount } = renderShell()
    fireEvent.click(screen.getByRole('button', { name: 'navigation.toggleMenu' }))
    await screen.findByRole('dialog')
    const listener = media.addEventListener.mock.calls[0][1] as () => void
    media.matches = true
    act(() => listener())
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(document.body).not.toHaveStyle({ pointerEvents: 'none' })
    unmount()
    expect(media.removeEventListener).toHaveBeenCalledWith('change', listener)
  })

  it('owns one global header above the offset work area and navigation-only sidebar', () => {
    const { container } = renderShell()
    const header = screen.getByRole('banner')
    expect(container.querySelectorAll('[data-platform-header]')).toHaveLength(1)
    expect(
      [...header.querySelectorAll('[data-platform-header-slot]')].map((slot) =>
        slot.getAttribute('data-platform-header-slot'),
      ),
    ).toEqual(['leading', 'services', 'actions'])
    expect(header.parentElement).toBe(container.firstElementChild)
    expect(
      within(header).getByRole('button', {
        name: 'Открыть список сервисов: CI/CD',
      }),
    ).toBeInTheDocument()
    expect(within(header).getByRole('link', { name: 'app.name' })).toHaveAttribute('href', '/')
    expect(container.querySelector('aside')).not.toContainElement(header)
    expect(container.querySelector('aside')).not.toHaveTextContent('admin')
  })

  it('keeps a long identity outside header layout and closes the account menu with Escape', () => {
    authState.session.username =
      'long-account-name-that-must-not-expand-the-platform-header@example.test'
    renderShell()
    fireEvent.keyDown(screen.getByRole('button', { name: 'navigation.account' }), {
      key: 'ArrowDown',
    })
    expect(
      within(screen.getByRole('menu', { hidden: true })).getByText(authState.session.username),
    ).toBeInTheDocument()
    fireEvent.keyDown(document, { key: 'Escape' })
    expect(screen.queryByRole('menu', { hidden: true })).not.toBeInTheDocument()
  })
})
