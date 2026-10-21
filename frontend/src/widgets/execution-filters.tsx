import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Button, Input, Select, FormField, SearchableSelect } from '@pdlc/ui/ui'
import type { NamespaceLocation } from '@pdlc/ui/lib'
import { api } from '@/api/client'
import type { components } from '@/api/schema'
import type { Environment } from '@/api/types'
import { useWorkspace } from '@/api/workspaces'
import { executionBoundary } from '@/api/execution-filters'

export const executionFilterKeys = [
  'repository_id',
  'configuration_id',
  'status',
  'git_ref',
  'since',
  'until',
  'environment_id',
  'approval_state',
] as const
export type ExecutionFilters = Record<
  (typeof executionFilterKeys)[number],
  string
>
export function readExecutionFilters(
  params: URLSearchParams,
): ExecutionFilters {
  return Object.fromEntries(
    executionFilterKeys.map((key) => [key, params.get(key) ?? '']),
  ) as ExecutionFilters
}
export function selectedResourceKey(
  kind: string,
  namespace?: NamespaceLocation,
  id = '',
) {
  return [
    'execution-selected',
    kind,
    namespace?.registry_instance_id,
    namespace?.namespace_id,
    id,
  ]
}

export function ResourceChoice({
  kind,
  namespace,
  repositoryId,
  value,
  onChange,
  allowEmpty = true,
}: {
  kind: 'repository' | 'configuration'
  namespace?: NamespaceLocation
  repositoryId?: string
  value: string
  onChange: (value: string) => void
  allowEmpty?: boolean
}) {
  const workspace = useWorkspace(namespace ?? null)
  const [search, setSearch] = useState('')
  const [offset, setOffset] = useState(0)
  const query = new URLSearchParams({
    limit: '50',
    offset: String(offset),
    search,
  })
  query.set('managed', 'true')
  if (namespace && kind === 'repository' && workspace.data?.group_id)
    query.set('group_id', workspace.data.group_id)
  if (namespace && kind === 'configuration') {
    query.set('registry_instance_id', namespace.registry_instance_id)
    query.set('namespace_id', namespace.namespace_id)
  }
  const path =
    kind === 'repository'
      ? '/catalog/repositories'
      : repositoryId
        ? `/catalog/repositories/${repositoryId}/delivery-configs`
        : '/delivery-configurations'
  const choices = useQuery<
    components['schemas']['CatalogPage'] | components['schemas']['ConfigPage']
  >({
    queryKey: [
      'execution-choices',
      kind,
      namespace?.registry_instance_id,
      namespace?.namespace_id,
      repositoryId,
      query.toString(),
    ],
    enabled: !namespace || !!workspace.data?.group_id,
    queryFn: ({ signal }) =>
      kind === 'repository'
        ? api<components['schemas']['CatalogPage']>(`${path}?${query}`, {
            signal,
          })
        : api<components['schemas']['ConfigPage']>(`${path}?${query}`, {
            signal,
          }),
  })
  const selected = useQuery({
    queryKey: selectedResourceKey(kind, namespace, value),
    enabled: !!value,
    queryFn: ({ signal }) =>
      kind === 'repository'
        ? api<components['schemas']['CatalogRepository']>(
            `/catalog/repositories/${value}`,
            { signal },
          ).then((item) => item.public_name)
        : api<components['schemas']['DeliveryConfig']>(
            `/delivery-configurations/${value}`,
            { signal },
          ).then((item) => `${item.name}${item.repository_name ? ` · ${item.repository_name}` : ''}`),
  })
  const label = kind === 'repository' ? 'Репозиторий' : 'CI-конфигурация'
  return (
    <SearchableSelect
      label={label}
      value={value}
      selectedLabel={selected.data}
      placeholder={
        kind === 'repository'
          ? 'Все репозитории'
          : allowEmpty
            ? 'Все конфигурации'
            : 'Выберите конфигурацию'
      }
      allowEmpty={allowEmpty}
      loading={choices.isPending}
      error={choices.isError ? 'Каталог недоступен.' : undefined}
      onRetry={() => void choices.refetch()}
      search={search}
      onSearch={(next) => {
        setSearch(next)
        setOffset(0)
      }}
      offset={offset}
      pageSize={50}
      total={choices.data?.total ?? 0}
      onPage={setOffset}
      onChange={onChange}
      options={(choices.data?.items ?? [])
        .filter((item) =>
          'public_name' in item ? item.namespace : item.repository_id,
        )
        .map((item) => ({
          value: item.id,
          label:
            'public_name' in item
              ? item.public_name
              : `${item.name}${item.repository_name ? ` · ${item.repository_name}` : ''}`,
        }))}
    />
  )
}

