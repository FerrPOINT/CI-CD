import { useState } from 'react'
import { NamespaceLink as Link } from '@sdlc/ui/ui'
import { useParams, useSearchParams } from 'react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Button, usePlatformServices } from '@sdlc/ui/ui'
import { withNamespaceLocation } from '@sdlc/ui/lib'
import { api } from '@/api/client'
import type { components } from '@/api/schema'
import { useNamespaceContext } from '@/widgets/namespace-context'
export function CatalogPullPage() {
  const { id, number } = useParams()
  const { ref, malformed } = useNamespaceContext()
  const cache = useQueryClient()
  const [error, setError] = useState<string>()
  const [busy, setBusy] = useState(false)
  const [params] = useSearchParams()
  const { services } = usePlatformServices()
  const task = params.get('task_id'),
    tracker = params.get('tracker_instance_id')
  const linksKey = ['pull-task-links', ref?.registry_instance_id, ref?.namespace_id, id, number]
  const links = useQuery({
    queryKey: linksKey,
    queryFn: ({ signal }) =>
      api<components['schemas']['PullTaskLink'][]>(
        `/catalog/repositories/${id}/pulls/${number}/tasks`,
        { signal },
      ),
  })
  const linked =
    links.data?.some(
      (link) => link.task.task_id === task && link.task.tracker_instance_id === tracker,
    ) ?? false
  async function linkTask() {
    if (!task || !tracker) return
    setBusy(true)
    setError(undefined)
    try {
      await api(`/catalog/repositories/${id}/pulls/${number}/tasks`, {
        method: 'POST',
        body: JSON.stringify({ tracker_instance_id: tracker, task_id: task }),
      })
      await cache.invalidateQueries({ queryKey: linksKey })
    } catch (error) {
      setError(error instanceof Error ? error.message : 'Не удалось подтвердить связь')
    } finally {
      setBusy(false)
    }
  }
  const key = ['catalog-pull', ref?.registry_instance_id, ref?.namespace_id, id, number]
  const query = useQuery({
    queryKey: key,
    queryFn: ({ signal }) =>
      api<components['schemas']['PullRequest']>(`/catalog/repositories/${id}/pulls/${number}`, {
        signal,
      }),
  })
  const repository = useQuery({
    queryKey: ['catalog-repository', ref?.registry_instance_id, ref?.namespace_id, id],
    queryFn: ({ signal }) =>
      api<components['schemas']['CatalogRepository']>(`/catalog/repositories/${id}`, { signal }),
  })
  async function act(action: string) {
    setBusy(true)
    setError(undefined)
    try {
      await api(`/catalog/repositories/${id}/pulls/${number}/action`, {
        method: 'POST',
        body: JSON.stringify({ action }),
      })
      await cache.invalidateQueries({ queryKey: key })
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Операция отклонена')
    } finally {
      setBusy(false)
    }
  }
  if (query.isPending || repository.isPending) return <p role="status">Загружаем PR…</p>
  if (
    malformed ||
    query.isError ||
    !query.data ||
    repository.isError ||
    !repository.data ||
    (ref &&
      (repository.data.namespace?.namespace_id !== ref.namespace_id ||
        repository.data.namespace?.registry_instance_id !== ref.registry_instance_id))
  )
    return (
      <p role="alert" className="text-danger">
        PR недоступен.
      </p>
    )
  const pr = query.data
  const trackerUrl = services.find((service) => service.key === 'task-tracker')?.ui_url
  return (
    <div className="space-y-4">
      <Link to={withNamespaceLocation(`/catalog/repositories/${id}`, ref)} className="text-accent">
        Репозиторий
      </Link>
      <h1 className="text-xl font-semibold">
        #{pr.number} {pr.title}
      </h1>
      <p>
        {pr.source_branch} → {pr.target_branch} · {pr.status}
      </p>
      <p>{pr.description}</p>
      {error && (
        <p role="alert" className="text-danger">
          {error}
        </p>
      )}
      <Link
        className="block text-accent"
        to={withNamespaceLocation(
          `/repositories/${encodeURIComponent(pr.repository_name)}/pulls/${pr.number}`,
          ref,
        )}
      >
        Diff и проверки
      </Link>
      <section className="space-y-2">
        <h2 className="font-semibold">Связанные задачи</h2>
        {links.isPending ? (
          <p role="status">Загружаем связи…</p>
        ) : links.isError ? (
          <p role="alert" className="text-danger">
            Связи недоступны.
          </p>
        ) : links.data?.length === 0 ? (
          <p className="text-text-muted">Задач пока нет.</p>
        ) : (
          links.data?.map((link) => (
            <div
              key={`${link.task.tracker_instance_id}:${link.task.task_id}`}
              className="rounded-md border border-border p-3"
            >
              {trackerUrl ? (
                <a
                  className="text-accent"
                  href={withNamespaceLocation(
                    `${trackerUrl}/issues/${link.task.task_id}`,
                    link.namespace,
                  )}
                >
                  {link.task_key}
                </a>
              ) : (
                <span>{link.task_key}</span>
              )}
              <p className="break-all font-mono text-xs text-text-muted">
                Commit: {link.source_commit_sha}
              </p>
            </div>
          ))
        )}
      </section>
      {task && tracker && repository.data.state === 'active' && (
        <div className="space-y-2">
          <Button
            variant="outline"
            disabled={busy || linked || links.isPending || links.isError}
            onClick={() => void linkTask()}
          >
            {linked ? 'PR связан с задачей' : 'Связать с задачей из Tracker'}
          </Button>
          <p className="text-sm text-text-muted">
            Связь закрепляет исходный commit. Проверки в задаче относятся к этому commit и merge
            commit.
          </p>
        </div>
      )}
      {pr.status === 'open' &&
        repository.data.state === 'active' &&
        !repository.data.availability && (
          <div className="flex gap-3">
            <Button disabled={busy} onClick={() => act('merge')}>
              Merge
            </Button>
            <Button variant="outline" disabled={busy} onClick={() => act('close')}>
              Закрыть
            </Button>
          </div>
        )}
    </div>
  )
}
