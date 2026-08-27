import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { describe, expect, it, vi } from 'vitest'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

vi.mock('react-router', async () => {
  const actual = await vi.importActual<typeof import('react-router')>('react-router')
  return { ...actual, Outlet: () => <div>outlet</div> }
})

vi.mock('@/shared/ui/theme-toggle', () => ({ ThemeToggle: () => <button>theme</button> }))

import { AppShell } from './app-shell'

describe('AppShell mobile navigation', () => {
  it('gives the mobile drawer trigger an accessible name', () => {
    render(<MemoryRouter><AppShell /></MemoryRouter>)

    expect(screen.getByRole('button', { name: 'navigation.toggleMenu' })).toBeDefined()
    expect(screen.getByRole('button', { name: 'navigation.toggleMenu' }).getAttribute('aria-controls')).toBe('mobile-navigation')
  })
})
