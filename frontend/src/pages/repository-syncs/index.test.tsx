import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  cleanup,
  within,
} from '@testing-library/react'
import { MemoryRouter, Routes, Route, useLocation } from 'react-router'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { ApiError } from '@/api/client'
import * as apiClient from '@/api/client'
import type { components } from '@/api/schema'
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
function RouterLocation() {
  const location = useLocation()
  return <span data-testid="router-location">{location.pathname}{location.search}{location.hash}</span>
}
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  context.ref = null
  context.all = true
  context.malformed = false
})
describe('production repository synchronization', () => {
  it.each(['Infinity', '-1', '1.5', 'not-a-number', '9007199254740992'])(
    'normalizes invalid list offset %s before requesting data',
    async (offset) => {
      const list = vi.spyOn(repositorySync, 'list').mockResolvedValue({ items: [], total: 0 })
      mount(<RepositorySyncsPage />, `/repository-syncs?offset=${offset}&provider=github&search=service`)
      await screen.findByRole('navigation', { name: 'Синхронизации репозиториев' })
      expect(list.mock.calls[0][0].get('offset')).toBe('0')
      expect(list.mock.calls[0][0].get('provider')).toBe('github')
      expect(list.mock.calls[0][0].get('search')).toBe('service')
      expect(screen.getByRole('button', { name: 'Назад' })).toBeDisabled()
    },
  )
  it.each(['Infinity', '-1', '1.5', 'not-a-number', '9007199254740992'])(
    'normalizes invalid catalog offset %s before requesting data',
    async (offset) => {
      vi.spyOn(repositorySync, 'list').mockResolvedValue({ items: [], total: 0 })
      const catalog = vi.spyOn(apiClient, 'api').mockResolvedValue({ items: [], total: 0 })
      mount(<RepositorySyncsPage />, `/repository-syncs?select_offset=${offset}&select_search=service`)
      fireEvent.click(screen.getByRole('button', { name: 'Добавить синхронизацию' }))
      await screen.findByRole('navigation', { name: 'Доступные репозитории' })
      const query = new URL(catalog.mock.calls[0][0], 'https://unit.invalid').searchParams
      expect(query.get('offset')).toBe('0')
      expect(query.get('search')).toBe('service')
    },
  )
  it.each(['Infinity', '-1', '1.5', 'not-a-number', '9007199254740992'])(
    'normalizes invalid history and refs offset %s before requesting data',
    async (offset) => {
      const runs = vi.spyOn(repositorySync, 'runs').mockResolvedValue({ items: [], total: 0 })
      const refs = vi.spyOn(repositorySync, 'refs').mockResolvedValue({ items: [], total: 0 })
      mount(<RepositorySyncEditor connection={connection} reload={vi.fn()} />,
        `/?tab=history&run_id=run&runs_offset=${offset}&refs_offset=${offset}`)
      await waitFor(() => expect(runs).toHaveBeenCalled())
      await waitFor(() => expect(refs).toHaveBeenCalled())
      expect(runs.mock.calls[0][1]).toBe(0)
      expect(refs.mock.calls[0][2]).toBe(0)
    },
  )
  it('preserves an unaligned catalog offset and advances only that pager', async () => {
    vi.spyOn(repositorySync, 'list').mockResolvedValue({ items: [], total: 55 })
    const catalog = vi.spyOn(apiClient, 'api').mockResolvedValue({ items: [], total: 55 })
    mount(<RepositorySyncsPage />, '/repository-syncs?offset=15&select_offset=15&select_search=service')
    fireEvent.click(screen.getByRole('button', { name: 'Добавить синхронизацию' }))
    const nav = await screen.findByRole('navigation', { name: 'Доступные репозитории' })
    expect(new URL(catalog.mock.calls[0][0], 'https://unit.invalid').searchParams.get('offset')).toBe('15')
    fireEvent.click(within(nav).getByRole('button', { name: 'Далее' }))
    await waitFor(() => expect(catalog).toHaveBeenCalledTimes(2))
    const query = new URL(catalog.mock.calls[1][0], 'https://unit.invalid').searchParams
    expect(query.get('offset')).toBe('35')
    expect(query.get('search')).toBe('service')
  })
  it('preserves unaligned history and refs offsets with their own page sizes', async () => {
    const runs = vi.spyOn(repositorySync, 'runs').mockResolvedValue({ items: [], total: 55 })
    const refs = vi.spyOn(repositorySync, 'refs').mockResolvedValue({ items: [], total: 125 })
    mount(<RepositorySyncEditor connection={connection} reload={vi.fn()} />,
      '/?tab=history&run_id=run&runs_offset=15&refs_offset=15')
    const historyNav = await screen.findByRole('navigation', { name: 'История синхронизации' })
    const refsNav = await screen.findByRole('navigation', { name: 'Результаты веток и тегов' })
    expect(runs.mock.calls[0][1]).toBe(15)
    expect(refs.mock.calls[0][2]).toBe(15)
    fireEvent.click(within(refsNav).getByRole('button', { name: 'Далее' }))
    await waitFor(() => expect(refs.mock.calls.at(-1)?.[2]).toBe(65))
    expect(runs).toHaveBeenCalledTimes(1)
    fireEvent.click(within(historyNav).getByRole('button', { name: 'Далее' }))
    await waitFor(() => expect(runs.mock.calls.at(-1)?.[1]).toBe(35))
    expect(refs.mock.calls.at(-1)?.[2]).toBe(65)
  })
  it('keeps owner filters and unaligned offsets when shared navigation requests the next range', async () => {
    const list = vi.spyOn(repositorySync, 'list').mockResolvedValue({ items: [connection], total: 55 })
    mount(<RepositorySyncsPage />, '/repository-syncs?provider=github&offset=15&search=service')
    await screen.findByRole('link', { name: 'platform/service' })
    fireEvent.click(screen.getByRole('button', { name: 'Далее' }))
    await waitFor(() => expect(list).toHaveBeenCalledTimes(2))
    const next = list.mock.calls[1][0]
    expect(next.get('offset')).toBe('35')
    expect(next.get('provider')).toBe('github')
    expect(next.get('search')).toBe('service')
  })
  it.each([
    { scope: 'all projects', all: true },
    { scope: 'one project', all: false },
  ])('pages and searches the hosted selector in $scope and links the selected UUID in its own namespace', async ({ all }) => {
    const registry = '10000000-0000-4000-8000-000000000001'
    const namespaceA = '20000000-0000-4000-8000-000000000001'
    const namespaceB = '20000000-0000-4000-8000-000000000002'
    const groupA = '30000000-0000-4000-8000-000000000001'
    const groupB = '30000000-0000-4000-8000-000000000002'
    const selected = '40000000-0000-4000-8000-000000000100'
    const first: components['schemas']['CatalogRepository'][] = Array.from({ length: 20 }, (_, index) => ({
      id: `40000000-0000-4000-8000-${String(index + 1).padStart(12, '0')}`,
      kind: 'hosted', ready: true, state: 'active', visibility: 'private',
      public_name: index === 0 && all ? 'platform/service' : `platform/worker-${index}`,
      slug: index === 0 && all ? 'service' : `worker-${index}`,
      provider_refs: {}, group_id: all ? groupA : groupB,
      namespace: { registry_instance_id: registry, namespace_id: all ? namespaceA : namespaceB },
    }))
    const second: components['schemas']['CatalogRepository'] = {
      ...first[0], id: selected, public_name: 'platform/service', slug: 'service',
      group_id: groupB, namespace: { registry_instance_id: registry, namespace_id: namespaceB },
    }
    const pages: components['schemas']['CatalogPage'][] = [
      { items: first, total: 21 }, { items: [second], total: 21 }, { items: [first[1]], total: 1 },
    ]
    context.all = all
    context.ref = { registry_instance_id: registry, namespace_id: all ? namespaceA : namespaceB }
    vi.spyOn(repositorySync, 'list').mockResolvedValue({ items: [], total: 0 })
    const catalog = vi.spyOn(apiClient, 'api')
      .mockResolvedValueOnce(pages[0]).mockResolvedValueOnce(pages[1]).mockResolvedValueOnce(pages[2])
    const save = vi.spyOn(repositorySync, 'save')
    const start = vi.spyOn(repositorySync, 'start')
    mount(<RepositorySyncsPage groupId={all ? undefined : groupB} />,
      `/repository-syncs?provider=github${all ? '&project_scope=all' : `&registry_instance_id=${registry}&namespace_id=${namespaceB}`}`)
    await screen.findByText(/Синхронизации не найдены/)
    expect(catalog).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Добавить синхронизацию' }))
    const picker = await screen.findByLabelText('Доступный hosted-репозиторий без подключения')
    expect(within(picker).getAllByRole('option')).toHaveLength(21)
    const query = (index: number) => new URL(catalog.mock.calls[index][0], 'http://fixture.invalid').searchParams
    expect(query(0).get('sync_available')).toBe('true')
    expect(query(0).get('limit')).toBe('20')
    expect(query(0).get('offset')).toBe('0')
    expect(query(0).get('group_id')).toBe(all ? null : groupB)
    fireEvent.change(picker, { target: { value: first[0].id } })
    expect(screen.getByRole('link', { name: 'Настроить подключение' }).getAttribute('href')).toContain(first[0].id)

    const selector = screen.getByRole('region', { name: 'Выбор репозитория' })
    fireEvent.click(within(selector).getByRole('button', { name: 'Далее' }))
    await waitFor(() => expect(catalog).toHaveBeenCalledTimes(2))
    await screen.findByText('21–21 из 21')
    expect(query(1).get('offset')).toBe('20')
    expect(query(1).get('group_id')).toBe(all ? null : groupB)
    expect(screen.queryByRole('link', { name: 'Настроить подключение' })).not.toBeInTheDocument()
    fireEvent.change(screen.getByLabelText('Доступный hosted-репозиторий без подключения'), { target: { value: selected } })
    const link = new URL(screen.getByRole('link', { name: 'Настроить подключение' }).getAttribute('href')!, 'http://fixture.invalid')
    expect(link.pathname).toBe(`/catalog/repositories/${selected}/sync`)
    expect(link.searchParams.get('registry_instance_id')).toBe(registry)
    expect(link.searchParams.get('namespace_id')).toBe(namespaceB)
    expect(link.searchParams.get('project_scope')).toBe(all ? 'all' : null)
    expect(link.searchParams.get('provider')).toBe('github')

    fireEvent.change(screen.getByLabelText('Поиск репозитория Forge'), { target: { value: 'worker-1' } })
    await waitFor(() => expect(catalog).toHaveBeenCalledTimes(3))
    await screen.findByText('1–1 из 1')
    expect(query(2).get('search')).toBe('worker-1')
    expect(query(2).get('offset')).toBe('0')
    expect(query(2).get('group_id')).toBe(all ? null : groupB)
    expect(screen.queryByRole('link', { name: 'Настроить подключение' })).not.toBeInTheDocument()
    expect(save).not.toHaveBeenCalled()
    expect(start).not.toHaveBeenCalled()
    expect(catalog.mock.calls.every(([, init]) => init?.method === undefined)).toBe(true)
  })
  it('creates a disabled manual connection from version zero without persisting its credential in browser storage', async () => {
    const save = vi.spyOn(repositorySync, 'save').mockResolvedValue({
      ...connection, version: 1,
      settings: { ...connection.settings, direction: 'disabled', automatic: false },
    })
    mount(<RepositorySyncEditor connection={{
      ...connection, version: 0, has_credential: false,
      settings: { ...connection.settings, repository_path: '', direction: 'disabled', automatic: false },
    }} reload={vi.fn()} />)
    expect(screen.getByRole('radio', { name: /^Отключено / })).toBeChecked()
    expect(screen.getByRole('radio', { name: /^Вручную / })).toBeChecked()
    expect(screen.getByLabelText('Токен доступа')).toBeRequired()
    expect(screen.getByText('Синхронизировать сейчас')).toBeDisabled()
    fireEvent.change(screen.getByLabelText('Владелец / репозиторий'), { target: { value: 'team/new' } })
    fireEvent.change(screen.getByLabelText('Токен доступа'), { target: { value: 'fixture-create-credential' } })
    fireEvent.click(screen.getByText('Сохранить подключение'))
    await screen.findByText('Подключение сохранено')
    expect(save.mock.calls[0][1]).toEqual({ expected_version: 0,
      settings: { ...connection.settings, repository_path: 'team/new', direction: 'disabled', automatic: false },
      token: 'fixture-create-credential' })
    expect(screen.getByLabelText('Токен доступа')).toHaveValue('')
    expect(JSON.stringify(window.localStorage)).not.toContain('fixture-create-credential')
    expect(JSON.stringify(window.sessionStorage)).not.toContain('fixture-create-credential')
  })
  it.each([
    { direction: 'disabled', label: /^Отключено /, automatic: false },
    { direction: 'pull', label: /^Внешний → Forge /, automatic: true },
    { direction: 'push', label: /^Forge → внешний /, automatic: false },
    { direction: 'both', label: /^В обе стороны /, automatic: true },
  ] as const)('saves the user-selected $direction direction and automatic=$automatic without starting a run', async ({ direction, label, automatic }) => {
    const selectedSettings = { ...connection.settings, direction, automatic }
    const save = vi.spyOn(repositorySync, 'save').mockResolvedValue({
      ...connection, version: 2, settings: selectedSettings,
    })
    const check = vi.spyOn(repositorySync, 'check').mockRejectedValue(new Error('fixture unexpected check'))
    const start = vi.spyOn(repositorySync, 'start').mockRejectedValue(new Error('fixture unexpected run'))
    mount(<RepositorySyncEditor connection={connection} reload={vi.fn()} />)

    const intermediate = screen.getByRole('radio', {
      name: direction === 'pull' ? /^Forge → внешний / : /^Внешний → Forge /,
    })
    fireEvent.click(intermediate)
    expect(intermediate).toBeChecked()
    const selectedDirection = screen.getByRole('radio', { name: label })
    fireEvent.click(selectedDirection)
    expect(selectedDirection).toBeChecked()
    if (intermediate !== selectedDirection) expect(intermediate).not.toBeChecked()

    fireEvent.click(screen.getByRole('radio', { name: /^Автоматически / }))
    expect(screen.getByRole('radio', { name: /^Автоматически / })).toBeChecked()
    if (!automatic) fireEvent.click(screen.getByRole('radio', { name: /^Вручную / }))
    expect(screen.getByRole('radio', { name: automatic ? /^Автоматически / : /^Вручную / })).toBeChecked()
    expect(screen.getByRole('radio', { name: automatic ? /^Вручную / : /^Автоматически / })).not.toBeChecked()
    expect(screen.getAllByRole('radio', { checked: true })).toHaveLength(2)
    expect(save).not.toHaveBeenCalled()
    expect(check).not.toHaveBeenCalled()
    expect(start).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: 'Сохранить подключение' }))
    await screen.findByText('Подключение сохранено')
    expect(save).toHaveBeenCalledTimes(1)
    expect(save.mock.calls[0]).toEqual(['repo', {
      expected_version: 1, settings: selectedSettings,
    }, expect.stringMatching(/^[0-9a-f-]{36}$/)])
    expect(selectedDirection).toBeChecked()
    expect(screen.getByRole('radio', { name: automatic ? /^Автоматически / : /^Вручную / })).toBeChecked()
    expect(check).not.toHaveBeenCalled()
    expect(start).not.toHaveBeenCalled()
  })
  it('requires a new credential after changing provider or instance and restores the saved-provider requirement', () => {
    mount(<RepositorySyncEditor connection={connection} reload={vi.fn()} />)
    expect(screen.getByLabelText('Токен доступа')).not.toBeRequired()
    fireEvent.change(screen.getByLabelText('Провайдер'), { target: { value: 'gitlab' } })
    expect(screen.getByLabelText('Адрес инстанса GitLab')).toHaveValue('')
    expect(screen.getByLabelText('Токен доступа')).toBeRequired()
    expect(screen.getByText('Проверить доступ')).toBeDisabled()
    fireEvent.change(screen.getByLabelText('Адрес инстанса GitLab'), { target: { value: 'https://gitlab.example.invalid' } })
    expect(screen.getByText('Проверить доступ')).toBeDisabled()
    fireEvent.change(screen.getByLabelText('Токен доступа'), { target: { value: 'fixture-rotated-credential' } })
    expect(screen.getByText('Проверить доступ')).toBeEnabled()
    fireEvent.change(screen.getByLabelText('Токен доступа'), { target: { value: '' } })
    fireEvent.change(screen.getByLabelText('Провайдер'), { target: { value: 'github' } })
    expect(screen.getByLabelText('Адрес GitHub')).toHaveValue('https://github.com')
    expect(screen.getByLabelText('Токен доступа')).not.toBeRequired()
    expect(screen.getByText('Проверить доступ')).toBeEnabled()
  })
  it('checks an unsaved draft without saving or starting a run', async () => {
    const check = vi.spyOn(repositorySync, 'check').mockResolvedValue({
      read: true, write: 'preliminary', message: 'Доступ проверен без сохранения.',
    })
    const save = vi.spyOn(repositorySync, 'save')
    const start = vi.spyOn(repositorySync, 'start')
    const reload = vi.fn()
    mount(<RepositorySyncEditor connection={connection} reload={reload} />)
    fireEvent.change(screen.getByLabelText('Владелец / репозиторий'), { target: { value: 'team/draft' } })
    fireEvent.click(screen.getByText('Проверить доступ'))
    await screen.findByText('Доступ проверен без сохранения.')
    expect(check.mock.calls[0]).toEqual(['repo', { settings: { ...connection.settings, repository_path: 'team/draft' } }])
    expect(save).not.toHaveBeenCalled()
    expect(start).not.toHaveBeenCalled()
    expect(reload).not.toHaveBeenCalled()
    expect(screen.getByLabelText('Владелец / репозиторий')).toHaveValue('team/draft')
    expect(screen.getByText('Сохранить подключение')).toBeEnabled()
  })
  it('preserves loaded settings and the draft when a background refresh fails', async () => {
    const get = vi.spyOn(repositorySync, 'get').mockResolvedValueOnce(connection)
      .mockRejectedValue(new ApiError({ kind: 'network', message: 'Failed to fetch' }))
    const cache = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={cache}><MemoryRouter initialEntries={['/catalog/repositories/repo/sync']}>
      <Routes><Route path="/catalog/repositories/:id/sync" element={<RepositorySyncPage />} /></Routes>
    </MemoryRouter></QueryClientProvider>)
    const input = await screen.findByLabelText('Владелец / репозиторий')
    fireEvent.change(input, { target: { value: 'team/unsaved' } })
    await cache.invalidateQueries({ queryKey: ['repository-sync', 'repo'] })
    await screen.findByText('Не удалось обновить состояние. Показаны последние загруженные данные.')
    expect(get).toHaveBeenCalledTimes(2)
    expect(screen.getByLabelText('Владелец / репозиторий')).toHaveValue('team/unsaved')
    expect(screen.queryByText('Подключение недоступно.')).not.toBeInTheDocument()
    cache.clear()
  })
  it('resets server pagination when changing provider while retaining the search', async () => {
    const list = vi.spyOn(repositorySync, 'list').mockResolvedValue({ items: [], total: 0 })
    mount(<RepositorySyncsPage />, '/repository-syncs?project_scope=all&search=service&offset=40')
    await screen.findByText(/Синхронизации не найдены/)
    fireEvent.change(screen.getByLabelText('Провайдер'), { target: { value: 'gitlab' } })
    await waitFor(() => expect(list.mock.lastCall?.[0].get('provider')).toBe('gitlab'))
    expect(list.mock.lastCall?.[0].has('offset')).toBe(false)
    expect(list.mock.lastCall?.[0].get('search')).toBe('service')
  })
  it('pages the list and resets its offset when editing or clearing search and state while retaining project and provider', async () => {
    const group = '30000000-0000-4000-8000-000000000002'
    const list = vi.spyOn(repositorySync, 'list').mockImplementation(async (query) => {
      const offset = Number(query.get('offset') ?? 0)
      const total = query.get('search') === 'narrow' ? 1 : query.get('state') === 'failed' ? 21 : 41
      return { total, items: Array.from({ length: Math.min(20, total - offset) }, (_, index) => ({
        ...connection, repository_id: `fixture-list-${offset + index}`,
        repository_name: `fixture/service-${offset + index}`,
        settings: { ...connection.settings, provider: 'gitlab' as const, instance_url: 'https://fixture-gitlab.invalid' },
      })) }
    })
    mount(<><RepositorySyncsPage groupId={group} /><RouterLocation /></>,
      '/repository-syncs?provider=gitlab&state=conflict&search=service')
    const url = () => new URL(screen.getByTestId('router-location').textContent!, 'https://fixture-forge.invalid').searchParams
    const expectQuery = async (offset: string | null, search: string | null, state: string | null) => {
      await waitFor(() => expect(Object.fromEntries(list.mock.lastCall![0])).toEqual({
        provider: 'gitlab', limit: '20', group_id: group,
        ...(offset === null ? {} : { offset }), ...(search === null ? {} : { search }),
        ...(state === null ? {} : { state }),
      }))
      expect(url().get('offset')).toBe(offset)
      expect(url().get('search')).toBe(search)
      expect(url().get('state')).toBe(state)
      expect(url().get('provider')).toBe('gitlab')
    }
    await screen.findByText('1–20 из 41')
    expect(screen.getByRole('button', { name: 'Назад' })).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: 'Далее' }))
    await screen.findByText('21–40 из 41')
    await expectQuery('20', 'service', 'conflict')
    expect(screen.getByRole('link', { name: 'fixture/service-20' })).toBeVisible()
    fireEvent.click(screen.getByRole('button', { name: 'Назад' }))
    await screen.findByText('1–20 из 41')
    await expectQuery('0', 'service', 'conflict')
    fireEvent.click(screen.getByRole('button', { name: 'Далее' }))
    await screen.findByText('21–40 из 41')
    fireEvent.change(screen.getByLabelText('Поиск'), { target: { value: 'narrow' } })
    await screen.findByText('1–1 из 1')
    await expectQuery(null, 'narrow', 'conflict')
    expect(screen.getByRole('button', { name: 'Далее' })).toBeDisabled()
    fireEvent.change(screen.getByLabelText('Поиск'), { target: { value: '' } })
    await screen.findByText('1–20 из 41')
    await expectQuery(null, null, 'conflict')
    fireEvent.click(screen.getByRole('button', { name: 'Далее' }))
    await screen.findByText('21–40 из 41')
    fireEvent.change(screen.getByLabelText('Состояние'), { target: { value: 'failed' } })
    await screen.findByText('1–20 из 21')
    await expectQuery(null, null, 'failed')
    fireEvent.click(screen.getByRole('button', { name: 'Далее' }))
    await screen.findByText('21–21 из 21')
    fireEvent.change(screen.getByLabelText('Состояние'), { target: { value: '' } })
    await screen.findByText('1–20 из 41')
    await expectQuery(null, null, null)
    expect(screen.getByLabelText('Поиск')).toHaveValue('')
    expect(screen.getByLabelText('Состояние')).toHaveValue('')
    expect(screen.getByLabelText('Провайдер')).toHaveValue('gitlab')
  })
  it('pages history and resets the old refs offset only when selecting another run', async () => {
    const allRuns: SyncRun[] = Array.from({ length: 21 }, (_, index) => ({
      id: `fixture-run-${index}`, repository_id: 'repo', connection_version: index + 1,
      settings: { ...connection.settings, repository_path: `fixture/run-${index}` },
      source: 'manual', state: 'success', message: null, cancel_requested: false,
      created_at: new Date(Date.UTC(2026, 9, 18, 10, index)).toISOString(),
      started_at: null, finished_at: '2026-10-18T11:00:00Z',
    }))
    const runs = vi.spyOn(repositorySync, 'runs').mockImplementation(async (_repository, offset) => ({
      items: allRuns.slice(offset, offset + 20), total: allRuns.length,
    }))
    const refs = vi.spyOn(repositorySync, 'refs').mockImplementation(async (_repository, run, offset) => ({
      items: [{ ref_name: `refs/heads/${run}-${offset}`, direction: 'pull',
        old_sha: null, new_sha: 'a'.repeat(40), state: 'applied', event_delivered: true }],
      total: run === 'fixture-run-0' ? 51 : 1,
    }))
    mount(<><RepositorySyncEditor connection={{ ...connection, last_run: allRuns[0] }} reload={vi.fn()} /><RouterLocation /></>,
      '/?tab=history&provider=github&project_scope=all&runs_offset=0&run_id=fixture-run-0&refs_offset=50')
    const url = () => new URL(screen.getByTestId('router-location').textContent!, 'https://fixture-forge.invalid').searchParams
    const historyNavigation = () => within(screen.getByRole('navigation', { name: 'История синхронизации' }))
    const historyNext = () => historyNavigation().getByRole('button', { name: 'Далее' })
    const historyBack = () => historyNavigation().getByRole('button', { name: 'Назад' })
    await screen.findByText('1–20 из 21')
    expect((await screen.findByText('refs/heads/fixture-run-0-50')).closest('tr')).toBeVisible()
    expect(historyBack()).toBeDisabled()
    fireEvent.click(historyNext())
    await screen.findByText('21–21 из 21')
    await waitFor(() => expect(runs.mock.lastCall?.slice(0, 2)).toEqual(['repo', 20]))
    expect(url().get('runs_offset')).toBe('20')
    expect(url().get('run_id')).toBe('fixture-run-0')
    expect(url().get('refs_offset')).toBe('50')
    expect(historyNext()).toBeDisabled()
    fireEvent.click(historyBack())
    await screen.findByText('1–20 из 21')
    await waitFor(() => expect(runs.mock.lastCall?.slice(0, 2)).toEqual(['repo', 0]))
    expect(url().get('runs_offset')).toBe('0')
    expect(url().get('refs_offset')).toBe('50')
    const otherRun = screen.getByText('https://github.com/fixture/run-1.git · v2').closest('tr')!
    fireEvent.click(within(otherRun).getByRole('button'))
    expect((await screen.findByText('refs/heads/fixture-run-1-0')).closest('tr')).toBeVisible()
    expect(refs.mock.lastCall?.slice(0, 3)).toEqual(['repo', 'fixture-run-1', 0])
    expect(url().get('run_id')).toBe('fixture-run-1')
    expect(url().get('runs_offset')).toBe('0')
    expect(url().has('refs_offset')).toBe(false)
    expect(url().get('provider')).toBe('github')
    expect(url().get('project_scope')).toBe('all')
    expect(url().get('tab')).toBe('history')
    expect(screen.queryByText('refs/heads/fixture-run-0-50')).not.toBeInTheDocument()
    expect(screen.getByText('1–1 из 1')).toBeVisible()
    expect(refs.mock.calls.some(([repository, run, offset]) => repository === 'repo' && run === 'fixture-run-1' && offset === 50)).toBe(false)
  })
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
  it('explains lost repository admission while preserving reconciling and unknown ref outcomes', async () => {
    const run: SyncRun = {
      id: 'run', repository_id: 'repo', connection_version: 1,
      settings: connection.settings, source: 'manual', state: 'reconciling',
      message: 'namespace_admission_lost', cancel_requested: false,
      created_at: '2026-10-18T10:00:00Z', started_at: '2026-10-18T10:00:01Z',
      finished_at: null,
    }
    vi.spyOn(repositorySync, 'runs').mockResolvedValue({ items: [run], total: 1 })
    vi.spyOn(repositorySync, 'refs').mockResolvedValue({ items: [{
      ref_name: 'refs/heads/qa-recovery', direction: 'push',
      old_sha: 'a'.repeat(40), new_sha: 'b'.repeat(40),
      state: 'unknown', reason: 'outcome_unknown', event_delivered: false,
    }], total: 1 })
    mount(
      <RepositorySyncEditor connection={{ ...connection, active_run: run }} reload={vi.fn()} />,
      '/?tab=history&run_id=run',
    )
    const historyRow = await screen.findByRole('row', {
      name: /Не удалось подтвердить доступ к репозиторию/,
    })
    expect(within(historyRow).getByText('Уточняется результат')).toBeVisible()
    expect(within(historyRow).getByText(
      'Не удалось подтвердить доступ к репозиторию. Результат синхронизации уточняется.',
    )).toBeVisible()
    expect(screen.queryByText('namespace_admission_lost')).not.toBeInTheDocument()
    const referenceRow = await screen.findByRole('row', { name: /refs\/heads\/qa-recovery/ })
    expect(within(referenceRow).getByText('Неизвестный результат')).toBeVisible()
    expect(within(referenceRow).getByText('Ожидается сверка фактического SHA')).toBeVisible()
    expect(within(referenceRow).queryByText('Передано')).not.toBeInTheDocument()
    expect(screen.queryByText('Успешно')).not.toBeInTheDocument()
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
        new ApiError({ kind: 'network', message: 'Failed to fetch' }),
      )
      .mockResolvedValue({ ...connection, version: 2 })
    const initialLocation = '/catalog/repositories/repo/sync?provider=github'
    mount(<><RepositorySyncEditor connection={connection} reload={vi.fn()} /><RouterLocation /></>, initialLocation)
    fireEvent.change(screen.getByLabelText('Токен доступа'), {
      target: { value: 'fixture-test-token' },
    })
    fireEvent.click(screen.getByText('Сохранить подключение'))
    await screen.findByText('Не удалось связаться с сервером.')
    expect(screen.queryByText('Failed to fetch')).not.toBeInTheDocument()
    expect(screen.getByLabelText('Токен доступа')).toBeDisabled()
    expect(screen.getByTestId('router-location').textContent).toBe(initialLocation)
    fireEvent.click(await screen.findByText('Проверить результат сохранения'))
    await screen.findByText('Подключение сохранено')
    expect(save.mock.calls[0]).toEqual(save.mock.calls[1])
    expect(screen.getByLabelText('Токен доступа')).toHaveValue('')
    expect(screen.getByTestId('router-location').textContent).toBe(initialLocation)
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
  it('retries an unknown manual run with the original operation instead of creating another', async () => {
    const queued: SyncRun = {
      id: 'queued', repository_id: 'repo', connection_version: 1,
      settings: connection.settings, source: 'manual', state: 'queued',
      message: null, cancel_requested: false, created_at: '2026-10-18T10:00:00Z',
      started_at: null, finished_at: null,
    }
    const start = vi.spyOn(repositorySync, 'start')
      .mockRejectedValueOnce(new ApiError({ kind: 'network', message: 'Failed to fetch' }))
      .mockResolvedValue(queued)
    const reload = vi.fn().mockResolvedValue(undefined)
    mount(<RepositorySyncEditor connection={connection} reload={reload} />)
    fireEvent.click(screen.getByText('Синхронизировать сейчас'))
    await screen.findByText('Не удалось связаться с сервером.')
    fireEvent.click(await screen.findByText('Проверить результат запуска'))
    await screen.findByText('Запуск поставлен в очередь')
    expect(start).toHaveBeenCalledTimes(2)
    expect(start.mock.calls[0]).toEqual(start.mock.calls[1])
    expect(start.mock.calls[0][1]).toMatch(/^[0-9a-f-]{36}$/)
    expect(reload).toHaveBeenCalledTimes(1)
    expect(screen.queryByText('Проверить результат запуска')).not.toBeInTheDocument()
  })
  it.each([
    { state: 'queued', cancelRequested: false, label: 'В очереди', refState: 'planned' },
    { state: 'stopping', cancelRequested: true, label: 'Останавливается', refState: 'planned' },
    { state: 'running', cancelRequested: true, label: 'Останавливается', refState: 'unknown' },
    { state: 'reconciling', cancelRequested: true, label: 'Останавливается · сверка refs', refState: 'unknown' },
  ])('keeps $state cancellation truthful and preserves history while saving only disable', async ({ state, cancelRequested, label, refState }) => {
    const active: SyncRun = {
      id: 'active', repository_id: 'repo', connection_version: 1,
      settings: connection.settings, source: 'manual', state,
      message: null, cancel_requested: cancelRequested, created_at: '2026-10-18T10:00:00Z',
      started_at: state === 'queued' ? null : '2026-10-18T10:00:01Z', finished_at: null,
    }
    const previous: SyncRun = {
      ...active, id: 'previous', state: 'success', cancel_requested: false,
      created_at: '2026-10-17T10:00:00Z', started_at: '2026-10-17T10:00:01Z',
      finished_at: '2026-10-17T10:00:02Z',
    }
    vi.spyOn(repositorySync, 'runs').mockResolvedValue({ items: [active, previous], total: 2 })
    vi.spyOn(repositorySync, 'refs').mockResolvedValue({ items: [{
      ref_name: 'refs/heads/qa-active', direction: 'pull',
      old_sha: null, new_sha: 'b'.repeat(40), state: refState,
      reason: refState === 'unknown' ? 'outcome_unknown' : null, event_delivered: false,
    }], total: 1 })
    const start = vi.spyOn(repositorySync, 'start')
    const save = vi.spyOn(repositorySync, 'save').mockResolvedValue({
      ...connection, version: 2, settings: { ...connection.settings, direction: 'disabled' },
      active_run: { ...active, state: 'stopping', cancel_requested: true },
    })
    mount(
      <RepositorySyncEditor connection={{ ...connection, active_run: active }} reload={vi.fn()} />,
      '/?run_id=active',
    )
    expect(screen.getByText(label)).toBeVisible()
    const runButton = screen.getByRole('button', { name: 'Синхронизировать сейчас' })
    expect(runButton).toBeDisabled()
    fireEvent.click(runButton)
    expect(start).not.toHaveBeenCalled()
    expect(screen.getByLabelText('Провайдер')).toBeDisabled()
    expect(screen.getByLabelText('Владелец / репозиторий')).toBeDisabled()
    expect(screen.getByLabelText('Токен доступа')).toBeDisabled()
    expect(screen.getByRole('radio', { name: /^Автоматически / })).toBeDisabled()
    expect(screen.getByRole('radio', { name: /^В обе стороны / })).toBeDisabled()
    expect(screen.getByRole('radio', { name: /^Отключено / })).toBeEnabled()

    fireEvent.mouseDown(screen.getByRole('tab', { name: 'История запусков' }), { button: 0, ctrlKey: false })
    const activeRow = await screen.findByRole('row', { name: new RegExp(label) })
    expect(within(activeRow).getByText(label)).toBeVisible()
    expect(within(activeRow).getByText('https://github.com/team/service.git · v1')).toBeVisible()
    const referenceRow = await screen.findByRole('row', { name: /refs\/heads\/qa-active/ })
    expect(within(referenceRow).getByText(refState === 'unknown' ? 'Неизвестный результат' : 'Ожидает передачи')).toBeVisible()
    expect(within(referenceRow).queryByText('Передано')).not.toBeInTheDocument()
    expect(within(activeRow).queryByText('Успешно')).not.toBeInTheDocument()

    fireEvent.mouseDown(screen.getByRole('tab', { name: 'Подключение' }), { button: 0, ctrlKey: false })
    fireEvent.click(screen.getByText('Отключить синхронизацию'))
    fireEvent.click(screen.getByText('Сохранить подключение'))
    await screen.findByText('Подключение сохранено')
    expect(save).toHaveBeenCalledTimes(1)
    expect(save.mock.calls[0][1]).toEqual({
      expected_version: 1, settings: { ...connection.settings, direction: 'disabled' },
    })
    expect(start).not.toHaveBeenCalled()
    fireEvent.mouseDown(screen.getByRole('tab', { name: 'История запусков' }), { button: 0, ctrlKey: false })
    await screen.findByRole('row', { name: new RegExp(label) })
    expect(screen.getByRole('row', { name: /Успешно/ })).toBeVisible()
    expect(screen.getAllByRole('row').filter(row => row.textContent?.includes('https://github.com/team/service.git · v1'))).toHaveLength(2)
  })
  it.each([
    { reason: 'namespace_read_only', explanation: 'проект в архиве' },
    { reason: 'storage_pending', explanation: 'хранилище недоступно' },
  ])('preserves saved direction and history during $reason pause without offering mutations', async ({ reason, explanation }) => {
    const previous: SyncRun = {
      id: 'previous', repository_id: 'repo', connection_version: 1,
      settings: { ...connection.settings, repository_path: 'team/previous' },
      source: 'manual', state: 'success', message: null, cancel_requested: false,
      created_at: '2026-10-17T10:00:00Z', started_at: '2026-10-17T10:00:01Z',
      finished_at: '2026-10-17T10:00:02Z',
    }
    vi.spyOn(repositorySync, 'runs').mockResolvedValue({ items: [previous], total: 1 })
    vi.spyOn(repositorySync, 'refs').mockResolvedValue({ items: [{
      ref_name: 'refs/heads/qa-previous', direction: 'pull', old_sha: null,
      new_sha: 'a'.repeat(40), state: 'applied', event_delivered: true,
    }], total: 1 })
    const save = vi.spyOn(repositorySync, 'save')
    const check = vi.spyOn(repositorySync, 'check')
    const start = vi.spyOn(repositorySync, 'start')
    mount(<RepositorySyncEditor connection={{
      ...connection, version: 2, writable: false, paused_reason: reason, last_run: previous,
      settings: { ...connection.settings, automatic: true },
    }} reload={vi.fn()} />, '/?run_id=previous')
    expect(screen.getByText(new RegExp(`Синхронизация приостановлена: ${explanation}`))).toBeVisible()
    expect(screen.getByRole('radio', { name: /^В обе стороны / })).toBeChecked()
    expect(screen.getByRole('radio', { name: /^Автоматически / })).toBeChecked()
    expect(screen.getByRole('radio', { name: /^Отключено / })).not.toBeChecked()
    expect(screen.getByLabelText('Токен доступа')).toBeDisabled()
    for (const name of ['Синхронизировать сейчас', 'Сохранить подключение', 'Проверить доступ']) {
      const button = screen.getByRole('button', { name })
      expect(button).toBeDisabled()
      fireEvent.click(button)
    }
    expect(save).not.toHaveBeenCalled()
    expect(check).not.toHaveBeenCalled()
    expect(start).not.toHaveBeenCalled()
    fireEvent.mouseDown(screen.getByRole('tab', { name: 'История запусков' }), { button: 0, ctrlKey: false })
    const historyRow = await screen.findByRole('row', { name: /Успешно/ })
    expect(within(historyRow).getByText('https://github.com/team/previous.git · v1')).toBeVisible()
    const referenceRow = await screen.findByRole('row', { name: /refs\/heads\/qa-previous/ })
    expect(within(referenceRow).getByText('Передано')).toBeVisible()
    expect(save).not.toHaveBeenCalled()
    expect(check).not.toHaveBeenCalled()
    expect(start).not.toHaveBeenCalled()
  })
  it.each(['history', 'refs'] as const)('preserves visible snapshots and pagination during a transient %s failure and retries the same selection', async (failed) => {
    const previous: SyncRun = {
      id: 'previous', repository_id: 'repo', connection_version: 1,
      settings: { ...connection.settings, repository_path: 'team/previous' },
      source: 'manual', state: 'success', message: null, cancel_requested: false,
      created_at: '2026-10-17T10:00:00Z', started_at: '2026-10-17T10:00:01Z',
      finished_at: '2026-10-17T10:00:02Z',
    }
    const history = { items: [previous], total: 21 }
    const reference = { items: [{
      ref_name: 'refs/heads/qa-preserved', direction: 'pull', old_sha: null,
      new_sha: 'a'.repeat(40), state: 'applied', event_delivered: true,
    }], total: 51 }
    const runs = vi.spyOn(repositorySync, 'runs').mockResolvedValue(history)
    const refs = vi.spyOn(repositorySync, 'refs').mockResolvedValue(reference)
    const unavailable = new ApiError({ kind: 'network', message: 'fixture observation unavailable' })
    if (failed === 'history') runs.mockResolvedValueOnce(history).mockRejectedValueOnce(unavailable)
    else refs.mockResolvedValueOnce(reference).mockRejectedValueOnce(unavailable)
    const cache = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={cache}>
      <MemoryRouter initialEntries={['/?tab=history&run_id=previous&runs_offset=20&refs_offset=50']}>
        <RepositorySyncEditor connection={{ ...connection, version: 2, last_run: previous }} reload={vi.fn()} />
      </MemoryRouter>
    </QueryClientProvider>)
    await screen.findByRole('row', { name: /https:\/\/github.com\/team\/previous.git · v1/ })
    await screen.findByRole('row', { name: /refs\/heads\/qa-preserved/ })
    await act(async () => {
      await cache.invalidateQueries({ queryKey: failed === 'history'
        ? ['sync-runs', 'repo', 20] : ['sync-refs', 'repo', 'previous', 50] })
    })
    const alert = await screen.findByRole('alert')
    expect(alert).toHaveTextContent(failed === 'history' ? 'История недоступна.' : 'Результаты недоступны.')
    expect(screen.getByRole('row', { name: /https:\/\/github.com\/team\/previous.git · v1/ })).toBeVisible()
    expect(screen.getByRole('row', { name: /refs\/heads\/qa-preserved/ })).toBeVisible()
    fireEvent.click(within(alert).getByRole('button', { name: 'Повторить' }))
    await waitFor(() => expect(screen.queryByRole('alert')).not.toBeInTheDocument())
    expect(failed === 'history' ? runs : refs).toHaveBeenCalledTimes(3)
    expect(runs.mock.calls.every(([repository, offset]) => repository === 'repo' && offset === 20)).toBe(true)
    expect(refs.mock.calls.every(([repository, run, offset]) => repository === 'repo' && run === 'previous' && offset === 50)).toBe(true)
    expect(screen.getByText('21–21 из 21')).toBeVisible()
    expect(screen.getByText('51–51 из 51')).toBeVisible()
    expect(within(screen.getByRole('row', { name: /refs\/heads\/qa-preserved/ })).getByText(/∅ → aaaaaaaaaaaa/)).toBeVisible()
    cache.clear()
  })
  it.each([403, 409])('does not replay a known HTTP %s run rejection as an unknown operation', async (status) => {
    const queued: SyncRun = {
      id: 'next-attempt', repository_id: 'repo', connection_version: 1,
      settings: connection.settings, source: 'manual', state: 'queued',
      message: null, cancel_requested: false, created_at: '2026-10-18T10:00:00Z',
      started_at: null, finished_at: null,
    }
    const start = vi.spyOn(repositorySync, 'start')
      .mockRejectedValueOnce(new ApiError({ kind: 'api', status,
        message: status === 403 ? 'git_access_denied' : 'sync_running' }))
      .mockResolvedValue(queued)
    const reload = vi.fn().mockResolvedValue(undefined)
    mount(<RepositorySyncEditor connection={connection} reload={reload} />)
    const button = screen.getByRole('button', { name: 'Синхронизировать сейчас' })
    fireEvent.click(button)
    await screen.findByText(status === 403
      ? 'Ошибка доступа: проверьте токен и права на репозиторий.'
      : 'Настройки нельзя менять во время запуска; доступно отключение.')
    expect(button).toBeEnabled()
    expect(screen.getByLabelText('Токен доступа')).toBeEnabled()
    expect(screen.queryByRole('button', { name: 'Проверить результат запуска' })).not.toBeInTheDocument()
    expect(reload).not.toHaveBeenCalled()
    fireEvent.click(button)
    await screen.findByText('Запуск поставлен в очередь')
    expect(start).toHaveBeenCalledTimes(2)
    const keys = start.mock.calls.map(([repository, key]) => {
      expect(repository).toBe('repo')
      expect(key).toMatch(/^[0-9a-f-]{36}$/)
      return key
    })
    expect(keys[0]).not.toBe(keys[1])
    expect(reload).toHaveBeenCalledTimes(1)
  })
  it.each([
    { reason: 'protected_branch', state: 'blocked', label: 'Защита ветки', explanation: 'Нужен обычный PR', transmitted: false },
    { reason: 'provider_protected_branch', state: 'blocked', label: 'Защита ветки', explanation: 'Нужен обычный PR', transmitted: false },
    { reason: 'access_denied', state: 'failed', label: 'Ошибка', explanation: 'Проверьте права токена на запись', transmitted: false },
    { reason: 'ci_non_commit_tag', state: 'applied', label: 'Передано', explanation: 'Тег без коммита: пайплайн не запускается', transmitted: true },
  ])('explains $reason without confusing Git transfer with a CI refusal', async ({ reason, state, label, explanation, transmitted }) => {
    const run: SyncRun = {
      id: 'reason-run', repository_id: 'repo', connection_version: 1,
      settings: connection.settings, source: 'manual',
      state: state === 'applied' ? 'success' : state === 'blocked' ? 'conflict' : 'failed',
      message: null, cancel_requested: false, created_at: '2026-10-18T10:00:00Z',
      started_at: '2026-10-18T10:00:01Z', finished_at: '2026-10-18T10:00:02Z',
    }
    vi.spyOn(repositorySync, 'runs').mockResolvedValue({ items: [run], total: 1 })
    vi.spyOn(repositorySync, 'refs').mockResolvedValue({ items: [{
      ref_name: reason === 'ci_non_commit_tag' ? 'refs/tags/qa-tree' : 'refs/heads/qa-rejected',
      direction: reason === 'protected_branch' || reason === 'ci_non_commit_tag' ? 'pull' : 'push',
      old_sha: null, new_sha: 'b'.repeat(40), state, reason, event_delivered: transmitted,
    }], total: 1 })
    mount(<RepositorySyncEditor connection={{ ...connection, last_run: run }} reload={vi.fn()} />,
      '/?tab=history&run_id=reason-run')
    const row = await screen.findByRole('row', { name: /refs\/(heads\/qa-rejected|tags\/qa-tree)/ })
    expect(within(row).getByText(label)).toBeVisible()
    expect(within(row).getByText(explanation)).toBeVisible()
    expect(screen.queryByText(reason)).not.toBeInTheDocument()
    if (transmitted) expect(within(row).queryByText('Ошибка')).not.toBeInTheDocument()
    else expect(within(row).queryByText('Передано')).not.toBeInTheDocument()
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
