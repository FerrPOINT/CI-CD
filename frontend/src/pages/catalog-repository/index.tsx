import { useState, type FormEvent } from 'react'
import { NamespaceLink as Link } from '@sdlc/ui/ui'
import { useParams, useSearchParams } from 'react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Button, Input, Label } from '@sdlc/ui/ui'
import { withNamespaceLocation } from '@sdlc/ui/lib'
import { api } from '@/api/client'
import { useProjects } from '@/api/hooks'
import type { components } from '@/api/schema'
import { useNamespaceContext } from '@/widgets/namespace-context'
export function CatalogRepositoryPage() {
  const { id } = useParams()
  const [params] = useSearchParams()
  const intent =
    params.get('task_id') && params.get('tracker_instance_id')
      ? `?${new URLSearchParams({ task_id: params.get('task_id')!, tracker_instance_id: params.get('tracker_instance_id')! })}`
      : ''
  const { ref, malformed } = useNamespaceContext()
  const cache = useQueryClient()
  const [error, setError] = useState<string>()
  const [busy, setBusy] = useState(false)
  const projects = useProjects()
  const repository = useQuery({
    queryKey: ['catalog-repository', ref?.registry_instance_id, ref?.namespace_id, id],
    queryFn: ({ signal }) =>
      api<components['schemas']['CatalogRepository']>(`/catalog/repositories/${id}`, { signal }),
  })
  const repo = repository.data
  const pulls = useQuery({
    queryKey: ['catalog-pulls', ref?.registry_instance_id, ref?.namespace_id, id],
    enabled: repo?.kind === 'hosted' && repo.ready,
    queryFn: ({ signal }) =>
      api<components['schemas']['PullRequest'][]>(`/catalog/repositories/${id}/pulls`, { signal }),
  })
  async function createPull(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const data = new FormData(event.currentTarget)
    setBusy(true)
    setError(undefined)
    try {
      await api(`/catalog/repositories/${id}/pulls`, {
        method: 'POST',
        body: JSON.stringify({
          title: data.get('title'),
          source_branch: data.get('source_branch'),
          target_branch: data.get('target_branch'),
          description: '',
        }),
      })
      await cache.invalidateQueries({
        queryKey: ['catalog-pulls', ref?.registry_instance_id, ref?.namespace_id, id],
      })
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Не удалось создать PR')
    } finally {
      setBusy(false)
    }
  }
  async function connect(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const data = new FormData(event.currentTarget)
    setBusy(true)
    setError(undefined)
    try {
      await api(`/catalog/repositories/${id}/delivery-configs/${data.get('project')}`, {
        method: 'PUT',
      })
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Не удалось подключить CI')
    } finally {
      setBusy(false)
    }
  }
  if (repository.isPending) return <p role="status">Загружаем репозиторий…</p>
  if (malformed || repository.isError || !repo)
    return (
      <p role="alert" className="text-danger">
        Репозиторий недоступен.
      </p>
    )
  if (
    ref &&
    (!repo.namespace ||
      ref.namespace_id !== repo.namespace.namespace_id ||
      ref.registry_instance_id !== repo.namespace.registry_instance_id)
  )
    return (
      <p role="alert" className="text-danger">
        Репозиторий принадлежит другому проекту.
      </p>
    )
  const clone = repo.clone_url ?? repo.external_url
  return (
    <div className="space-y-5">
      <Link to={withNamespaceLocation('/namespace', ref)} className="text-accent">
        Репозитории проекта
      </Link>
      <h1 className="text-xl font-semibold">{repo.public_name}</h1>
      {repo.availability && (
        <p role="alert" className="text-danger">
          Git-хранилище недоступно. История каталога сохранена; для push и merge требуется
          восстановить исходные данные.
        </p>
      )}
      <p className="break-all rounded-md border border-border bg-surface p-3 font-mono text-sm">
        {clone}
      </p>
      {repo.kind === 'hosted' && repo.storage_name && (
        <Link
          className="text-accent"
          to={withNamespaceLocation(`/repositories/${encodeURIComponent(repo.storage_name)}`, ref)}
        >
          Файлы и история
        </Link>
      )}
      {repo.kind === 'external' && (
        <p className="text-sm text-text-muted">
          Используется внешний Git и существующий CI. Список PR остаётся у провайдера.
        </p>
      )}
      {error && (
        <p role="alert" className="text-danger">
          {error}
        </p>
      )}
      {repo.kind === 'hosted' && (
        <section className="space-y-3">
          <h2 className="font-semibold">Pull requests</h2>
          {pulls.isPending ? (
            <p role="status">Загружаем PR…</p>
          ) : pulls.isError ? (
            <p role="alert" className="text-danger">
              PR недоступны.
            </p>
          ) : pulls.data?.length === 0 ? (
            <p className="text-text-muted">PR пока нет.</p>
          ) : (
            pulls.data?.map((pr) => (
              <Link
                key={pr.id}
                className="block text-accent"
                to={withNamespaceLocation(
                  `/catalog/repositories/${id}/pulls/${pr.number}${intent}`,
                  ref,
                )}
              >
                #{pr.number} {pr.title} · {pr.status}
              </Link>
            ))
          )}
        </section>
      )}
      {repo.state === 'active' && !repo.availability && repo.kind === 'hosted' && (
        <form
          onSubmit={createPull}
          className="max-w-xl space-y-3 rounded-lg border border-border bg-surface p-4"
        >
          <h2 className="font-semibold">Создать PR</h2>
          <Label htmlFor="pr-title">Название</Label>
          <Input id="pr-title" name="title" required />
          <Label htmlFor="pr-source">Исходная ветка</Label>
          <Input id="pr-source" name="source_branch" required />
          <Label htmlFor="pr-target">Целевая ветка</Label>
          <Input id="pr-target" name="target_branch" defaultValue="main" required />
          <Button disabled={busy}>Создать PR</Button>
        </form>
      )}
      {repo.state === 'active' && (
        <form
          onSubmit={connect}
          className="max-w-xl space-y-3 rounded-lg border border-border bg-surface p-4"
        >
          <h2 className="font-semibold">Подключить CI</h2>
          <p className="text-sm text-text-muted">
            Выберите существующую конфигурацию. Связь задаётся явно и сохраняет её историю.
          </p>
          <Label htmlFor="delivery-project">Конфигурация</Label>
          {projects.isError ? (
            <p role="alert" className="text-danger">
              Конфигурации недоступны.
            </p>
          ) : (
            <select
              id="delivery-project"
              name="project"
              required
              className="min-h-10 w-full rounded-md border border-border bg-surface px-3 text-sm"
            >
              <option value="">Выберите конфигурацию</option>
              {projects.data?.map((project) => (
                <option key={project.id} value={project.id}>
                  {project.name}
                </option>
              ))}
            </select>
          )}
          <Button disabled={busy || projects.isPending || projects.isError}>Подключить</Button>
        </form>
      )}
    </div>
  )
}
