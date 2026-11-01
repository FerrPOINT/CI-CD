import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { api, ApiError } from '@/api/client'
import { RepositoryConfigurations } from './repository-configurations'

vi.mock('@/api/client', async original => ({ ...await original<object>(), api: vi.fn() }))
vi.mock('@/shared/auth/auth-provider', () => ({ useAuth: () => ({ session: { subject: 'ux-config-tests' } }) }))
const config = { id: 'config', name: 'Build', default_branch: 'main', repository_id: 'repo' }
function mount(writable = true) {
  render(<QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
    <MemoryRouter><RepositoryConfigurations repositoryId="repo" writable={writable} /></MemoryRouter>
  </QueryClientProvider>)
}
beforeEach(() => {
  sessionStorage.clear()
  vi.mocked(api).mockImplementation(async path => {
    if (path.endsWith('/push-config')) return { configuration_id: 'config' }
    if (path === '/delivery-configurations/config') return config
    if (path.includes('/delivery-config-operations/')) throw new ApiError({ kind: 'api', status: 404, message: 'not found' })
    return { items: [config], total: 1 }
  })
})
afterEach(() => { cleanup(); vi.resetAllMocks(); sessionStorage.clear() })

it('keeps management forms closed until requested and omits a single-page pager', async () => {
  mount()
  await screen.findByText('Build')
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  expect(screen.queryByRole('button', { name: 'Далее' })).not.toBeInTheDocument()
  fireEvent.click(screen.getByRole('button', { name: 'Создать конфигурацию' }))
  expect(screen.getByRole('dialog')).toBeInTheDocument()
  expect(screen.getByLabelText('Название')).toBeInTheDocument()
})

it('checks the original receipt after an unknown POST and never sends a second create', async () => {
  let committed = false
  vi.mocked(api).mockImplementation(async (path, init) => {
    if (path.includes('/delivery-config-operations/')) {
      if (committed) return config
      throw new ApiError({ kind: 'api', status: 404, message: 'not found' })
    }
    if (init?.method === 'POST') { committed = true; throw new ApiError({ kind: 'network', message: 'Connection lost' }) }
    if (path.endsWith('/push-config')) return { configuration_id: 'config' }
    return { items: [config], total: 1 }
  })
  mount()
  fireEvent.click(screen.getByRole('button', { name: 'Создать конфигурацию' }))
  fireEvent.change(screen.getByLabelText('Название'), { target: { value: 'Build' } })
  fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Создать' }))
  expect(await screen.findByRole('alert')).toHaveTextContent('Проверьте результат операции')
  const replay = within(screen.getByRole('dialog')).getByRole('button', { name: 'Проверить и продолжить' })
  await waitFor(() => expect(replay).not.toBeDisabled())
  fireEvent.click(replay)
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
  expect(vi.mocked(api).mock.calls.filter(([, init]) => init?.method === 'POST')).toHaveLength(1)
  const reads = vi.mocked(api).mock.calls.filter(([path]) => path.includes('/delivery-config-operations/'))
  expect(reads).toHaveLength(2)
  expect(reads[0][0]).toBe(reads[1][0])
})

it('requires an explicit changed push selection and saves disabled as null', async () => {
  mount()
  fireEvent.click(screen.getByRole('button', { name: 'Автозапуск' }))
  const save = screen.getByRole('button', { name: 'Сохранить автозапуск' })
  await screen.findByRole('button', { name: 'Конфигурация автозапуска: Build' })
  expect(save).toBeDisabled()
  fireEvent.click(screen.getByRole('button', { name: 'Конфигурация автозапуска: Build' }))
  fireEvent.click(screen.getByRole('button', { name: 'Отключён' }))
  expect(save).not.toBeDisabled()
  fireEvent.click(save)
  await waitFor(() => expect(api).toHaveBeenCalledWith('/catalog/repositories/repo/push-config', expect.objectContaining({ method: 'PUT', body: JSON.stringify({ configuration_id: null }) })))
})

it('keeps readonly configuration history visible without writable actions', async () => {
  mount(false)
  await screen.findByText('Build')
  expect(screen.getByRole('button', { name: 'Создать конфигурацию' })).toBeDisabled()
  expect(screen.queryByRole('button', { name: 'Автозапуск' })).not.toBeInTheDocument()
  expect(screen.getByRole('link', { name: 'Запуски' })).toBeInTheDocument()
})
