import { useState, type FormEvent } from 'react'
import { NamespaceLink as Link } from '@sdlc/ui/ui'

import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Button, Input, Label } from '@sdlc/ui/ui'
import { useSessionCommand, withNamespaceLocation } from '@sdlc/ui/lib'
import { api } from '@/api/client'
import { useAuth } from '@/shared/auth/auth-provider'
import type { components } from '@/api/schema'
import { useNamespaceContext } from '@/widgets/namespace-context'
import { AttachRepository } from './attach-repository'
type CreateRepository = components['schemas']['CreateRepository']
export function NamespacePage() {
  const { ref, malformed, query } = useNamespaceContext()
  const resource = query.data
  const cache = useQueryClient()
  const { session } = useAuth()
  const [offset, setOffset] = useState(0)
  const [error, setError] = useState<string>()
  const [busy, setBusy] = useState(false)
  const [kind, setKind] = useState<'hosted' | 'external'>('hosted')
  const journal = useSessionCommand<CreateRepository>(
    `forge:repository-create:${session?.subject}:${ref?.registry_instance_id}:${ref?.namespace_id}`,
  )
  const repositories = useQuery({
    queryKey: [
      'namespace-repositories',
      ref?.registry_instance_id,
      ref?.namespace_id,
      resource?.binding.resource.resource_id,
      offset,
    ],
    enabled: Boolean(resource),
    queryFn: ({ signal }) =>
      api<components['schemas']['CatalogPage']>(
        `/git-groups/${resource!.binding.resource.resource_id}/repositories?limit=50&offset=${offset}`,
        { signal },
      ),
  })
  async function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!session?.subject) {
      setError('Для создания требуется действующая SSO-сессия')
      return
    }
    const data = new FormData(event.currentTarget)
    const command = journal.capture({
      id: crypto.randomUUID(),
      slug: String(data.get('slug')),
      kind,
      visibility: String(data.get('visibility')),
      external_url: kind === 'external' ? String(data.get('external_url')) : null,
      provider_refs: {},
    })
    setBusy(true)
    setError(undefined)
    try {
      await api(`/git-groups/${resource!.binding.resource.resource_id}/repositories`, {
        method: 'POST',
        body: JSON.stringify(command),
      })
      journal.clear()
      await cache.invalidateQueries({
        queryKey: ['namespace-repositories', ref?.registry_instance_id, ref?.namespace_id],
      })
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Не удалось зарегистрировать репозиторий')
    } finally {
      setBusy(false)
    }
  }
  if (malformed)
    return (
      <p role="alert" className="text-danger">
        Некорректная ссылка на проект.
      </p>
    )
  if (!ref)
    return (
      <div className="space-y-3">
        <h1 className="text-xl font-semibold">Репозитории проекта</h1>
        <p>Выберите проект в верхней панели.</p>
        <Link className="text-accent" to="/repositories">
          Каталог Forge
        </Link>
      </div>
    )
  if (query.isPending) return <p role="status">Разрешаем привязку Git-группы…</p>
  if (query.isError || !resource)
    return (
      <p role="alert" className="text-danger">
        Привязка Git-группы недоступна. Другая группа не выбрана.
      </p>
    )
  return (
    <div className="space-y-5">
      <h1 className="text-xl font-semibold">{resource.label}</h1>
      <p className="text-sm text-text-muted">
        {resource.binding.state === 'archived'
          ? 'Проект в архиве. Push, merge и новые pipeline закрыты.'
          : 'Репозитории проекта'}
      </p>
      {repositories.isPending ? (
        <p role="status">Загружаем репозитории…</p>
      ) : repositories.isError ? (
        <p role="alert" className="text-danger">
          Каталог репозиториев недоступен.
        </p>
      ) : (
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          {repositories.data?.items.map((repo) => (
            <Link
              key={repo.id}
              to={withNamespaceLocation(`/catalog/repositories/${repo.id}`, ref)}
              className="rounded-lg border border-border bg-surface p-4"
            >
              <h2 className="font-medium">{repo.public_name}</h2>
              <p className="mt-2 text-sm text-text-muted">
                {repo.kind === 'hosted' ? 'Git в Forge' : 'Внешний Git'} ·{' '}
                {repo.availability
                  ? 'Git-хранилище недоступно'
                  : repo.ready
                    ? 'Подключён'
                    : 'Инициализируется'}
              </p>
            </Link>
          ))}
          {repositories.data?.items.length === 0 && (
            <p className="text-text-muted">Репозиториев пока нет.</p>
          )}
        </div>
      )}
      <div className="flex gap-2">
        <Button
          variant="outline"
          disabled={offset === 0}
          onClick={() => setOffset(Math.max(0, offset - 50))}
        >
          Назад
        </Button>
        <Button
          variant="outline"
          disabled={(repositories.data?.items.length ?? 0) < 50}
          onClick={() => setOffset(offset + 50)}
        >
          Далее
        </Button>
      </div>
      {resource.binding.state === 'active' && (
        <AttachRepository groupId={resource.binding.resource.resource_id} namespace={ref} />
      )}
      {resource.binding.state === 'active' && (
        <form
          onSubmit={create}
          className="max-w-xl space-y-4 rounded-lg border border-border bg-surface p-4"
        >
          <h2 className="font-semibold">Добавить репозиторий</h2>
          <div className="space-y-2">
            <Label htmlFor="repo-slug">Имя репозитория</Label>
            <Input id="repo-slug" name="slug" required />
          </div>
          <div className="space-y-2">
            <Label htmlFor="repo-kind">Git</Label>
            <select
              id="repo-kind"
              value={kind}
              onChange={(e) => setKind(e.target.value as 'hosted' | 'external')}
              className="min-h-10 w-full rounded-md border border-border bg-surface px-3 text-sm"
            >
              <option value="hosted">Создать в Forge</option>
              <option value="external">Подключить внешний репозиторий</option>
            </select>
          </div>
          {kind === 'external' && (
            <div className="space-y-2">
              <Label htmlFor="external-url">Git URL</Label>
              <Input id="external-url" name="external_url" required />
            </div>
          )}
          <div className="space-y-2">
            <Label htmlFor="visibility">Видимость Git</Label>
            <select
              id="visibility"
              name="visibility"
              className="min-h-10 w-full rounded-md border border-border bg-surface px-3 text-sm"
            >
              <option value="private">По авторизации</option>
              <option value="public">Публичное чтение</option>
            </select>
          </div>
          {error && (
            <p role="alert" className="text-danger">
              {error}. Повтор использует исходный репозиторий.
            </p>
          )}
          <Button disabled={busy}>
            {busy ? 'Добавляем…' : journal.command ? 'Повторить исходную операцию' : 'Добавить'}
          </Button>
        </form>
      )}
    </div>
  )
}
