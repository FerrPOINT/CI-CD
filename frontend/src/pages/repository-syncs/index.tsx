import { useEffect, useRef, useState, type FormEvent } from 'react'
import { useParams, useSearchParams } from 'react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  ArrowLeft,
  ArrowLeftRight,
  CirclePause,
  Plus,
  RefreshCw,
} from 'lucide-react'
import {
  Select,
  Button,
  Input,
  Label,
  NamespaceLink as Link,
  OffsetPageNavigation,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger, Radio } from '@pdlc/ui/ui'
import { withNamespaceLocation } from '@pdlc/ui/lib'
import { api, ApiError } from '@/api/client'
import {
  repositorySync,
  type SaveSync,
  type SyncConnection,
  type SyncRun,
  type SyncSettings,
} from '@/api/repository-sync'
import type { components } from '@/api/schema'
import { useNamespaceRef } from '@/widgets/namespace-context'
import { useWorkspace } from '@/api/workspaces'

const modes = [
  {
    value: 'disabled',
    title: 'Отключено',
    detail: 'Подключение сохранено, обмен остановлен.',
    icon: CirclePause,
  },
  {
    value: 'pull',
    title: 'Внешний → Forge',
    detail: 'Получать ветки и теги.',
    icon: ArrowDownToLine,
  },
  {
    value: 'push',
    title: 'Forge → внешний',
    detail: 'Отправлять ветки и теги.',
    icon: ArrowUpFromLine,
  },
  {
    value: 'both',
    title: 'В обе стороны',
    detail: 'Получать и отправлять изменения.',
    icon: ArrowLeftRight,
  },
] as const
const states: Record<string, string> = {
  queued: 'В очереди',
  running: 'Выполняется',
  stopping: 'Останавливается',
  reconciling: 'Уточняется результат',
  success: 'Успешно',
  partial: 'Частично выполнено',
  conflict: 'Конфликт',
  failed: 'Ошибка',
  canceled: 'Остановлено',
  applied: 'Передано',
  unchanged: 'Без изменений',
  blocked: 'Защита ветки',
  unknown: 'Неизвестный результат',
  planned: 'Ожидает передачи',
}
const runState = (run: SyncRun) =>
  run.cancel_requested && !run.finished_at
    ? `Останавливается${run.state === 'reconciling' ? ' · сверка refs' : ''}`
    : (states[run.state] ?? 'Неизвестное состояние')
const sources: Record<string, string> = {
  manual: 'Вручную',
  schedule: 'По расписанию',
  git_change: 'Изменения в Forge',
}
const remote = (settings: SyncSettings) =>
  `${settings.instance_url}/${settings.repository_path}.git`
const time = (value: string) => new Date(value).toLocaleString('ru-RU')
const diagnostics: Record<string, string> = {
  git_access_denied: 'Ошибка доступа: проверьте токен и права на репозиторий.',
  git_tls_failed: 'Не удалось проверить TLS-сертификат провайдера.',
  git_network_failed: 'Провайдер недоступен.',
  git_outcome_unknown: 'Результат Git-операции уточняется.',
  namespace_admission_lost:
    'Не удалось подтвердить доступ к репозиторию. Результат синхронизации уточняется.',
  sync_version_conflict: 'Сохранённая версия настроек изменилась.',
  sync_running:
    'Настройки нельзя менять во время запуска; доступно отключение.',
}
const message = (error: unknown) =>
  error instanceof ApiError && error.kind === 'network'
    ? 'Не удалось связаться с сервером.'
    : error instanceof Error
    ? (diagnostics[error.message] ?? error.message)
    : 'Не удалось выполнить действие'
const offsetOf = (value: string | null) => {
  const offset = Number(value)
  return Number.isSafeInteger(offset) && offset >= 0 ? offset : 0
}

