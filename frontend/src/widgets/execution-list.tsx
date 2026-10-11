import { useQuery } from '@tanstack/react-query'
import { useSearchParams } from 'react-router'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Button,
  NamespaceLink as Link,
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
} from '@sdlc/ui/ui'
import { type NamespaceLocation, withNamespaceLocation } from '@sdlc/ui/lib'
import type { components } from '@/api/schema'
import { api, ApiError } from '@/api/client'
import { workspaceKey, workspacePath } from '@/api/workspaces'
import { executionBoundary } from '@/api/execution-filters'
import {
  executionFilterKeys,
  ExecutionFilterForm,
  readExecutionFilters,
  selectedResourceKey,
} from './execution-filters'

export function ExecutionList({
  namespace,
  repositoryId,
  deployment = false,
  preview = false,
}: {
  namespace?: NamespaceLocation
  repositoryId?: string
  deployment?: boolean
  preview?: boolean
}) {
  const { t } = useTranslation()
  const [params, setParams] = useSearchParams()
  const [filtersOpen, setFiltersOpen] = useState(false)
  const applied = readExecutionFilters(preview ? new URLSearchParams() : params)
  const activeKeys = executionFilterKeys.filter(
    (key) => applied[key] && !(repositoryId && key === 'repository_id'),
  )
  const requestedOffset = Number(params.get('offset'))
  const offset = preview || !Number.isSafeInteger(requestedOffset) ? 0 : Math.max(0, requestedOffset)
  const pageSize = preview ? 5 : 50
  const query = new URLSearchParams({
    limit: String(pageSize),
    offset: String(offset),
  })
  for (const key of executionFilterKeys)
    if (applied[key])
      query.set(
        key,
        ['since', 'until'].includes(key)
          ? executionBoundary(applied[key], key === 'until')
          : applied[key],
      )
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
      api<components['schemas']['ExecutionPage']>(`${path}?${query}`, {
        signal,
      }),
    refetchInterval: (current) =>
      current.state.data?.items.some((row) =>
        ['queued', 'running', 'pending'].includes(row.status),
      )
        ? 3000
        : false,
  })
  const emptyConfigurations = useQuery({
    queryKey: [
      'execution-empty-configurations',
      namespace?.registry_instance_id,
      namespace?.namespace_id,
      repositoryId,
    ],
    enabled: list.data?.total === 0 && !activeKeys.length,
    queryFn: ({ signal }) => {
      const scope = namespace
        ? `&registry_instance_id=${namespace.registry_instance_id}&namespace_id=${namespace.namespace_id}`
        : ''
      return api<components['schemas']['ConfigPage']>(
        `${repositoryId ? `/catalog/repositories/${repositoryId}/delivery-configs` : '/delivery-configurations'}?limit=1&managed=true${scope}`,
        { signal },
      )
    },
  })
  const selectedRepository = useQuery({
    queryKey: selectedResourceKey(
      'repository',
      namespace,
      applied.repository_id,
    ),
    enabled: !!applied.repository_id,
    queryFn: ({ signal }) =>
      api<components['schemas']['CatalogRepository']>(
        `/catalog/repositories/${applied.repository_id}`,
        { signal },
      ).then((item) => item.public_name),
  })
  const selectedConfiguration = useQuery({
    queryKey: selectedResourceKey(
      'configuration',
      namespace,
      applied.configuration_id,
    ),
    enabled: !!applied.configuration_id,
    queryFn: ({ signal }) =>
      api<components['schemas']['DeliveryConfig']>(
        `/delivery-configurations/${applied.configuration_id}`,
        { signal },
      ).then((item) => `${item.name}${item.repository_name ? ` · ${item.repository_name}` : ''}`),
  })
  const denied =
    list.error instanceof ApiError && [401, 403].includes(list.error.status)
  const display = denied ? undefined : list.data
  const statusLabel = (status: string) =>
    t(
      deployment
        ? `deployments.status${status[0].toUpperCase()}${status.slice(1)}`
        : `pipelines.${status}`,
      { defaultValue: status },
    )
  const approvalLabel = (state: string) =>
    ({
      pending: 'Ожидает подтверждения',
      approved: 'Подтверждено',
      rejected: 'Отклонено',
      not_required: 'Не требуется',
    })[state] ?? state
  function reset() {
    setParams((current) => {
      const next = new URLSearchParams(current)
      executionFilterKeys.forEach((key) => next.delete(key))
      next.delete('offset')
      return next
    })
  }
  const emptyAction = activeKeys.length ? (
    <Button type="button" variant="outline" onClick={reset}>
      Сбросить фильтры
    </Button>
  ) : (
    <Link
      className="text-accent underline"
      to={
        repositoryId
          ? `/catalog/repositories/${repositoryId}?tab=configurations`
          : namespace
            ? workspacePath(namespace, 'repositories')
            : '/repositories'
      }
    >
      Открыть репозитории и CI-конфигурации
    </Link>
  )
  const errorMessage = denied
    ? 'Нет доступа к этому списку.'
    : list.error?.message === 'invalid_execution_period'
      ? 'Дата начала позже даты окончания. Измените период.'
      : list.error?.message?.includes('mismatch')
        ? 'Выбранные ресурсы не соответствуют проекту. Измените фильтры.'
        : 'Не удалось загрузить список. Повторите запрос.'
  return (
    <section
      className="min-w-0 space-y-4"
      aria-label={deployment ? 'Список деплоев' : 'Список пайплайнов'}
      aria-busy={list.isFetching || undefined}
    >
      {!preview && (
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h2 className="text-lg font-semibold">
            {deployment ? 'Деплои' : 'Пайплайны'}
          </h2>
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              variant="outline"
              onClick={() => setFiltersOpen(true)}
            >
              Фильтры{activeKeys.length ? ` (${activeKeys.length})` : ''}
            </Button>
            <Button
              type="button"
              variant="outline"
              disabled={list.isFetching}
              onClick={() => void list.refetch()}
            >
              Обновить
            </Button>
          </div>
        </div>
      )}
      {!!activeKeys.length && (
        <div className="flex flex-wrap items-center gap-2 text-sm">
          {activeKeys.map((key) => (
            <span
              key={key}
              className="max-w-full break-words rounded-md bg-surface-raised px-2 py-1"
            >
              {key === 'repository_id'
                ? (selectedRepository.data ?? 'Репозиторий недоступен')
                : key === 'configuration_id'
                  ? (selectedConfiguration.data ?? 'Конфигурация недоступна')
                  : key === 'status'
                    ? statusLabel(applied[key])
                    : key === 'approval_state'
                      ? approvalLabel(applied[key])
                      : key === 'environment_id'
                        ? 'Выбрано окружение'
                        : `${({ git_ref: 'Git', since: 'С', until: 'По' } as Record<string, string>)[key]}: ${applied[key]}`}
            </span>
          ))}
          <Button type="button" variant="ghost" onClick={reset}>
            Сбросить
          </Button>
        </div>
      )}
      <Dialog open={filtersOpen} onOpenChange={setFiltersOpen}>
        <DialogContent className="max-w-2xl">
          <DialogTitle>
            Фильтры {deployment ? 'деплоев' : 'пайплайнов'}
          </DialogTitle>
          <DialogDescription>
            Изменения применяются после нажатия «Применить».
          </DialogDescription>
          <ExecutionFilterForm
            key={params.toString()}
            params={params}
            namespace={namespace}
            repositoryId={repositoryId}
            deployment={deployment}
            onCancel={() => setFiltersOpen(false)}
            onApply={(draft) => {
              setParams((current) => {
                const next = new URLSearchParams(current)
                executionFilterKeys.forEach((key) => {
          if (draft[key].trim()) next.set(key, draft[key].trim())
                  else next.delete(key)
                })
                next.delete('offset')
                return next
              })
              setFiltersOpen(false)
            }}
          />
        </DialogContent>
      </Dialog>
      {list.isError && (
        <div
          role="alert"
          className="flex flex-wrap items-center gap-2 rounded-md border border-border p-3"
        >
          <p>
            {display
              ? 'Не удалось обновить. Показаны последние загруженные данные.'
              : errorMessage}
          </p>
          <Button
            type="button"
            variant="outline"
            onClick={() => void list.refetch()}
          >
            Повторить
          </Button>
        </div>
      )}
      {list.isPending ? (
        <p role="status">Загружаем запуски…</p>
      ) : (
        display && (
          <>
            {!!display.items.length && (
              <ul className="divide-y divide-border rounded-lg border border-border bg-surface">
                {display.items.map((row) => {
                  const target = deployment
                    ? `/delivery-configs/${row.configuration_id}/environments?environment_id=${row.environment_id}&deployment_id=${row.id}`
                    : `/pipelines/${row.pipeline_id ?? row.id}`
                  const tone =
                    row.status === 'success'
                      ? 'text-success'
                      : row.status === 'failed'
                        ? 'text-danger'
                        : ['running', 'queued', 'pending'].includes(row.status)
                          ? 'text-warning'
                          : 'text-text-muted'
                  const seconds =
                    row.started_at && row.finished_at
                      ? Math.max(
                          0,
                          Math.round(
                            (Date.parse(row.finished_at) -
                              Date.parse(row.started_at)) /
                              1000,
                          ),
                        )
                      : undefined
                  return (
                    <li
                      key={row.id}
                      className="flex min-w-0 flex-wrap items-start justify-between gap-3 p-4"
                    >
                      <div className="min-w-0 flex-1">
                        <Link
                          className="font-semibold text-accent underline-offset-2 hover:underline"
                          to={withNamespaceLocation(target, row.namespace)}
                        >
                          {deployment ? 'Деплой' : 'Пайплайн'} #
                          {row.id.slice(0, 8)}
                        </Link>
                        <div className="mt-1 flex flex-wrap gap-x-2 text-sm">
                          <Link
                            className="text-accent"
                            to={withNamespaceLocation(
                              `/catalog/repositories/${row.repository_id}`,
                              row.namespace,
                            )}
                          >
                            {row.repository_name}
                          </Link>
                          <span aria-hidden>·</span>
                          <Link
                            className="text-accent"
                            to={withNamespaceLocation(
                              `/delivery-configs/${row.configuration_id}/settings`,
                              row.namespace,
                            )}
                          >
                            {row.configuration_name}
                          </Link>
                        </div>
                        <p className="mt-1 break-words text-sm text-text-muted">
                          {row.git_ref} ·{' '}
                          <time dateTime={row.created_at}>
                            {new Date(row.created_at).toLocaleString()}
                          </time>
                          {row.environment_name
                            ? ` · ${row.environment_name}`
                            : ''}
                          {seconds !== undefined ? ` · ${seconds} с` : ''}
                        </p>
                        {row.commit_sha && (
                          <p className="mt-1 text-xs text-text-muted">
                            Коммит{' '}
                            <Link
                              className="font-mono text-accent"
                              to={withNamespaceLocation(
                                `/catalog/repositories/${row.repository_id}?tab=files&ref=${encodeURIComponent(row.commit_sha)}`,
                                row.namespace,
                              )}
                            >
                              {row.commit_sha.slice(0, 8)}
                            </Link>
                          </p>
                        )}
                      </div>
                      <div className="flex flex-wrap items-center gap-2">
                        <span
                          className={`rounded-md bg-surface-raised px-2 py-1 text-sm font-medium ${tone}`}
                        >
                          {statusLabel(row.status)}
                        </span>
                        {row.approval_state && (
                          <span className="text-sm text-text-muted">
                            {approvalLabel(row.approval_state)}
                          </span>
                        )}
                        <Link
                          className="text-accent underline"
                          to={withNamespaceLocation(target, row.namespace)}
                        >
                          Открыть
                        </Link>
                        {deployment && row.pipeline_id && (
                          <Link
                            className="text-sm text-accent"
                            to={withNamespaceLocation(
                              `/pipelines/${row.pipeline_id}`,
                              row.namespace,
                            )}
                          >
                            Пайплайн
                          </Link>
                        )}
                      </div>
                    </li>
                  )
                })}
              </ul>
            )}
            {!display.items.length && (
              <div className="space-y-3 rounded-lg border border-dashed border-border p-6 text-center">
                <p>
                  {activeKeys.length
                    ? 'По выбранным фильтрам ничего не найдено.'
                    : emptyConfigurations.data?.total === 0
                      ? 'Нет CI-конфигураций.'
                      : `${deployment ? 'Деплоев' : 'Пайплайнов'} пока нет.`}
                </p>
                <p className="text-sm text-text-muted">
                  {activeKeys.length
                    ? 'Измените условия или сбросьте фильтры.'
                    : emptyConfigurations.data?.total === 0
                      ? 'Создайте CI-конфигурацию в репозитории.'
                      : 'Запустите настроенную CI-конфигурацию в репозитории.'}
                </p>
                {!preview && emptyAction}
              </div>
            )}
            {!preview && (
              <div className="flex flex-wrap items-center justify-between gap-2">
                <p className="text-sm text-text-muted">
                  {display.total
                    ? `${offset + (display.items.length ? 1 : 0)}–${offset + display.items.length} из ${display.total}`
                    : '0 результатов'}
                </p>
                {display.total > pageSize && (
                  <nav
                    aria-label="Страницы запусков"
                    className="flex items-center gap-2"
                  >
                    <Button
                      type="button"
                      variant="outline"
                      disabled={!offset}
                      onClick={() =>
                        setParams((current) => {
                          const next = new URLSearchParams(current)
                          next.set(
                            'offset',
                            String(Math.max(0, offset - pageSize)),
                          )
                          return next
                        })
                      }
                    >
                      Назад
                    </Button>
                    <span className="text-sm">
                      {Math.floor(offset / pageSize) + 1} /{' '}
                      {Math.ceil(display.total / pageSize)}
                    </span>
                    <Button
                      type="button"
                      variant="outline"
                      disabled={offset + pageSize >= display.total}
                      onClick={() =>
                        setParams((current) => {
                          const next = new URLSearchParams(current)
                          next.set('offset', String(offset + pageSize))
                          return next
                        })
                      }
                    >
                      Далее
                    </Button>
                  </nav>
                )}
              </div>
            )}
          </>
        )
      )}
    </section>
  )
}
