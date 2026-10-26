import { useState } from 'react'
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createMemoryRouter, RouterProvider, useNavigate } from 'react-router'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { Button } from '@pdlc/ui/ui'
import { ApiError } from '@/api/client'
import {
  repositorySync,
  type SyncConnection,
  type SyncRun,
} from '@/api/repository-sync'
import { RepositorySyncPage, RepositorySyncEditor } from './index'
import '@/shared/i18n/config'

const original: SyncConnection = {
  repository_id: 'guard-repository',
  repository_name: 'workspace/tooling',
  namespace: null,
  version: 4,
  settings: {
    provider: 'github',
    instance_url: 'https://github.com',
    repository_path: 'workspace/tooling',
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
const registry = 'aaaa1111-bbbb-4ccc-8ddd-eeeeeeeeeeee'
const namespace = 'bbbb2222-cccc-4ddd-8eee-ffffffffffff'
const otherNamespace = 'cccc3333-dddd-4eee-8fff-aaaaaaaaaaaa'
const path = '/catalog/repositories/guard-repository/sync'
const routers: ReturnType<typeof createMemoryRouter>[] = []
const clients: QueryClient[] = []

afterEach(() => {
  cleanup()
  routers.splice(0).forEach((router) => router.dispose())
  clients.splice(0).forEach((client) => client.clear())
  vi.restoreAllMocks()
})

function EphemeralNavigation() {
  const [visible, setVisible] = useState(true)
  const navigate = useNavigate()
  return (
    visible && (
      <Button
        onClick={() => {
          setVisible(false)
          void navigate('/repository-syncs')
        }}
      >
        Исчезающий пункт навигации
      </Button>
    )
  )
}
function mount(
  options: {
    connection?: SyncConnection
    location?: string
    parentBoundary?: boolean
  } = {},
) {
  let persisted = options.connection ?? original
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity },
      mutations: { retry: false },
    },
  })
  clients.push(client)
  client.setQueryData(['repository-sync', persisted.repository_id], persisted)
  const get = vi
    .spyOn(repositorySync, 'get')
    .mockImplementation(async () => persisted)
  const save = vi
    .spyOn(repositorySync, 'save')
    .mockRejectedValue(new Error('Unexpected fixture write'))
  const start = vi
    .spyOn(repositorySync, 'start')
    .mockRejectedValue(new Error('Unexpected fixture run'))
  const check = vi
    .spyOn(repositorySync, 'check')
    .mockRejectedValue(new Error('Unexpected fixture check'))
  vi.spyOn(repositorySync, 'runs').mockResolvedValue({ items: [], total: 0 })
  vi.spyOn(repositorySync, 'refs').mockResolvedValue({ items: [], total: 0 })
  const router = createMemoryRouter(
    [
      {
        path: '/catalog/repositories/:id/sync',
        element: (
          <>
            <EphemeralNavigation />
            {options.parentBoundary ? (
              <RepositorySyncEditor
                connection={persisted}
                reload={async () => undefined}
                shouldBlockNavigation={({ currentLocation, nextLocation }) =>
                  new URLSearchParams(currentLocation.search).get(
                    'production',
                  ) !==
                  new URLSearchParams(nextLocation.search).get('production')
                }
              />
            ) : (
              <RepositorySyncPage />
            )}
          </>
        ),
      },
      { path: '/repository-syncs', element: <h1>Список синхронизаций</h1> },
    ],
    {
      initialEntries: [
        options.location ?? path + '?project_scope=all&provider=github',
      ],
    },
  )
  routers.push(router)
  render(
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  )
  return {
    router,
    get,
    save,
    start,
    check,
    updateServer(next: SyncConnection) {
      persisted = next
      act(() =>
        client.setQueryData(['repository-sync', next.repository_id], next),
      )
    },
  }
}
function beforeUnload() {
  const event = new Event('beforeunload', { cancelable: true })
  window.dispatchEvent(event)
  return event.defaultPrevented
}
function back() {
  const link = screen.getByRole('link', { name: 'Все синхронизации' })
  link.focus()
  fireEvent.click(link)
  return link
}
const queued: SyncRun = {
  id: 'queued-guard-run',
  repository_id: original.repository_id,
  connection_version: 4,
  settings: original.settings,
  source: 'manual',
  state: 'queued',
  message: null,
  cancel_requested: false,
  created_at: '2026-10-27T00:00:00Z',
  started_at: null,
  finished_at: null,
}

