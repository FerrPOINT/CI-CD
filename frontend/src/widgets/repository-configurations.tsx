import { useState, type FormEvent } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Button, Input, Label, NamespaceLink as Link } from '@sdlc/ui/ui'
import { useSessionCommand } from '@sdlc/ui/lib'
import { api, ApiError } from '@/api/client'
import { useConfigurations, type Configuration } from '@/api/workspaces'
import type { components } from '@/api/schema'
import { useAuth } from '@/shared/auth/auth-provider'
type Command = components['schemas']['CreateConfig']
export function RepositoryConfigurations({
  repositoryId,
  writable,
}: {
  repositoryId: string
  writable: boolean
}) {
  const { session } = useAuth()
  const [offset, setOffset] = useState(0)
  const configs = useConfigurations(repositoryId, offset)
  const [unboundOffset, setUnboundOffset] = useState(0)
  const unbound = useConfigurations(undefined, unboundOffset)
  const [search, setSearch] = useState('')
  const choices = useQuery({
    queryKey: ['push-config-choices', repositoryId, search],
    queryFn: ({ signal }) =>
      api<components['schemas']['ConfigPage']>(
        `/catalog/repositories/${repositoryId}/delivery-configs?limit=100&search=${encodeURIComponent(search)}`,
        { signal },
      ),
  })
  const push = useQuery({
    queryKey: ['push-config', repositoryId],
    queryFn: ({ signal }) =>
      api<components['schemas']['PushConfig']>(
        `/catalog/repositories/${repositoryId}/push-config`,
        { signal },
      ),
  })
  const journal = useSessionCommand<Command>(`forge:config:${session?.subject}:${repositoryId}`)
  const cache = useQueryClient()
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string>()
  async function refresh() {
    await Promise.all([
      cache.invalidateQueries({ queryKey: ['delivery-configurations'] }),
      cache.invalidateQueries({ queryKey: ['push-config-choices', repositoryId] }),
      cache.invalidateQueries({ queryKey: ['push-config', repositoryId] }),
    ])
  }
  async function execute(action: () => Promise<unknown>) {
    setBusy(true)
    setError(undefined)
    try {
      await action()
      await refresh()
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : 'Не удалось сохранить конфигурацию')
    } finally {
      setBusy(false)
    }
  }
  async function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const data = new FormData(event.currentTarget)
    const command = journal.capture({
      operation_id: crypto.randomUUID(),
      name: String(data.get('name')),
      default_branch: String(data.get('branch')),
    })
    await execute(async () => {
      try {
        await api<Configuration>(
          `/catalog/repositories/${repositoryId}/delivery-config-operations/${command.operation_id}`,
        )
      } catch (failure) {
        if (!(failure instanceof ApiError) || failure.status !== 404) throw failure
        await api<Configuration>(`/catalog/repositories/${repositoryId}/delivery-configs`, {
          method: 'POST',
          body: JSON.stringify(command),
        })
      }
      journal.clear()
    })
  }
  return (
    <section className="space-y-4">
      <h2 className="font-semibold">CI-конфигурации</h2>
      {configs.isPending ? (
        <p role="status">Загружаем конфигурации…</p>
      ) : configs.isError ? (
        <p role="alert">Конфигурации недоступны.</p>
      ) : (
        <ul className="divide-y divide-border rounded-lg border border-border bg-surface">
          {configs.data.items.map((config) => (
            <li key={config.id} className="flex flex-wrap items-center justify-between gap-3 p-4">
              <div>
                <p className="font-semibold">{config.name}</p>
                <p className="text-sm text-text-muted">Ветка: {config.default_branch}</p>
              </div>
              <div className="flex flex-wrap gap-3 text-sm">
                {[
                  ['settings', 'Настройки'],
                  ['pipelines', 'Запуски'],
                  ['secrets', 'Secrets'],
                  ['environments', 'Окружения'],
                  ['schedules', 'Расписания'],
                  ['webhooks', 'Hooks'],
                  ['reports', 'Отчёты'],
                ].map(([path, label]) => (
                  <Link
                    key={path}
                    className="text-accent"
                    to={`/delivery-configs/${config.id}/${path}`}
                  >
                    {label}
                  </Link>
                ))}
              </div>
            </li>
          ))}
          {!configs.data.total && <li className="p-4 text-text-muted">Конфигураций пока нет.</li>}
        </ul>
      )}
      <div className="flex gap-2">
        <Button
          variant="outline"
          disabled={!offset}
          onClick={() => setOffset(Math.max(0, offset - 50))}
        >
          Назад
        </Button>
        <Button
          variant="outline"
          disabled={!configs.data || offset + 50 >= configs.data.total}
          onClick={() => setOffset(offset + 50)}
        >
          Далее
        </Button>
      </div>
      {error && (
        <p role="alert" className="text-danger">
          {error}
        </p>
      )}
      <form
        onSubmit={create}
        className="max-w-xl space-y-3 rounded-lg border border-border bg-surface p-4"
      >
        <h3 className="font-semibold">Создать CI-конфигурацию</h3>
        <Label htmlFor="config-name">Название</Label>
        <Input
          id="config-name"
          name="name"
          required
          maxLength={255}
          disabled={!writable || busy || !!journal.command}
          defaultValue={journal.command?.name}
        />
        <Label htmlFor="config-branch">Ветка по умолчанию</Label>
        <Input
          id="config-branch"
          name="branch"
          required
          defaultValue={journal.command?.default_branch ?? 'main'}
          disabled={!writable || busy || !!journal.command}
        />
        {journal.command && (
          <p className="text-sm text-warning">Сначала проверим результат предыдущей операции.</p>
        )}
        <Button disabled={busy || (!writable && !journal.command)}>
          {journal.command ? 'Проверить и продолжить' : 'Создать'}
        </Button>
      </form>
      {writable && (
        <>
          <form
            className="max-w-xl space-y-3 rounded-lg border border-border bg-surface p-4"
            onSubmit={(event) => {
              event.preventDefault()
              const data = new FormData(event.currentTarget)
              void execute(() =>
                api(`/catalog/repositories/${repositoryId}/push-config`, {
                  method: 'PUT',
                  body: JSON.stringify({ configuration_id: data.get('configuration') || null }),
                }),
              )
            }}
          >
            <h3 className="font-semibold">Автозапуск при Git push</h3>
            <p className="text-sm text-text-muted">
              Push запускает одну выбранную конфигурацию. Остальные доступны вручную и по
              расписанию.
            </p>
            <Input
              aria-label="Поиск конфигурации для push"
              placeholder="Поиск конфигураций…"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            />
            {push.isError || choices.isError ? (
              <p role="alert">Настройки автозапуска недоступны.</p>
            ) : !push.data || !choices.data ? (
              <p role="status">Загружаем настройки…</p>
            ) : (
              <label className="block text-sm">
                Конфигурация
                <select
                  key={push.data.configuration_id ?? 'off'}
                  name="configuration"
                  defaultValue={push.data.configuration_id ?? ''}
                  className="block min-h-10 w-full rounded-md border border-border bg-surface px-3"
                >
                  <option value="">Отключён</option>
                  {push.data.configuration_id &&
                    !choices.data.items.some((item) => item.id === push.data.configuration_id) && (
                      <option value={push.data.configuration_id}>
                        Текущая конфигурация (используйте поиск)
                      </option>
                    )}
                  {choices.data.items.map((config) => (
                    <option key={config.id} value={config.id}>
                      {config.name}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <Button disabled={busy || !push.data || !choices.data}>Сохранить автозапуск</Button>
          </form>
          <form
            className="max-w-xl space-y-3 rounded-lg border border-border bg-surface p-4"
            onSubmit={(event) => {
              event.preventDefault()
              const data = new FormData(event.currentTarget)
              void execute(() =>
                api(
                  `/catalog/repositories/${repositoryId}/delivery-configs/${data.get('configuration')}`,
                  { method: 'PUT' },
                ),
              )
            }}
          >
            <h3 className="font-semibold">Подключить существующую конфигурацию</h3>
            <p className="text-sm text-text-muted">Прежняя история и настройки сохраняются.</p>
            {unbound.isError ? (
              <p role="alert">Конфигурации недоступны.</p>
            ) : (
              <label className="block text-sm">
                Конфигурация
                <select
                  name="configuration"
                  required
                  className="block min-h-10 w-full rounded-md border border-border bg-surface px-3"
                >
                  <option value="">Выберите конфигурацию</option>
                  {unbound.data?.items.map((config) => (
                    <option key={config.id} value={config.id}>
                      {config.name}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <div className="flex gap-2">
              <Button
                type="button"
                variant="outline"
                disabled={!unboundOffset}
                onClick={() => setUnboundOffset(Math.max(0, unboundOffset - 50))}
              >
                Назад
              </Button>
              <Button
                type="button"
                variant="outline"
                disabled={!unbound.data || unboundOffset + 50 >= unbound.data.total}
                onClick={() => setUnboundOffset(unboundOffset + 50)}
              >
                Далее
              </Button>
              <Button disabled={busy || !unbound.data?.items.length}>Подключить</Button>
            </div>
          </form>
        </>
      )}
    </section>
  )
}