export function RepositorySyncsPage({ groupId }: { groupId?: string }) {
  const [params, setParams] = useSearchParams()
  const { ref, malformed, all } = useNamespaceRef()
  const workspace = useWorkspace(malformed || (!groupId && all) ? null : ref)
  const group = groupId ?? (all ? undefined : workspace.data?.group_id)
  const [adding, setAdding] = useState(false)
  const [selection, setSelection] = useState('')
  const query = new URLSearchParams()
  for (const key of ['search', 'provider', 'state'])
    if (params.get(key)) query.set(key, params.get(key)!)
  if (params.has('offset')) query.set('offset', String(offsetOf(params.get('offset'))))
  query.set('limit', '20')
  if (group) query.set('group_id', group)
  const enabled = !malformed && (all || !ref || !!group)
  const list = useQuery({
    queryKey: ['repository-syncs', query.toString()],
    enabled,
    queryFn: ({ signal }) => repositorySync.list(query, signal),
    refetchInterval: 5000,
  })
  const availableQuery = new URLSearchParams({
    sync_available: 'true',
    limit: '20',
    offset: String(offsetOf(params.get('select_offset'))),
    search: params.get('select_search') ?? '',
  })
  if (group) availableQuery.set('group_id', group)
  const available = useQuery({
    queryKey: ['sync-available', availableQuery.toString()],
    enabled: enabled && adding,
    queryFn: ({ signal }) =>
      api<components['schemas']['CatalogPage']>(
        `/catalog/repositories?${availableQuery}`,
        { signal },
      ),
  })
  function update(key: string, value: string) {
    const next = new URLSearchParams(params)
    if (value) next.set(key, value)
    else next.delete(key)
    if (['search', 'provider', 'state'].includes(key)) next.delete('offset')
    if (key === 'select_search') next.delete('select_offset')
    setParams(next, { replace: key.endsWith('search') })
  }
  const location = (
    id: string,
    history = false,
    namespace?: SyncConnection['namespace'],
  ) => {
    const next = new URLSearchParams(params)
    next.set('tab', history ? 'history' : 'connection')
    if (all && !groupId) next.set('project_scope', 'all')
    else next.delete('project_scope')
    return withNamespaceLocation(
      `/catalog/repositories/${id}/sync?${next}`,
      namespace ?? null,
    )
  }
  if (malformed) return <p role="alert">Некорректная ссылка на проект.</p>
  if (!groupId && !all && ref && workspace.isError)
    return <p role="alert">Проект недоступен.</p>
  if (!groupId && !all && ref && workspace.data && !group)
    return <p>Git-группа проекта не подключена.</p>
  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold">Синхронизации</h1>
          <p className="mt-1 text-sm text-text-secondary">
            Подключения репозиториев Forge к GitLab и GitHub.com.
          </p>
        </div>
        <Button onClick={() => setAdding(!adding)} aria-expanded={adding}>
          <Plus aria-hidden size={16} />
          Добавить синхронизацию
        </Button>
      </div>
      {adding && (
        <section
          className="space-y-3 border-y border-border py-4"
          aria-label="Выбор репозитория"
        >
          <Label htmlFor="select-search">Поиск репозитория Forge</Label>
          <Input
            id="select-search"
            value={params.get('select_search') ?? ''}
            onChange={(event) => update('select_search', event.target.value)}
          />
          {available.isError && (
            <p role="alert">
              Не удалось загрузить репозитории.{' '}
              <Button
                variant="outline"
                onClick={() => void available.refetch()}
              >
                Повторить
              </Button>
            </p>
          )}
          {available.isPending && <p role="status">Загружаем репозитории…</p>}
          {available.data && (
            <>
              <Label htmlFor="new-repository">
                Доступный hosted-репозиторий без подключения
              </Label>
              <Select id="new-repository"
className="w-full"
value={selection}
onValueChange={(nextValue) => setSelection(nextValue)} options={[{ value: "", label: "Выберите репозиторий" }, ...(available.data.items.map((repo) => ({ value: String(repo.id), label: String(repo.public_name) })) ?? [])]} />
              {selection &&
                available.data.items.some((repo) => repo.id === selection) && (
                  <Button asChild>
                    <Link
                      to={location(
                        selection,
                        false,
                        available.data.items.find(
                          (repo) => repo.id === selection,
                        )?.namespace,
                      )}
                    >
                      Настроить подключение
                    </Link>
                  </Button>
                )}
              <OffsetPageNavigation label="Доступные репозитории"
                offset={offsetOf(params.get('select_offset'))}
                total={available.data.total}
                limit={20}
                onOffsetChange={(next) => update('select_offset', String(next))}
              />
            </>
          )}
        </section>
      )}
      <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto_auto]">
        <div>
          <Label htmlFor="sync-search">Поиск</Label>
          <Input
            id="sync-search"
            type="search"
            value={params.get('search') ?? ''}
            onChange={(event) => update('search', event.target.value)}
            placeholder="Репозиторий или внешний адрес"
          />
        </div>
        <div>
          <Label htmlFor="provider-filter">Провайдер</Label>
          <Select id="provider-filter"
