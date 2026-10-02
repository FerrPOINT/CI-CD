import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { SsoLogoutPendingError } from '@sdlc/ui/sso'
import { LoginPage } from './'

const beginSso = vi.hoisted(() => vi.fn<(...args: unknown[]) => Promise<void>>(async () => {}))
vi.mock('@sdlc/ui/sso', async (importOriginal) => ({
  ...await importOriginal<typeof import('@sdlc/ui/sso')>(), beginSso,
}))
vi.mock('@sdlc/ui/ui', async (importOriginal) => ({
  ...await importOriginal<typeof import('@sdlc/ui/ui')>(),
  PlatformMark: () => <span>SDLC</span>,
}))
vi.mock('@/shared/auth/auth-provider', () => ({
  ssoConfig: { issuer: 'http://localhost:7701', clientId: 'ci-cd' },
  useAuth: () => ({ status: 'anonymous' }),
}))

function renderLogin(path = '/login') {
  return render(<MemoryRouter initialEntries={[path]}><LoginPage /></MemoryRouter>)
}

describe('ci-cd login recovery', () => {
  beforeEach(() => {
    beginSso.mockReset().mockResolvedValue(undefined)
  })
  afterEach(cleanup)

  it('keeps automatic guards non-interactive', async () => {
    renderLogin()
    await waitFor(() => expect(beginSso).toHaveBeenCalledWith(
      expect.objectContaining({ clientId: 'ci-cd' }), '/',
    ))
  })

  it('requires explicit login after logout and marks it interactive', async () => {
    renderLogin('/login?logged_out=1')
    expect(beginSso).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Войти через SDLC' }))
    await waitFor(() => expect(beginSso).toHaveBeenCalledWith(
      expect.objectContaining({ clientId: 'ci-cd' }), '/', { interactive: true },
    ))
  })

  it.each([new SsoLogoutPendingError(), new DOMException('Cancelled', 'AbortError')])(
    'does not report interrupted automatic navigation as auth failure: %s', async (error) => {
      beginSso.mockRejectedValueOnce(error)
      renderLogin()
      await waitFor(() => expect(beginSso).toHaveBeenCalledOnce())
      fireEvent.click(screen.getByRole('button', { name: 'Войти через SDLC' }))
      await waitFor(() => expect(beginSso).toHaveBeenCalledTimes(2))
      expect(screen.queryByRole('alert')).not.toBeInTheDocument()
      expect(beginSso).toHaveBeenLastCalledWith(
        expect.objectContaining({ clientId: 'ci-cd' }), '/', { interactive: true },
      )
    },
  )

  it('keeps real auth failure visible and clears it after retry', async () => {
    beginSso.mockRejectedValueOnce(new Error('private auth details'))
    renderLogin('/login?logged_out=1')
    fireEvent.click(screen.getByRole('button', { name: 'Войти через SDLC' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('Central Auth временно недоступен.')
    expect(screen.queryByText(/private auth details/)).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Войти через SDLC' }))
    await waitFor(() => expect(screen.queryByRole('alert')).not.toBeInTheDocument())
  })
})