export function ExecutionFilterForm({
  params,
  namespace,
  repositoryId,
  deployment,
  onApply,
  onCancel,
}: {
  params: URLSearchParams
  namespace?: NamespaceLocation
  repositoryId?: string
  deployment: boolean
  onApply: (filters: ExecutionFilters) => void
  onCancel: () => void
}) {
  const [draft, setDraft] = useState(() => readExecutionFilters(params))
  const [error, setError] = useState('')
  function update(key: keyof ExecutionFilters, value: string) {
    setDraft((current) => ({ ...current, [key]: value }))
    setError('')
  }
  const environments = useQuery({
    queryKey: [
      'execution-environments',
      namespace?.registry_instance_id,
      namespace?.namespace_id,
      draft.configuration_id,
    ],
    enabled: deployment && !!draft.configuration_id,
    queryFn: ({ signal }) =>
      api<Environment[]>(`/projects/${draft.configuration_id}/environments`, {
        signal,
      }),
  })
  return (
    <form
      className="space-y-4"
      onSubmit={(event) => {
        event.preventDefault()
        const data = new FormData(event.currentTarget)
        // Native calendar edits can commit at submit/blur; preserve exact ISO bookmarks when the displayed day is unchanged.
        const calendarValue = (key: 'since' | 'until') => {
          const value = String(data.get(key) ?? '')
          return value === draft[key].slice(0, 10) ? draft[key] : value
        }
        const submitted = { ...draft, since: calendarValue('since'), until: calendarValue('until') }
        setDraft(submitted)
        const since = submitted.since && executionBoundary(submitted.since, false)
        const until = submitted.until && executionBoundary(submitted.until, true)
        if (
          (since && Number.isNaN(Date.parse(since))) ||
          (until && Number.isNaN(Date.parse(until)))
        ) {
          setError('Укажите корректные даты.')
          return
        }
        if (since && until && since > until) {
          setError('Дата начала позже даты окончания.')
          return
        }
        onApply(submitted)
      }}
    >
      <div className="grid min-w-0 gap-4 sm:grid-cols-2">
        {!repositoryId && (
          <div className="space-y-1.5">
            <p className="text-sm font-medium">Репозиторий</p>
            <ResourceChoice
              kind="repository"
              namespace={namespace}
              value={draft.repository_id}
              onChange={(value) =>
                setDraft((current) => ({
                  ...current,
                  repository_id: value,
                  configuration_id: '',
                  environment_id: '',
                }))
              }
            />
          </div>
        )}
        <div className="space-y-1.5">
          <p className="text-sm font-medium">CI-конфигурация</p>
          <ResourceChoice
            kind="configuration"
            namespace={namespace}
            repositoryId={repositoryId ?? draft.repository_id}
            value={draft.configuration_id}
            onChange={(value) =>
              setDraft((current) => ({
                ...current,
                configuration_id: value,
                environment_id: '',
              }))
            }
          />
        </div>
        <FormField id="execution-status" label="Статус">
          {(attrs) => (
            <Select {...attrs}
value={draft.status}
onValueChange={(nextValue) => update('status', nextValue)} options={[{ value: "", label: "Все статусы" }, ...((deployment
        ? [
            ['pending', 'Ожидает'],
            ['running', 'Выполняется'],
            ['success', 'Успешно'],
            ['failed', 'Ошибка']
        ]
        : [
            ['queued', 'В очереди'],
            ['running', 'Выполняется'],
            ['success', 'Успешно'],
            ['failed', 'Ошибка'],
            ['canceled', 'Отменён']
        ]).map(([value, label]) => ({ value: String(value), label: String(label) })) ?? [])]} />
          )}
        </FormField>
        <FormField id="execution-ref" label="Ветка / тег / коммит">
          {(attrs) => (
            <Input
              {...attrs}
              value={draft.git_ref}
              onChange={(e) => update('git_ref', e.target.value)}
              placeholder="Например, main"
            />
          )}
        </FormField>
        <fieldset className="grid gap-3 sm:col-span-2 sm:grid-cols-2">
          <legend className="mb-2 text-sm font-medium">Период</legend>
          <FormField id="execution-since" label="Дата начала">
            {(attrs) => (
              <Input
                {...attrs}
                type="date"
                name="since"
                value={draft.since.slice(0, 10)}
                onChange={(e) => update('since', e.target.value)}
              />
            )}
          </FormField>
          <FormField id="execution-until" label="Дата окончания">
            {(attrs) => (
              <Input
                {...attrs}
                type="date"
                name="until"
                value={draft.until.slice(0, 10)}
                onChange={(e) => update('until', e.target.value)}
              />
            )}
          </FormField>
          <div className="flex flex-wrap gap-2 sm:col-span-2">
            {[7, 30].map((days) => (
              <Button
                key={days}
                type="button"
                size="sm"
                variant="outline"
                onClick={() => {
                  const start = new Date()
                  start.setDate(start.getDate() - days + 1)
                  const localDate = (date: Date) =>
                    `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`
                  setDraft((current) => ({
                    ...current,
                    since: localDate(start),
                    until: localDate(new Date()),
                  }))
                  setError('')
                }}
              >
                Последние {days} дней
              </Button>
            ))}
          </div>
        </fieldset>
        {deployment && (
          <>
            <FormField
              id="execution-environment"
              label="Окружение"
              hint={
                !draft.configuration_id
                  ? 'Сначала выберите CI-конфигурацию.'
                  : undefined
              }
            >
              {(attrs) => (
                <Select {...attrs}
disabled={
                    !draft.configuration_id ||
                    environments.isPending ||
                    environments.isError
                  }
value={draft.environment_id}
onValueChange={(nextValue) => update('environment_id', nextValue)} options={[{ value: "", label: "Все окружения" }, ...(environments.data?.map((item) => ({ value: String(item.id), label: String(item.name) })) ?? [])]} />
              )}
            </FormField>
            <FormField id="execution-approval" label="Подтверждение">
              {(attrs) => (
                <Select {...attrs}
value={draft.approval_state}
onValueChange={(nextValue) => update('approval_state', nextValue)} options={[{ value: "", label: "Любое" }, ...([
        ['pending', 'Ожидает подтверждения'],
        ['approved', 'Подтверждено'],
        ['rejected', 'Отклонено'],
        ['not_required', 'Не требуется']
    ].map(([value, label]) => ({ value: String(value), label: String(label) })) ?? [])]} />
              )}
            </FormField>
            {environments.isError && (
              <p role="alert">
                Окружения недоступны.{' '}
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => void environments.refetch()}
                >
                  Повторить
                </Button>
              </p>
            )}
          </>
        )}
      </div>
      {error && (
        <p role="alert" className="text-danger">
          {error}
        </p>
      )}
      <div className="flex flex-wrap justify-end gap-2">
        <Button type="button" variant="outline" onClick={onCancel}>
          Отмена
        </Button>
        <Button type="submit">Применить</Button>
      </div>
    </form>
  )
}