className="w-full"
value={params.get('provider') ?? ''}
onValueChange={(nextValue) => update('provider', nextValue)} options={[{ value: "", label: "Все провайдеры" }, { value: "github", label: "GitHub.com" }, { value: "gitlab", label: "Свой GitLab" }]} />
        </div>
        <div>
          <Label htmlFor="state-filter">Состояние</Label>
          <Select id="state-filter"
className="w-full"
value={params.get('state') ?? ''}
onValueChange={(nextValue) => update('state', nextValue)} options={[{ value: "", label: "Все подключения" }, { value: "enabled", label: "Включённые" }, { value: "disabled", label: "Отключённые" }, ...([
        'queued',
        'running',
        'stopping',
        'reconciling',
        'success',
        'partial',
        'conflict',
        'failed',
        'canceled'
    ].map((value) => ({ value: String(value), label: String(states[value]) })) ?? [])]} />
        </div>
      </div>
      {list.isError && (
        <p role="alert" className="text-danger">
          Список не удалось обновить.{' '}
          {list.data && 'Показаны последние загруженные данные.'}{' '}
          <Button variant="outline" onClick={() => void list.refetch()}>
            Повторить
          </Button>
        </p>
      )}
      {list.isPending && <p role="status">Загружаем синхронизации…</p>}
      {list.data && (
        <>
          <Table>
            <caption className="sr-only">Список синхронизаций</caption>
            <TableHeader>
              <TableRow>
                {[
                  'Репозиторий Forge',
                  'Внешний репозиторий',
                  'Направление',
                  'Запуск',
                  'Состояние',
                  'Последний результат',
                  'История',
                ].map((title) => (
                  <TableHead key={title}>{title}</TableHead>
                ))}
              </TableRow>
            </TableHeader>
            <TableBody>
              {list.data.items.map((connection) => (
                <TableRow key={connection.repository_id}>
                  <TableCell>
                    <Link
                      className="font-medium text-accent"
                      to={location(
                        connection.repository_id,
                        false,
                        connection.namespace,
                      )}
                    >
                      {connection.repository_name}
                    </Link>
                  </TableCell>
                  <TableCell>
                    <span className="block">
                      {connection.settings.provider === 'github'
                        ? 'GitHub.com'
                        : 'Свой GitLab'}
                    </span>
                    <span className="block max-w-72 break-all text-xs text-text-secondary">
                      {remote(connection.settings)}
                    </span>
                  </TableCell>
                  <TableCell>
                    {
                      modes.find(
                        (mode) => mode.value === connection.settings.direction,
                      )?.title
                    }
                  </TableCell>
                  <TableCell>
                    {connection.settings.automatic
                      ? 'Автоматически'
                      : 'Вручную'}
                  </TableCell>
                  <TableCell>
                    {connection.paused_reason
                      ? 'Приостановлена'
                      : connection.active_run
                        ? runState(connection.active_run)
                        : connection.settings.direction === 'disabled'
                          ? 'Отключена'
                          : 'Включена'}
                  </TableCell>
                  <TableCell>
                    {connection.last_run ? (
                      <>
                        {runState(connection.last_run)}
                        <span className="block text-xs text-text-muted">
                          {time(connection.last_run.created_at)}
                        </span>
                      </>
                    ) : (
                      'Запусков ещё нет'
                    )}
                  </TableCell>
                  <TableCell>
                    <Link
                      className="text-accent"
                      to={location(
                        connection.repository_id,
                        true,
                        connection.namespace,
                      )}
                    >
                      История
                    </Link>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
          {!list.data.total && (
            <p>
              Синхронизации не найдены. Измените фильтры или добавьте
              подключение.
            </p>
          )}
          <OffsetPageNavigation label="Синхронизации репозиториев"
            offset={offsetOf(params.get('offset'))}
            total={list.data.total}
            limit={20}
            onOffsetChange={(next) => update('offset', String(next))}
          />
        </>
      )}
    </div>
  )
}

export function RepositorySyncPage() {
  const { id = '' } = useParams()
  const { ref, malformed } = useNamespaceRef()
  const connection = useQuery({
    queryKey: ['repository-sync', id],
    enabled: !malformed,
    queryFn: ({ signal }) => repositorySync.get(id, signal),
    refetchInterval: 3000,
  })
  if (malformed) return <p role="alert">Некорректная ссылка на проект.</p>
  if (
    connection.data &&
    ref &&
    (connection.data.namespace?.namespace_id !== ref.namespace_id ||
      connection.data.namespace?.registry_instance_id !==
        ref.registry_instance_id)
  )
    return <p role="alert">Репозиторий принадлежит другому проекту.</p>
  if (!connection.data)
    return (
      <p role={connection.isError ? 'alert' : 'status'}>
        {connection.isError
          ? 'Подключение недоступно.'
          : 'Загружаем подключение…'}
      </p>
    )
  return (
    <>
      <RepositorySyncEditor
        key={id}
        connection={connection.data}
        reload={() => connection.refetch()}
      />
      {connection.isError && (
        <p role="alert">
          Не удалось обновить состояние. Показаны последние загруженные данные.
        </p>
      )}
    </>
  )
}

export function RepositorySyncEditor({
  connection,
  reload,
  backLocation,
}: {
  connection: SyncConnection
  reload: () => Promise<unknown>
  backLocation?: string
}) {
  const [params, setParams] = useSearchParams()
  const cache = useQueryClient()
  const [draft, setDraft] = useState(connection.settings)
  const [version, setVersion] = useState(connection.version)
  const [token, setToken] = useState('')
  const [busy, setBusy] = useState(false)
  const [feedback, setFeedback] = useState('')
  const [conflict, setConflict] = useState(false)
  const [uncertain, setUncertain] = useState(false)
  const [runUncertain, setRunUncertain] = useState(false)
  const pending = useRef<{ key: string; payload: SaveSync } | null>(null)
  const runKey = useRef<string | null>(null)
  const activeTab = params.get('tab') === 'history' ? 'history' : 'connection'
  const requestedTab = useRef(activeTab)
  useEffect(() => {
    requestedTab.current = activeTab
  }, [activeTab])
  const writable = connection.writable && !busy && !uncertain
  const endpointChanged =
    draft.provider !== connection.settings.provider ||
    draft.instance_url !== connection.settings.instance_url
  const dirty =
    version === 0 ||
    token !== '' ||
    JSON.stringify(draft) !== JSON.stringify(connection.settings)
  function patch(next: Partial<SyncSettings>) {
    setDraft({ ...draft, ...next })
    pending.current = null
    setFeedback('')
  }
  async function save(event?: FormEvent) {
    event?.preventDefault()
    setBusy(true)
    setFeedback('')
    setConflict(false)
    pending.current ??= {
      key: crypto.randomUUID(),
      payload: {
        expected_version: version,
        settings: draft,
        ...(token ? { token } : {}),
      },
    }
    try {
      const saved = await repositorySync.save(
        connection.repository_id,
        pending.current.payload,
        pending.current.key,
      )
      setVersion(saved.version)
      setDraft(saved.settings)
      setToken('')
      pending.current = null
      setUncertain(false)
      cache.setQueryData(['repository-sync', connection.repository_id], saved)
      void cache.invalidateQueries({ queryKey: ['repository-syncs'] })
      setFeedback('Подключение сохранено')
    } catch (error) {
      setFeedback(message(error))
      const unknown =
        !(error instanceof ApiError) ||
        error.kind === 'network' ||
        error.status >= 500
      setUncertain(unknown)
      if (!unknown) pending.current = null
      setConflict(error instanceof ApiError && error.status === 409)
    } finally {
      setBusy(false)
    }
  }
  async function check() {
    setBusy(true)
    setFeedback('')
    try {
      const result = await repositorySync.check(connection.repository_id, {
        settings: draft,
        ...(token ? { token } : {}),
      })
      setFeedback(result.message)
    } catch (error) {
      setFeedback(message(error))
    } finally {
      setBusy(false)
    }
  }
  async function start() {
    setBusy(true)
    setFeedback('')
    runKey.current ??= crypto.randomUUID()
    try {
      await repositorySync.start(connection.repository_id, runKey.current)
      runKey.current = null
      setRunUncertain(false)
      await reload()
      setFeedback('Запуск поставлен в очередь')
    } catch (error) {
      setFeedback(message(error))
      if (
        error instanceof ApiError &&
        error.status >= 400 &&
        error.status < 500
      ) {
        runKey.current = null
        setRunUncertain(false)
      } else setRunUncertain(true)
    } finally {
      setBusy(false)
    }
  }
  function tab(value: string) {
    // Pointer focus and mousedown can both activate Radix before the URL commits.
    if (requestedTab.current === value) return
    requestedTab.current = value
    const next = new URLSearchParams(params)
    next.set('tab', value)
    setParams(next)
  }
  const back = new URLSearchParams(params)
  for (const key of ['tab', 'run_id', 'runs_offset', 'refs_offset'])
    back.delete(key)
  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-3">
          <Link
            to={
              backLocation ??
              withNamespaceLocation(
                `/repository-syncs?${back}`,
                connection.namespace ?? null,
              )
            }
            aria-label="Все синхронизации"
            title="Все синхронизации"
            className="p-2 text-accent"
          >
            <ArrowLeft aria-hidden size={20} />
          </Link>
          <div>
            <h1 className="text-xl font-semibold">Внешний репозиторий</h1>
            <p className="text-sm text-text-secondary">
              {connection.repository_name}
            </p>
          </div>
        </div>
        <Button
          disabled={
            !connection.writable ||
            !connection.worker_enabled ||
            busy ||
            dirty ||
            connection.settings.direction === 'disabled' ||
            !!connection.active_run
          }
          onClick={() => void start()}
        >
          <RefreshCw aria-hidden size={16} />
          Синхронизировать сейчас
        </Button>
      </div>
      <div className="flex flex-wrap justify-between gap-2 border-y border-border py-3 text-sm">
        <div>
          <strong>
            {
              modes.find((mode) => mode.value === connection.settings.direction)
                ?.title
            }
          </strong>
          {connection.version > 0 && (
            <span className="mt-1 block break-all text-text-secondary">
              {remote(connection.settings)}
            </span>
          )}
        </div>
        <span>
          {connection.active_run
            ? runState(connection.active_run)
            : connection.last_run
              ? `${runState(connection.last_run)} · ${time(connection.last_run.created_at)}`
              : 'Запусков ещё нет'}
        </span>
      </div>
      {connection.paused_reason && (
        <p role="status">
          Синхронизация приостановлена:{' '}
          {connection.paused_reason === 'namespace_read_only'
            ? 'проект в архиве'
            : 'хранилище недоступно'}
          .
        </p>
      )}
      {!connection.worker_enabled && (
        <p role="status" className="text-sm text-text-secondary">
          Исполнитель отключён оператором. Настройки и история сохранены.
        </p>
      )}
      {!connection.writable && <p role="status">Доступно только чтение.</p>}
      <Tabs
        value={activeTab}
        onValueChange={tab}
      >
        <TabsList>
          <TabsTrigger value="connection">Подключение</TabsTrigger>
          <TabsTrigger value="history">История запусков</TabsTrigger>
        </TabsList>
        <TabsContent value="connection">
          <form onSubmit={(event) => void save(event)} className="space-y-4">
            <fieldset
              disabled={!writable || !!connection.active_run}
              className="grid gap-4 sm:grid-cols-2"
            >
              <div className="space-y-1">
                <Label htmlFor="provider">Провайдер</Label>
                <Select id="provider"
className="w-full"
value={draft.provider}
onValueChange={(nextValue) => patch({
                      provider: nextValue as SyncSettings['provider'],
                      instance_url:
                        nextValue === 'github'
                          ? 'https://github.com'
                          : '',
                    })} options={[{ value: "github", label: "GitHub.com" }, { value: "gitlab", label: "Свой GitLab" }]} />
              </div>
              <div className="space-y-1">
                <Label htmlFor="instance">
                  {draft.provider === 'gitlab'
                    ? 'Адрес инстанса GitLab'
                    : 'Адрес GitHub'}
                </Label>
                <Input
                  id="instance"
                  type="url"
                  required
                  value={draft.instance_url}
                  readOnly={draft.provider === 'github'}
                  onChange={(event) =>
                    patch({ instance_url: event.target.value })
                  }
                  placeholder="https://gitlab.example.org"
                />
              </div>
              <div className="space-y-1">
                <Label htmlFor="repository-path">
                  {draft.provider === 'github'
                    ? 'Владелец / репозиторий'
                    : 'Группа / репозиторий'}
                </Label>
                <Input
                  id="repository-path"
                  required
                  value={draft.repository_path}
                  onChange={(event) =>
                    patch({ repository_path: event.target.value })
                  }
                  placeholder="team/service-api"
                />
                <p className="text-xs text-text-muted">
                  Все ветки и теги; удаления не распространяются.
                </p>
              </div>
              <div className="space-y-1">
                <Label htmlFor="sync-token">Токен доступа</Label>
                <Input
                  id="sync-token"
                  type="password"
                  autoComplete="new-password"
                  required={!connection.has_credential || endpointChanged}
                  value={token}
                  onChange={(event) => {
                    setToken(event.target.value)
                    pending.current = null
                  }}
                  placeholder={
                    connection.has_credential && !endpointChanged
                      ? 'Токен сохранён · введите новый для замены'
                      : 'Новый токен'
                  }
                />
                <p className="text-xs text-text-muted">
                  {draft.provider === 'github'
                    ? 'Доступ к выбранному репозиторию: Contents read/write по направлению.'
                    : 'Git scopes: read_repository / write_repository по направлению.'}
                </p>
              </div>
            </fieldset>
            <fieldset disabled={!writable} className="space-y-2">
              <legend className="mb-2 font-medium">
                Направление синхронизации
              </legend>
              <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
                {modes.map((mode) => (
                  <label
                    key={mode.value}
                    className={`flex cursor-pointer gap-2 rounded-md border px-3 py-2 ${draft.direction === mode.value ? 'border-accent bg-surface' : 'border-border'}`}
                  >
                    <Radio
                      className="mt-1 shrink-0"
                      name="direction"
                      disabled={
                        !!connection.active_run && mode.value !== 'disabled'
                      }
                      value={mode.value}
                      checked={draft.direction === mode.value}
                      onCheckedChange={() => patch({ direction: mode.value })}
                    />
                    <span className="min-w-0">
                      <strong className="flex items-center gap-2 text-sm">
                        <mode.icon aria-hidden size={14} />
                        {mode.title}
                      </strong>
                      <span className="text-xs text-text-secondary">
                        {mode.detail}
                      </span>
                    </span>
                  </label>
                ))}
              </div>
            </fieldset>
            <fieldset disabled={!writable || !!connection.active_run}>
              <legend className="mb-2 font-medium">Способ запуска</legend>
              <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                {[false, true].map((automatic) => (
                  <label
                    key={String(automatic)}
                    className={`flex cursor-pointer gap-3 rounded-md border p-3 ${draft.automatic === automatic ? 'border-accent' : 'border-border'}`}
                  >
                    <Radio
                      name="automatic"
                      checked={draft.automatic === automatic}
                      onCheckedChange={() => patch({ automatic })}
                    />
                    <span>
                      <strong className="block text-sm">
                        {automatic ? 'Автоматически' : 'Вручную'}
                      </strong>
                      <span className="text-xs text-text-secondary">
                        {automatic
                          ? 'После изменений в Forge и периодическая сверка.'
                          : 'По кнопке «Синхронизировать сейчас».'}
                      </span>
                    </span>
                  </label>
                ))}
              </div>
            </fieldset>
            <p className="text-xs text-text-secondary">
              Защищённые ветки принимают изменения через обычный PR.
              Расходящаяся история и отличающиеся теги требуют ручного
              разрешения.
            </p>
            <div className="flex flex-wrap gap-3">
              <Button type="submit" disabled={!writable || !dirty}>
                Сохранить подключение
              </Button>
              <Button
                type="button"
                variant="outline"
                disabled={
                  !writable ||
                  (!token && (!connection.has_credential || endpointChanged))
                }
                onClick={() => void check()}
              >
                Проверить доступ
              </Button>
              {connection.active_run &&
                connection.settings.direction !== 'disabled' && (
                  <Button
                    type="button"
                    variant="outline"
                    disabled={!writable}
                    onClick={() => {
                      setToken('')
                      patch({ ...connection.settings, direction: 'disabled' })
                    }}
                  >
                    Отключить синхронизацию
                  </Button>
                )}
            </div>
          </form>
        </TabsContent>
        <TabsContent value="history">
          <SyncHistory
            repository={connection.repository_id}
            active={!!connection.active_run}
          />
        </TabsContent>
      </Tabs>
      {feedback && (
        <p role="status" className="text-sm">
          {feedback}
        </p>
      )}
      {runUncertain && (
        <div role="alert">
          <p>Ответ запуска неизвестен. Получите результат той же операции.</p>
          <Button disabled={busy} onClick={() => void start()}>
            Проверить результат запуска
          </Button>
        </div>
      )}
      {uncertain && (
        <div role="alert" className="space-y-2">
          <p>
            Ответ сохранения неизвестен. Повторите ту же операцию для получения
            сохранённого результата.
          </p>
          <Button disabled={busy} onClick={() => void save()}>
            Проверить результат сохранения
          </Button>
        </div>
      )}
      {conflict && (
        <div role="alert" className="space-y-2">
          <p>
            Черновик сохранён на экране. Обновите сохранённую версию, проверьте
            изменения и повторите сохранение.
          </p>
          <Button
            variant="outline"
            onClick={async () => {
              try {
                const saved = await repositorySync.get(connection.repository_id)
                cache.setQueryData(
                  ['repository-sync', connection.repository_id],
                  saved,
                )
                setVersion(saved.version)
                pending.current = null
                setConflict(false)
              } catch (error) {
                setFeedback(message(error))
              }
            }}
          >
            Обновить сохранённую версию
          </Button>
        </div>
      )}
    </div>
  )
}

