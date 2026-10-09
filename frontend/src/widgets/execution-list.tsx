import { useQuery } from '@tanstack/react-query'
import { useSearchParams } from 'react-router'
import { useState } from 'react'
import { Button, Input, NamespaceLink as Link } from '@sdlc/ui/ui'
import { type NamespaceLocation, withNamespaceLocation } from '@sdlc/ui/lib'
import type { components } from '@/api/schema'
import { api } from '@/api/client'
import { useWorkspace, workspaceKey } from '@/api/workspaces'
import { executionBoundary } from '@/api/execution-filters'

export function ExecutionList({
  namespace,
  repositoryId,
  deployment = false,
}: {
  namespace?: NamespaceLocation
  repositoryId?: string
  deployment?: boolean
}) {
  const [params, setParams] = useSearchParams()
  const workspace = useWorkspace(namespace ?? null)
  const [repositorySearch, setRepositorySearch] = useState('')
  const [configurationSearch, setConfigurationSearch] = useState('')
  const repositories = useQuery({
    queryKey: [
      'execution-repository-choices',
      namespace?.registry_instance_id,
      namespace?.namespace_id,
      repositorySearch,
    ],
    enabled: !repositoryId && (!namespace || !!workspace.data?.group_id),
    queryFn: ({ signal }) =>
      api<components['schemas']['CatalogPage']>(
        `/catalog/repositories?limit=100${namespace ? '&group_id=' + workspace.data!.group_id : ''}&search=${encodeURIComponent(repositorySearch)}`,
        { signal },
      ),
  })
  const selectedRepository = repositoryId ?? params.get('repository_id') ?? ''
  const configPath = selectedRepository
    ? `/catalog/repositories/${selectedRepository}/delivery-configs`
    : '/delivery-configurations'
  const configurations = useQuery({
    queryKey: [
      'execution-config-choices',
      namespace?.registry_instance_id,
      namespace?.namespace_id,
      selectedRepository,
      configurationSearch,
    ],
    queryFn: ({ signal }) =>
      api<components['schemas']['ConfigPage']>(
        `${configPath}?limit=100&search=${encodeURIComponent(configurationSearch)}${namespace ? '&registry_instance_id=' + namespace.registry_instance_id + '&namespace_id=' + namespace.namespace_id : ''}`,
        { signal },
      ),
  })
  const offset = Math.max(0, Number(params.get('offset')) || 0)
  const query = new URLSearchParams({ limit: '50', offset: String(offset) })
  for (const key of ['repository_id', 'configuration_id', 'status', 'git_ref'])
    if (params.get(key)) query.set(key, params.get(key)!)
  for (const key of ['since', 'until'])
    if (params.get(key)) query.set(key, executionBoundary(params.get(key)!, key === 'until'))
  const path = repositoryId
    ? `/catalog/repositories/${repositoryId}/${deployment ? 'deployments' : 'pipelines'}`
    : namespace
      ? `/workspace-projects/${workspaceKey(namespace)}/${deployment ? 'deployments' : 'pipelines'}`
      : '/workspace-pipelines'
  const list = useQuery({
    queryKey: [
      'executions',
      namespace?.registry_instance_id,
      namespace?.namespace_id,
      repositoryId,
      deployment,
      query.toString(),
    ],
    queryFn: ({ signal }) =>
      api<components['schemas']['ExecutionPage']>(`${path}?${query}`, { signal }),
  })
  function update(key: string, value: string) {
    setParams((current) => {
      const next = new URLSearchParams(current)
      next.delete('offset')
      if (value) next.set(key, value)
      else next.delete(key)
      return next
    })
  }
  return (
    <section className="space-y-4">
      <form
        className="flex flex-wrap items-end gap-3"
        onSubmit={(event) => {
          event.preventDefault()
          const data = new FormData(event.currentTarget)
          setParams((current) => {
            const next = new URLSearchParams(current)
            next.delete('offset')
            for (const [key, value] of data) {
              if (String(value)) next.set(key, String(value))
              else next.delete(key)
            }
            return next
          })
        }}
      >
        {!repositoryId && (
          <label className="text-sm">
            Репозиторий
            <Input
              aria-label="Поиск репозитория"
              placeholder="Поиск репозитория…"
              value={repositorySearch}
              onChange={(event) => setRepositorySearch(event.target.value)}
            />
            <select
              className="block min-h-10 w-full rounded-md border border-border bg-surface px-3"
              value={params.get('repository_id') ?? ''}
              onChange={(event) =>
                setParams((current) => {
                  const next = new URLSearchParams(current)
                  next.delete('offset')
                  next.delete('configuration_id')
                  if (event.target.value) next.set('repository_id', event.target.value)
                  else next.delete('repository_id')
                  return next
                })
              }
            >
              <option value="">Все репозитории</option>
              {params.get('repository_id') &&
                !repositories.data?.items.some(
                  (item) => item.id === params.get('repository_id'),
                ) && <option value={params.get('repository_id')!}>Выбранный репозиторий</option>}
              {repositories.data?.items
                .filter((item) => item.namespace)
                .map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.public_name}
                  </option>
                ))}
            </select>
            {repositories.isError && <span className="text-danger">Каталог недоступен</span>}
          </label>
        )}
        <label className="text-sm">
          CI-конфигурация
          <Input
            aria-label="Поиск CI-конфигурации"
            placeholder="Поиск конфигурации…"
            value={configurationSearch}
            onChange={(event) => setConfigurationSearch(event.target.value)}
          />
          <select
            className="block min-h-10 w-full rounded-md border border-border bg-surface px-3"
            value={params.get('configuration_id') ?? ''}
            onChange={(event) => update('configuration_id', event.target.value)}
          >
            <option value="">Все конфигурации</option>
            {params.get('configuration_id') &&
              !configurations.data?.items.some(
                (item) => item.id === params.get('configuration_id'),
              ) && <option value={params.get('configuration_id')!}>Выбранная конфигурация</option>}
            {configurations.data?.items
              .filter((item) => item.repository_id)
              .map((item) => (
                <option key={item.id} value={item.id}>
                  {item.name}
                </option>
              ))}
          </select>
          {configurations.isError && <span className="text-danger">Каталог недоступен</span>}
        </label>
        <label className="text-sm">
          Статус
          <select
            className="block min-h-10 rounded-md border border-border bg-surface px-3"
            value={params.get('status') ?? ''}
            onChange={(event) => update('status', event.target.value)}
          >
            <option value="">Все</option>
            {(deployment
              ? ['pending', 'running', 'success', 'failed']
              : ['queued', 'running', 'success', 'failed', 'canceled']
            ).map((status) => (
              <option key={status}>{status}</option>
            ))}
          </select>
        </label>
        <label className="text-sm">
          Git ref
          <Input
            name="git_ref"
            value={params.get('git_ref') ?? ''}
            onChange={(event) => update('git_ref', event.target.value)}
          />
        </label>
        <label className="text-sm">
          С
          <Input
            name="since"
            type="date"
            value={params.get('since')?.slice(0, 10) ?? ''}
            onChange={(event) => update('since', event.target.value)}
          />
        </label>
        <label className="text-sm">
          По
          <Input
            name="until"
            type="date"
            value={params.get('until')?.slice(0, 10) ?? ''}
            onChange={(event) => update('until', event.target.value)}
          />
        </label>
        <Button variant="outline">Применить</Button>
      </form>
      {list.isPending ? (
        <p role="status">Загружаем запуски…</p>
      ) : list.isError ? (
        <p role="alert" className="text-danger">
          Список запусков недоступен: {list.error.message}
        </p>
      ) : (
        <>
          <p className="text-sm text-text-muted">Всего: {list.data.total}</p>
          <ul className="divide-y divide-border rounded-lg border border-border bg-surface">
            {list.data.items.map((row) => (
              <li key={row.id} className="flex flex-wrap items-center justify-between gap-3 p-4">
                <div>
                  <Link
                    className="text-accent"
                    to={withNamespaceLocation(
                      `/catalog/repositories/${row.repository_id}`,
                      row.namespace,
                    )}
                  >
                    {row.repository_name}
                  </Link>
                  <span className="mx-2 text-text-muted">/</span>
                  <Link
                    className="text-accent"
                    to={withNamespaceLocation(
                      `/delivery-configs/${row.configuration_id}/${deployment ? 'environments' : 'pipelines'}`,
                      row.namespace,
                    )}
                  >
                    {row.configuration_name}
                  </Link>
                  <p className="mt-1 text-sm text-text-muted">
                    {row.git_ref} · {new Date(row.created_at).toLocaleString()}
                    {row.environment_name ? ` · ${row.environment_name}` : ''}
                  </p>
                </div>
                <Link
                  to={withNamespaceLocation(
                    row.pipeline_id
                      ? `/pipelines/${row.pipeline_id}`
                      : `/delivery-configs/${row.configuration_id}/environments`,
                    row.namespace,
                  )}
                  className="text-accent"
                >
                  {row.status}
                  {row.approval_state ? ` · ${row.approval_state}` : ''}
                </Link>
              </li>
            ))}
          </ul>
          {!list.data.items.length && <p className="text-text-muted">Запусков пока нет.</p>}
        </>
      )}
      <div className="flex gap-2">
        <Button
          variant="outline"
          disabled={!offset}
          onClick={() =>
            setParams((current) => {
              const next = new URLSearchParams(current)
              next.set('offset', String(Math.max(0, offset - 50)))
              return next
            })
          }
        >
          Назад
        </Button>
        <Button
          variant="outline"
          disabled={!list.data || offset + 50 >= list.data.total}
          onClick={() =>
            setParams((current) => {
              const next = new URLSearchParams(current)
              next.set('offset', String(offset + 50))
              return next
            })
          }
        >
          Далее
        </Button>
      </div>
    </section>
  )
}
