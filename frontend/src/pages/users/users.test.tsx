import { render, screen, waitFor } from '@testing-library/react'
import { PlatformServicesProvider } from '@pdlc/ui/lib'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { UsersPage } from './index'

const catalogUrl = 'https://admin.own.test/api/v1/runtime/services'

function view(path = '/users') {
  return render(
    <PlatformServicesProvider catalogUrl={catalogUrl}>
      <MemoryRouter initialEntries={[path]}><UsersPage /></MemoryRouter>
    </PlatformServicesProvider>,
  )
}

afterEach(() => vi.unstubAllGlobals())

describe('UsersPage', () => {
  it('does not expose a compiled Admin address while the own catalog is pending', () => {
    vi.stubGlobal('fetch', vi.fn(() => new Promise<Response>(() => {})))
    view()
    expect(screen.getByRole('button', { name: /Открыть пользователей/ })).toBeDisabled()
    expect(screen.queryByRole('link')).not.toBeInTheDocument()
    expect(screen.queryByLabelText(/password|пароль|role|роль/i)).not.toBeInTheDocument()
  })

  it('keeps navigation disabled when the own Admin catalog is unavailable', async () => {
    const request = vi.fn(() => Promise.resolve(new Response('unavailable', { status: 503 })))
    vi.stubGlobal('fetch', request)
    view()
    await waitFor(() => expect(request).toHaveBeenCalledTimes(1))
    expect(screen.getByRole('button', { name: /Открыть пользователей/ })).toBeDisabled()
    expect(screen.queryByRole('link')).not.toBeInTheDocument()
  })

  it('uses the runtime Admin base path and preserves the exact namespace', async () => {
    const request = vi.fn(() => Promise.resolve(new Response(JSON.stringify({ services: [{
      key: 'admin-panel', label: 'Own Admin', url: 'http://localhost:8771',
      ui_url: 'http://localhost:8772/admin/', health: 'unknown',
    }] }))))
    vi.stubGlobal('fetch', request)
    const scope = 'registry_instance_id=11111111-1111-4111-8111-111111111111&namespace_id=22222222-2222-4222-8222-222222222222'
    view('/users?' + scope)
    expect(await screen.findByRole('link', { name: /Открыть пользователей/ })).toHaveAttribute(
      'href', 'http://localhost:8772/admin/users?' + scope,
    )
    expect(request).toHaveBeenCalledWith(catalogUrl, { cache: 'no-store' })
    expect(screen.queryByLabelText(/password|пароль|role|роль/i)).not.toBeInTheDocument()
  })
})