function SyncHistory({
  repository,
  active,
}: {
  repository: string
  active: boolean
}) {
  const cache = useQueryClient()
  const wasActive = useRef(active)
  const [params, setParams] = useSearchParams()
  const offset = offsetOf(params.get('runs_offset'))
  const runId = params.get('run_id') ?? ''
  const refsOffset = offsetOf(params.get('refs_offset'))
  const runs = useQuery({
    queryKey: ['sync-runs', repository, offset],
    queryFn: ({ signal }) => repositorySync.runs(repository, offset, signal),
    refetchInterval: active ? 3000 : false,
  })
  const refs = useQuery({
    queryKey: ['sync-refs', repository, runId, refsOffset],
    enabled: !!runId,
    queryFn: ({ signal }) =>
      repositorySync.refs(repository, runId, refsOffset, signal),
    refetchInterval: active ? 3000 : false,
  })
  useEffect(() => {
    if (wasActive.current && !active) {
      void cache.invalidateQueries({ queryKey: ['sync-runs', repository] })
      void cache.invalidateQueries({ queryKey: ['sync-refs', repository] })
    }
    wasActive.current = active
  }, [active, cache, repository])
  function update(key: string, value: string) {
    const next = new URLSearchParams(params)
    next.set(key, value)
    if (key === 'run_id') next.delete('refs_offset')
    setParams(next)
  }
  return (
    <div className="space-y-4">
      {runs.isError && (
        <p role="alert">
          История недоступна.{' '}
          <Button variant="outline" onClick={() => void runs.refetch()}>
            Повторить
          </Button>
        </p>
      )}
      {runs.isPending && <p role="status">Загружаем историю…</p>}
      {runs.data && (
        <>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Запуск</TableHead>
                <TableHead>Источник</TableHead>
                <TableHead>Результат</TableHead>
                <TableHead>Подключение</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {runs.data.items.map((run: SyncRun) => (
                <TableRow key={run.id}>
                  <TableCell>
                    <Button variant="ghost"
                      className="min-h-10 text-left text-accent"
                      onClick={() => update('run_id', run.id)}
                    >
                      {time(run.created_at)}
                    </Button>
                  </TableCell>
                  <TableCell>{sources[run.source] ?? run.source}</TableCell>
                  <TableCell>
                    {runState(run)}
                    {run.message && (
                      <p className="text-xs">
                        {diagnostics[run.message] ?? run.message}
                      </p>
                    )}
                  </TableCell>
                  <TableCell>
                    <span className="break-all text-xs">
                      {remote(run.settings)} · v{run.connection_version}
                    </span>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
          <OffsetPageNavigation label="История синхронизации"
            offset={offset}
            total={runs.data.total}
            limit={20}
            onOffsetChange={(value) => update('runs_offset', String(value))}
          />
        </>
      )}
      {runId && (
        <section className="space-y-3">
          <h2 className="font-medium">Результаты веток и тегов</h2>
          {refs.isError && (
            <p role="alert">
              Результаты недоступны.{' '}
              <Button variant="outline" onClick={() => void refs.refetch()}>
                Повторить
              </Button>
            </p>
          )}
          {refs.isPending && <p role="status">Загружаем результаты…</p>}
          {refs.data && (
            <>
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Ref</TableHead>
                    <TableHead>Передача</TableHead>
                    <TableHead>Сравнение SHA</TableHead>
                    <TableHead>Результат</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {refs.data.items.map((reference) => (
                    <TableRow key={reference.ref_name}>
                      <TableCell className="break-all">
                        {reference.ref_name}
                      </TableCell>
                      <TableCell>
                        {reference.direction === 'pull'
                          ? 'Внешний → Forge'
                          : reference.direction === 'push'
                            ? 'Forge → внешний'
                            : '—'}
                      </TableCell>
                      <TableCell className="font-mono text-xs">
                        {reference.old_sha?.slice(0, 12) ?? '∅'} →{' '}
                        {reference.new_sha?.slice(0, 12) ?? '∅'}
                      </TableCell>
                      <TableCell>
                        {states[reference.state] ?? 'Неизвестный результат'}
                        {reference.reason && (
                          <p className="text-xs">
                            {reference.reason === 'protected_branch' ||
                            reference.reason === 'provider_protected_branch'
                              ? 'Нужен обычный PR'
                              : reference.reason === 'access_denied'
                                ? 'Проверьте права токена на запись'
                                : reference.reason === 'ci_non_commit_tag'
                                  ? 'Тег без коммита: пайплайн не запускается'
                                  : reference.reason === 'outcome_unknown'
                                    ? 'Ожидается сверка фактического SHA'
                                    : 'Изменения требуют проверки'}
                          </p>
                        )}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
              <OffsetPageNavigation label="Результаты веток и тегов"
                offset={refsOffset}
                total={refs.data.total}
                limit={50}
                onOffsetChange={(value) => update('refs_offset', String(value))}
              />
            </>
          )}
        </section>
      )}
    </div>
  )
}