describe('repository synchronization leave safety', () => {
  it('protects typed fields before blur and preserves input and focus when leaving is cancelled', async () => {
    const fixture = mount()
    fireEvent.change(screen.getByLabelText('Владелец / репозиторий'), {
      target: { value: 'workspace/edited' },
    })
    fireEvent.change(screen.getByLabelText('Токен доступа'), {
      target: { value: 'synthetic-not-a-credential' },
    })
    expect(beforeUnload()).toBe(true)
    const opener = back()
    const dialog = await screen.findByRole('alertdialog', {
      name: 'Покинуть страницу?',
    })
    expect(dialog).not.toHaveTextContent('synthetic-not-a-credential')
    fireEvent.click(within(dialog).getByRole('button', { name: 'Остаться' }))
    await waitFor(() => expect(opener).toHaveFocus())
    expect(await screen.findByLabelText('Владелец / репозиторий')).toHaveValue(
      'workspace/edited',
    )
    expect(screen.getByLabelText('Токен доступа')).toHaveValue(
      'synthetic-not-a-credential',
    )
    expect(fixture.save).not.toHaveBeenCalled()
    expect(fixture.start).not.toHaveBeenCalled()
    back()
    fireEvent.click(await screen.findByRole('button', { name: 'Покинуть' }))
    expect(
      await screen.findByRole('heading', { name: 'Список синхронизаций' }),
    ).toBeVisible()
    expect(beforeUnload()).toBe(false)
  })

  it('allows view navigation and canonical scope spelling but protects a different namespace', async () => {
    const scoped = {
      ...original,
      namespace: { registry_instance_id: registry, namespace_id: namespace },
    }
    const fixture = mount({
      connection: scoped,
      location: `${path}?registry_instance_id=${registry}&namespace_id=${namespace}`,
    })
    fireEvent.change(screen.getByLabelText('Владелец / репозиторий'), {
      target: { value: 'workspace/edited' },
    })
    screen.getByRole('tab', { name: 'История запусков' }).focus()
    fireEvent.keyDown(screen.getByRole('tab', { name: 'История запусков' }), {
      key: 'Enter',
    })
    await screen.findByRole('navigation', { name: 'История синхронизации' })
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument()
    await act(() =>
      fixture.router.navigate(
        `${path}?namespace_id=${namespace.toUpperCase()}&registry_instance_id=${registry.toUpperCase()}&tab=history&run_id=selected&runs_offset=20&refs_offset=50`,
      ),
    )
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument()
    await act(() =>
      fixture.router.navigate(
        `${path}?project_scope=all&namespace_id=${namespace}&registry_instance_id=${registry}&tab=history`,
      ),
    )
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument()
    await act(() =>
      fixture.router.navigate(
        `${path}?namespace_id=${otherNamespace}&registry_instance_id=${registry}`,
      ),
    )
    const dialog = await screen.findByRole('alertdialog')
    fireEvent.click(within(dialog).getByRole('button', { name: 'Остаться' }))
    await waitFor(() =>
      expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument(),
    )
    screen.getByRole('tab', { name: 'Подключение' }).focus()
    fireEvent.keyDown(screen.getByRole('tab', { name: 'Подключение' }), {
      key: 'Enter',
    })
    expect(await screen.findByLabelText('Владелец / репозиторий')).toHaveValue(
      'workspace/edited',
    )
    expect(fixture.router.state.location.search).not.toContain(otherNamespace)
    expect(fixture.save).not.toHaveBeenCalled()
  })

  it('releases the original leave request when an in-flight save is confirmed', async () => {
    const fixture = mount()
    let resolveSave!: (connection: SyncConnection) => void
    fixture.save.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveSave = resolve
      }),
    )
    fireEvent.change(screen.getByLabelText('Владелец / репозиторий'), {
      target: { value: 'workspace/confirmed' },
    })
    fireEvent.click(
      screen.getByRole('button', { name: 'Сохранить подключение' }),
    )
    back()
    await screen.findByRole('alertdialog')
    const saved = {
      ...original,
      version: 5,
      settings: {
        ...original.settings,
        repository_path: 'workspace/confirmed',
      },
    }
    await act(async () => {
      resolveSave(saved)
    })
    expect(
      await screen.findByRole('heading', { name: 'Список синхронизаций' }),
    ).toBeVisible()
    expect(fixture.save).toHaveBeenCalledTimes(1)
    expect(beforeUnload()).toBe(false)
  })

  it('keeps an unknown save protected even when a refresh matches the edited settings', async () => {
    const fixture = mount()
    const saved = {
      ...original,
      version: 5,
      settings: {
        ...original.settings,
        repository_path: 'workspace/confirmed',
      },
    }
    fixture.save
      .mockRejectedValueOnce(
        new ApiError({ kind: 'network', message: 'fixture offline' }),
      )
      .mockResolvedValueOnce(saved)
    fireEvent.change(screen.getByLabelText('Владелец / репозиторий'), {
      target: { value: 'workspace/confirmed' },
    })
    fireEvent.click(
      screen.getByRole('button', { name: 'Сохранить подключение' }),
    )
    await screen.findByRole('button', {
      name: 'Проверить результат сохранения',
    })
    fixture.updateServer(saved)
    expect(beforeUnload()).toBe(true)
    back()
    const dialog = await screen.findByRole('alertdialog')
    expect(dialog).toHaveTextContent('не подтверждён')
    fireEvent.click(within(dialog).getByRole('button', { name: 'Остаться' }))
    await waitFor(() =>
      expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument(),
    )
    expect(fixture.save).toHaveBeenCalledTimes(1)
    fireEvent.click(
      screen.getByRole('button', { name: 'Проверить результат сохранения' }),
    )
    await screen.findByText('Подключение сохранено')
    expect(fixture.save.mock.calls[1]).toEqual(fixture.save.mock.calls[0])
    back()
    expect(
      await screen.findByRole('heading', { name: 'Список синхронизаций' }),
    ).toBeVisible()
  })

  it('protects a pending and unknown run without replaying it from the leave dialog', async () => {
    const fixture = mount()
    let rejectRun!: (error: unknown) => void
    fixture.start.mockReturnValueOnce(
      new Promise((_resolve, reject) => {
        rejectRun = reject
      }),
    )
    fireEvent.click(
      screen.getByRole('button', { name: 'Синхронизировать сейчас' }),
    )
    expect(beforeUnload()).toBe(true)
    back()
    const pending = await screen.findByRole('alertdialog')
    fireEvent.click(within(pending).getByRole('button', { name: 'Остаться' }))
    await act(async () => {
      rejectRun(new ApiError({ kind: 'network', message: 'fixture offline' }))
    })
    await screen.findByRole('button', { name: 'Проверить результат запуска' })
    back()
    const unknown = await screen.findByRole('alertdialog')
    expect(unknown).toHaveTextContent('не подтверждён')
    fireEvent.click(within(unknown).getByRole('button', { name: 'Остаться' }))
    await waitFor(() =>
      expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument(),
    )
    expect(fixture.start).toHaveBeenCalledTimes(1)
    fixture.start.mockResolvedValueOnce(queued)
    fireEvent.click(
      screen.getByRole('button', { name: 'Проверить результат запуска' }),
    )
    await screen.findByText('Запуск поставлен в очередь')
    expect(fixture.start.mock.calls[1]).toEqual(fixture.start.mock.calls[0])
    expect(beforeUnload()).toBe(false)
    back()
    expect(
      await screen.findByRole('heading', { name: 'Список синхронизаций' }),
    ).toBeVisible()
  })

  it('does not block a clean read-only access check in flight', async () => {
    const fixture = mount()
    let resolveCheck!: (
      result: Awaited<ReturnType<typeof repositorySync.check>>,
    ) => void
    fixture.check.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveCheck = resolve
      }),
    )
    fireEvent.click(screen.getByRole('button', { name: 'Проверить доступ' }))
    expect(beforeUnload()).toBe(false)
    back()
    expect(
      await screen.findByRole('heading', { name: 'Список синхронизаций' }),
    ).toBeVisible()
    await act(async () => {
      resolveCheck({ read: true, write: 'preliminary', message: 'Проверено' })
    })
    expect(fixture.save).not.toHaveBeenCalled()
    expect(fixture.start).not.toHaveBeenCalled()
  })

  it('falls back to the Base page heading if navigation removes its initiator', async () => {
    const fixture = mount()
    fireEvent.change(screen.getByLabelText('Владелец / репозиторий'), {
      target: { value: 'workspace/retained' },
    })
    fixture.updateServer({ ...original, writable: false })
    await waitFor(() =>
      expect(screen.getByLabelText('Владелец / репозиторий')).toBeDisabled(),
    )
    const opener = screen.getByRole('button', {
      name: 'Исчезающий пункт навигации',
    })
    opener.focus()
    fireEvent.click(opener)
    fireEvent.click(await screen.findByRole('button', { name: 'Остаться' }))
    await waitFor(() =>
      expect(
        screen.getByRole('heading', { name: 'Внешний репозиторий' }),
      ).toHaveFocus(),
    )
    expect(screen.getByLabelText('Владелец / репозиторий')).toHaveValue(
      'workspace/retained',
    )
    expect(fixture.save).not.toHaveBeenCalled()
  })

  it('honors an additional parent-owned mode boundary', async () => {
    const fixture = mount({
      parentBoundary: true,
      location: path + '?production=1',
    })
    fireEvent.change(screen.getByLabelText('Владелец / репозиторий'), {
      target: { value: 'workspace/retained' },
    })
    await act(() => fixture.router.navigate(path + '?production=0'))
    const dialog = await screen.findByRole('alertdialog')
    fireEvent.click(within(dialog).getByRole('button', { name: 'Остаться' }))
    expect(fixture.router.state.location.search).toBe('?production=1')
  })

  it.each([403, 409])(
    'does not retain leave protection after a definite HTTP %s run rejection',
    async (status) => {
      const fixture = mount()
      fixture.start.mockRejectedValueOnce(
        new ApiError({ kind: 'api', status, message: 'fixture denied' }),
      )
      fireEvent.click(
        screen.getByRole('button', { name: 'Синхронизировать сейчас' }),
      )
      await screen.findByText('fixture denied')
      await waitFor(() =>
        expect(
          screen.getByRole('button', { name: 'Синхронизировать сейчас' }),
        ).toBeEnabled(),
      )
      expect(beforeUnload()).toBe(false)
      back()
      expect(
        await screen.findByRole('heading', { name: 'Список синхронизаций' }),
      ).toBeVisible()
      expect(fixture.start).toHaveBeenCalledTimes(1)
    },
  )
})
