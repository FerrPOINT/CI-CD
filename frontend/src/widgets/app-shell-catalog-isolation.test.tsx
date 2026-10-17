import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { PlatformServicesProvider, ThemeProvider } from '@pdlc/ui/lib'
import { MemoryRouter, Route, Routes } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AppShell } from './app-shell'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))
vi.mock('@/shared/auth/auth-provider', () => ({
  useAuth: () => ({ session: { username: 'QA' }, logout: vi.fn() }),
}))

const catalogUrl = 'https://admin.own.test/api/v1/runtime/services'
const catalog = {
  services: [
    ['admin-panel', 'Own Admin', 'http://localhost:8772'],
    ['ci-cd', 'Own CI/CD', 'http://localhost:8712'],
    ['task-tracker', 'Own Tracker', 'http://localhost:8722'],
    ['wiki', 'Own Wiki', 'http://localhost:8732'],
    ['fleet-control', 'Own Fleet', 'http://localhost:8742'],
    ['project-workflow', 'Own Workflow', 'http://localhost:8752'],
  ].map(([key, label, url]) => ({ key, label, url, ui_url: url, health: 'unknown' })),
}

function view() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } })
  render(
    <QueryClientProvider client={client}>
      <ThemeProvider>
        <PlatformServicesProvider catalogUrl={catalogUrl}>
          <MemoryRouter initialEntries={['/repository-syncs?project_scope=all']}>
            <Routes>
              <Route element={<AppShell />}>
                <Route path="*" element={<p>Own CI page</p>} />
              </Route>
            </Routes>
          </MemoryRouter>
        </PlatformServicesProvider>
      </ThemeProvider>
    </QueryClientProvider>,
  )
}

async function openMenu() {
  fireEvent.keyDown(screen.getByRole('button', { name: /Открыть список сервисов/ }), {
    key: 'ArrowDown',
  })
  return screen.findByRole('menu')
}

function expectNoDestination(menu: HTMLElement) {
  const items = within(menu).getAllByRole('menuitem')
  expect(items).toHaveLength(6)
  for (const item of items) {
    expect(item).not.toHaveAttribute('href')
    expect(item).toHaveAttribute('aria-disabled', 'true')
    expect(item.querySelector('a[href]')).toBeNull()
  }
}

beforeEach(() => vi.stubEnv('VITE_NAMESPACE_ENABLED', 'false'))
afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
  vi.unstubAllEnvs()
})

describe('CI shell with the real Base service catalog', () => {
  it('keeps pending destinations disabled and then uses the exact own runtime catalog', async () => {
    let complete!: (response: Response) => void
    const request = vi.fn(() => new Promise<Response>((resolve) => { complete = resolve }))
    vi.stubGlobal('fetch', request)
    view()
    await waitFor(() => expect(request).toHaveBeenCalledTimes(1))
    expect(request).toHaveBeenCalledWith(catalogUrl, { cache: 'no-store' })
    const menu = await openMenu()
    expectNoDestination(menu)
    await act(async () => complete(new Response(JSON.stringify(catalog))))
    await waitFor(() => {
      for (const service of catalog.services) {
        const item = within(menu).getByRole('menuitem', { name: new RegExp(service.label) })
        if (service.key === 'ci-cd') {
          expect(item).toHaveAttribute('aria-disabled', 'true')
        } else {
          expect(item).toHaveAttribute('href', service.ui_url + '/')
          expect(item).not.toHaveAttribute('aria-disabled', 'true')
        }
      }
    })
    fireEvent.keyDown(menu, { key: 'Escape' })
    expect(screen.getByRole('link', { name: 'Все репозитории' })).toHaveAttribute(
      'href', '/repositories?project_scope=all',
    )
    expect(request).toHaveBeenCalledTimes(1)
  })

  it('keeps product navigation usable without exposing compiled origins when the catalog fails', async () => {
    const request = vi.fn(() => Promise.reject(new Error('own catalog unavailable')))
    vi.stubGlobal('fetch', request)
    view()
    await waitFor(() => expect(request).toHaveBeenCalledTimes(1))
    const menu = await openMenu()
    expectNoDestination(menu)
    fireEvent.keyDown(menu, { key: 'Escape' })
    expect(screen.getByRole('link', { name: 'Синхронизации' })).toHaveAttribute(
      'href', '/repository-syncs?project_scope=all',
    )
    expect(screen.getByText('Own CI page')).toBeVisible()
    expect(request).toHaveBeenCalledTimes(1)
  })
})
