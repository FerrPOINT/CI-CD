import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  fireEvent,
  render,
  screen,
  waitFor,
  cleanup,
} from '@testing-library/react'
import { MemoryRouter, Routes, Route } from 'react-router'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { ApiError } from '@/api/client'
import { repositorySync, type SyncConnection, type SyncRun } from '@/api/repository-sync'
import {
  RepositorySyncEditor,
  RepositorySyncsPage,
  RepositorySyncPage,
} from './index'

const context = vi.hoisted(() => ({
  ref: null as { registry_instance_id: string; namespace_id: string } | null,
  all: true,
  malformed: false,
}))

vi.mock('@/widgets/namespace-context', () => ({
  useNamespaceRef: () => context,
}))
vi.mock('@/api/workspaces', () => ({
  useWorkspace: () => ({ data: { group_id: 'group-a' } }),
}))
const connection: SyncConnection = {
  repository_id: 'repo',
  repository_name: 'platform/service',
  namespace: null,
  version: 1,
  settings: {
    provider: 'github',
    instance_url: 'https://github.com',
    repository_path: 'team/service',
    direction: 'both',
    automatic: false,
  },
  has_credential: true,
  writable: true,
  paused_reason: null,
  worker_enabled: true,
  next_check_at: null,
  active_run: null,
  last_run: null,
}
function mount(component: React.ReactNode, location = '/') {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  return render(
    <QueryClientProvider client={cache}>
      <MemoryRouter initialEntries={[location]}>{component}</MemoryRouter>
    </QueryClientProvider>,
  )
}
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  context.ref = null
  context.all = true
  context.malformed = false
})
describe('production repository synchronization', () => {
  it('refreshes visible history and refs when the parent observes completion first', async () => {
    const running: SyncRun = {
      id: 'run', repository_id: 'repo', connection_version: 1,
      settings: connection.settings, state: 'running', source: 'manual',
      created_at: '2026-10-17T00:00:00Z', started_at: '2026-10-17T00:00:00Z',
      finished_at: null, cancel_requested: false, message: null,
    }
    const finished: SyncRun = { ...running, state: 'success', finished_at: '2026-10-17T00:00:01Z' }
    const runs = vi.spyOn(repositorySync, 'runs')
      .mockResolvedValueOnce({ items: [running], total: 1 })
      .mockResolvedValue({ items: [finished], total: 1 })
    const refs = vi.spyOn(repositorySync, 'refs')
      .mockResolvedValueOnce({ items: [{ ref_name: 'refs/heads/qa', direction: 'pull', state: 'planned', event_delivered: false }], total: 1 })
      .mockResolvedValue({ items: [{ ref_name: 'refs/heads/qa', direction: 'pull', state: 'applied', event_delivered: true }], total: 1 })
    const cache = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const view = (current: SyncConnection) => (
      <QueryClientProvider client={cache}>
        <MemoryRouter initialEntries={['/?tab=history&run_id=run']}>
          <RepositorySyncEditor connection={current} reload={vi.fn()} />
        </MemoryRouter>
      </QueryClientProvider>
    )
    const rendered = render(view({ ...connection, active_run: running }))
    await waitFor(() => expect(runs).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(refs).toHaveBeenCalledTimes(1))
    rendered.rerender(view({ ...connection, active_run: null, last_run: finished }))
    await waitFor(() => expect(runs).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(refs).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(cache.getQueryData<{ items: SyncRun[] }>(['sync-runs', 'repo', 0])?.items[0].state).toBe('success'))
    await waitFor(() => expect(cache.getQueryData<{ items: { state: string }[] }>(['sync-refs', 'repo', 'run', 0])?.items[0].state).toBe('applied'))
    cache.clear()
  })

  it('refreshes terminal history and refs after connection polling observes completion', async () => {
    const run: SyncRun = {
      id: 'run', repository_id: 'repo', connection_version: 1,
      settings: connection.settings, source: 'manual', state: 'running',
      message: null, cancel_requested: false, created_at: '2026-10-17T10:00:00Z',
      started_at: '2026-10-17T10:00:01Z', finished_at: null,
    }
    const done = { ...run, state: 'success', finished_at: '2026-10-17T10:00:02Z' }
    vi.spyOn(repositorySync, 'runs')
      .mockResolvedValueOnce({ items: [run], total: 1 })
      .mockResolvedValue({ items: [done], total: 1 })
    const reference = { ref_name: 'refs/heads/main', direction: 'pull', old_sha: null,
      new_sha: 'a'.repeat(40), state: 'planned', reason: null, event_delivered: false }
    vi.spyOn(repositorySync, 'refs')
      .mockResolvedValueOnce({ items: [reference], total: 1 })
      .mockResolvedValue({ items: [{ ...reference, state: 'applied', event_delivered: true }], total: 1 })
    const cache = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const page = (value: SyncConnection) => (
      <QueryClientProvider client={cache}>
        <MemoryRouter initialEntries={['/?tab=history&run_id=run']}>
          <RepositorySyncEditor connection={value} reload={vi.fn()} />
        </MemoryRouter>
      </QueryClientProvider>
    )
    const view = render(page({ ...connection, active_run: run }))
    await screen.findByText('Ожидает передачи')
    await waitFor(() => expect(screen.getAllByText('Выполняется')).toHaveLength(2))
    view.rerender(page({ ...connection, active_run: null, last_run: done }))
    await screen.findByText('Передано')
    await waitFor(() => expect(screen.queryByText('Выполняется')).not.toBeInTheDocument())
    expect(screen.queryByText('Ожидает передачи')).not.toBeInTheDocument()
    cache.clear()
  })
  it('preserves all-project scope and opens each row in its actual namespace', async () => {
    context.ref = {
      registry_instance_id: 'registry-a',
      namespace_id: 'namespace-a',
    }
    context.all = true
    const list = vi
      .spyOn(repositorySync, 'list')
      .mockResolvedValue({
        items: [
          {
            ...connection,
            namespace: {
              registry_instance_id: 'registry-b',
              namespace_id: 'namespace-b',
            },
          },
        ],
        total: 1,
      })
    mount(
      <RepositorySyncsPage />,
      '/repository-syncs?project_scope=all&search=service',
    )
    const link = await screen.findByRole('link', { name: 'platform/service' })
    expect(list.mock.calls[0][0].has('group_id')).toBe(false)
    expect(link.getAttribute('href')).toContain('namespace_id=namespace-b')
    expect(link.getAttribute('href')).toContain('project_scope=all')
    expect(link.getAttribute('href')).toContain('search=service')
  })
  it('blocks an editor whose URL context belongs to another namespace', async () => {
    context.ref = {
      registry_instance_id: 'registry-a',
      namespace_id: 'namespace-a',
    }
    context.all = false
    vi.spyOn(repositorySync, 'get').mockResolvedValue({
      ...connection,
      namespace: {
        registry_instance_id: 'registry-b',
        namespace_id: 'namespace-b',
      },
    })
    mount(
      <Routes>
        <Route
          path="/catalog/repositories/:id/sync"
          element={<RepositorySyncPage />}
        />
      </Routes>,
      '/catalog/repositories/repo/sync',
    )
    await screen.findByText('Репозиторий принадлежит другому проекту.')
    expect(screen.queryByLabelText('Токен доступа')).not.toBeInTheDocument()
  })
  it('retains draft on CAS conflict and refreshes version without replacing edited fields', async () => {
    const save = vi
      .spyOn(repositorySync, 'save')
      .mockRejectedValueOnce(
        new ApiError({
          kind: 'api',
          status: 409,
          message: 'sync_version_conflict',
        }),
      )
      .mockResolvedValue({ ...connection, version: 3 })
    vi.spyOn(repositorySync, 'get').mockResolvedValue({
      ...connection,
      version: 2,
    })
    mount(<RepositorySyncEditor connection={connection} reload={vi.fn()} />)
    fireEvent.change(screen.getByLabelText('Владелец / репозиторий'), {
      target: { value: 'team/edited' },
    })
    fireEvent.click(screen.getByText('Сохранить подключение'))
    fireEvent.click(await screen.findByText('Обновить сохранённую версию'))
    await waitFor(() =>
      expect(
        screen.queryByText('Обновить сохранённую версию'),
      ).not.toBeInTheDocument(),
    )
    expect(screen.getByLabelText('Владелец / репозиторий')).toHaveValue(
      'team/edited',
    )
    fireEvent.click(screen.getByText('Сохранить подключение'))
    await waitFor(() => expect(save).toHaveBeenCalledTimes(2))
    expect(save.mock.calls[1][1]).toMatchObject({
      expected_version: 2,
      settings: { repository_path: 'team/edited' },
    })
  })
  it('retries an unknown save with the same operation and clears the credential after success', async () => {
    const save = vi
      .spyOn(repositorySync, 'save')
      .mockRejectedValueOnce(
        new ApiError({ kind: 'network', message: 'unknown' }),
      )
      .mockResolvedValue({ ...connection, version: 2 })
    mount(<RepositorySyncEditor connection={connection} reload={vi.fn()} />)
    fireEvent.change(screen.getByLabelText('Токен доступа'), {
      target: { value: 'fictional-test-token' },
    })
    fireEvent.click(screen.getByText('Сохранить подключение'))
    fireEvent.click(await screen.findByText('Проверить результат сохранения'))
    await screen.findByText('Подключение сохранено')
    expect(save.mock.calls[0]).toEqual(save.mock.calls[1])
    expect(screen.getByLabelText('Токен доступа')).toHaveValue('')
    expect(window.location.href).not.toContain('fictional-test-token')
  })
  it('keeps a manual run available in automatic mode and honors read-only permissions', () => {
    const { unmount } = mount(
      <RepositorySyncEditor
        connection={{
          ...connection,
          settings: { ...connection.settings, automatic: true },
        }}
        reload={vi.fn()}
      />,
    )
    expect(screen.getByText('Синхронизировать сейчас')).toBeEnabled()
    unmount()
    mount(
      <RepositorySyncEditor
        connection={{ ...connection, writable: false }}
        reload={vi.fn()}
      />,
    )
    expect(screen.getByLabelText('Токен доступа')).toBeDisabled()
    expect(screen.getByText('Синхронизировать сейчас')).toBeDisabled()
  })
  it('sends list filters and pagination to the server and does not turn loading errors into an empty list', async () => {
    const list = vi
      .spyOn(repositorySync, 'list')
      .mockRejectedValue(new ApiError({ kind: 'network', message: 'offline' }))
    mount(
      <RepositorySyncsPage />,
      '/repository-syncs?provider=github&state=conflict&offset=20&search=service',
    )
    await screen.findByRole('alert')
    const query = list.mock.calls[0][0]
    expect(query.get('provider')).toBe('github')
    expect(query.get('offset')).toBe('20')
    expect(query.get('search')).toBe('service')
    expect(
      screen.queryByText(/Синхронизации не найдены/),
    ).not.toBeInTheDocument()
  })
})
