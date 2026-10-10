import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter, Route, Routes } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { api } from '@/api/client'
import { ConfigurationSettingsPage } from './index'
vi.mock('@/api/client', () => ({ api: vi.fn() }))
function mount(state = 'active') {
  vi.mocked(api).mockImplementation(async (path, options) =>
    options?.method === 'PATCH'
      ? {}
      : path.startsWith('/catalog')
        ? { state }
        : {
            id: 'config',
            name: 'Build',
            repository_id: 'repo',
            repository_url: 'https://example.test/original.git',
            default_branch: 'main',
            max_running_jobs: null,
          },
  )
  render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <MemoryRouter initialEntries={['/delivery-configs/config/settings']}>
        <Routes>
          <Route
            path="/delivery-configs/:projectId/settings"
            element={<ConfigurationSettingsPage />}
          />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}
afterEach(() => {
  cleanup()
  vi.resetAllMocks()
})
describe('repository-bound configuration settings', () => {
  it('updates execution settings without rewriting the repository checkout identity', async () => {
    mount()
    const branch = await screen.findByLabelText('Ветка по умолчанию')
    await waitFor(() => expect(screen.getByRole('button', { name: 'Сохранить' })).toBeEnabled())
    expect(screen.getByLabelText('Git URL')).toHaveAttribute('readonly')
    fireEvent.change(branch, { target: { value: 'release' } })
    fireEvent.change(screen.getByLabelText('Одновременно выполняемых jobs'), {
      target: { value: '3' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Сохранить' }))
    await screen.findByText('Настройки сохранены.')
    const request = vi.mocked(api).mock.calls.find(([, options]) => options?.method === 'PATCH')!
    expect(request[0]).toBe('/projects/config')
    expect(JSON.parse(request[1]!.body as string)).toEqual({
      name: 'Build',
      default_branch: 'release',
      max_running_jobs: 3,
    })
  })
  it('keeps archived configuration settings readable and closes new writes', async () => {
    mount('archived')
    expect(await screen.findByLabelText('Название конфигурации')).toHaveValue('Build')
    expect(screen.getByRole('button', { name: 'Сохранить' })).toBeDisabled()
    expect(screen.getByRole('alert')).toHaveTextContent('только для чтения')
    expect(vi.mocked(api).mock.calls.some(([, options]) => options?.method === 'PATCH')).toBe(false)
  })
})
