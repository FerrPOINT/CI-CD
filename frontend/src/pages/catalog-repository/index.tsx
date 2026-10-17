import { LoadingState } from '@pdlc/ui/ui'
import { useState, type FormEvent } from 'react'
import { NamespaceLink as Link } from '@pdlc/ui/ui'
import { useParams, useSearchParams } from 'react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Button,
  Input,
  Label,
  SectionNavigation,
  SectionNavigationItem,
  Dialog,
  DialogContent,
  DialogTitle,
} from '@pdlc/ui/ui'
import { withNamespaceLocation } from '@pdlc/ui/lib'
import { api } from '@/api/client'
import { RepositoryConfigurations } from '@/widgets/repository-configurations'
import { RepositoryBrowserPage } from '@/pages/repository-browser'
import { ExecutionList } from '@/widgets/execution-list'
import { useWorkspace, workspacePath } from '@/api/workspaces'
import { ProjectAvatar } from '@pdlc/ui/ui'
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
  const [showPullForm, setShowPullForm] = useState(false)
  const [copied, setCopied] = useState(false)
  const tab = params.get('tab') ?? 'git'
  function tabLocation(nextTab: string) {
    const next = new URLSearchParams(params)
    next.set('tab', nextTab)
    for (const key of [
      'offset',
      'repository_id',
      'configuration_id',
      'status',
      'git_ref',
      'since',
      'until',
        'environment_id',
        'approval_state',
    ])
      next.delete(key)
    return `/catalog/repositories/${id}?${next}`
  }
  const repository = useQuery({
    queryKey: [
      'catalog-repository',
      ref?.registry_instance_id,
      ref?.namespace_id,
      id,
    ],
    queryFn: ({ signal }) =>
      api<components['schemas']['CatalogRepository']>(
        `/catalog/repositories/${id}`,
        { signal },
      ),
  })
  const repo = repository.data
  const project = useWorkspace(repo?.namespace ?? ref)
  const pulls = useQuery({
    queryKey: [
      'catalog-pulls',
      ref?.registry_instance_id,
      ref?.namespace_id,
      id,
    ],
    enabled: repo?.kind === 'hosted' && repo.ready,
    queryFn: ({ signal }) =>
      api<components['schemas']['PullRequest'][]>(
        `/catalog/repositories/${id}/pulls`,
        { signal },
      ),
  })
  async function createPull(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const form = event.currentTarget
    const data = new FormData(form)
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
        queryKey: [
          'catalog-pulls',
          ref?.registry_instance_id,
          ref?.namespace_id,
          id,
        ],
      })
      form.reset()
      setShowPullForm(false)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Не удалось создать PR')
    } finally {
      setBusy(false)
    }
  }
  if (repository.isPending) return <LoadingState message="Загружаем репозиторий…" />
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
      <Link
        to={withNamespaceLocation(
          repo.namespace
            ? workspacePath(repo.namespace, 'repositories')
            : '/repositories',
          repo.namespace ?? null,
        )}
        className="text-accent"
      >
        Репозитории проекта
      </Link>
      {project.data && (
        <p className="flex items-center gap-2 text-sm">
          <ProjectAvatar projectKey={project.data.project_key} size="xs" />
          {project.data.name} · {project.data.project_key}
        </p>
      )}
      <div className="flex flex-wrap items-center justify-between gap-3"><h1 className="text-xl font-semibold">{repo.public_name}</h1>{repo.kind === 'hosted' && <Link className="text-accent" to={withNamespaceLocation(`/catalog/repositories/${id}/sync`,repo.namespace ?? null)}>Синхронизация с внешним репозиторием</Link>}</div>
      <SectionNavigation aria-label="Разделы репозитория">
        {[
          ['git', 'Git / PR'],
          ...(repo.kind === 'hosted' ? [['files', 'Файлы и история']] : []),
          ['pipelines', 'Пайплайны'],
          ['deployments', 'Деплои'],
          ['configurations', 'CI-конфигурации'],
        ].map(([key, label]) => (
          <SectionNavigationItem key={key} asChild active={tab === key}>
            <Link
              to={withNamespaceLocation(
                tabLocation(key),
                repo.namespace ?? null,
              )}
            >
              {label}
            </Link>
          </SectionNavigationItem>
        ))}
      </SectionNavigation>
      {!['git', 'files', 'pipelines', 'deployments', 'configurations'].includes(tab) && <p role="alert">Раздел репозитория не найден.</p>}
      {tab === 'files' && repo.kind === 'external' && <p role="status">Файлы внешнего репозитория доступны у Git-провайдера.</p>}
      {tab === 'files' &&
        repo.kind === 'hosted' &&
        repo.storage_name &&
        (repo.availability ? (
          <p role="alert" className="text-danger">
            Git-хранилище недоступно.
          </p>
        ) : (
          <RepositoryBrowserPage repositoryName={repo.storage_name} embedded />
        ))}
      {tab === 'pipelines' && (
        <ExecutionList
          key={id + tab}
          namespace={repo.namespace ?? undefined}
          repositoryId={id}
        />
      )}
      {tab === 'deployments' && (
        <ExecutionList
          key={id + tab}
          namespace={repo.namespace ?? undefined}
          repositoryId={id}
          deployment
        />
      )}
      {tab === 'configurations' && (
        <RepositoryConfigurations
          key={id}
          repositoryId={id!}
          writable={repo.state === 'active' && repo.ready && !repo.availability}
        />
      )}
      <div hidden={tab !== 'git'} className="space-y-5">
        {repo.availability && (
          <p role="alert" className="text-danger">
            Git-хранилище недоступно. История каталога сохранена; для push и
            merge требуется восстановить исходные данные.
          </p>
        )}
        <div className="space-y-2">
          <Label htmlFor="repository-clone-url">
            {repo.kind === 'hosted' ? 'Clone URL' : 'URL внешнего Git'}
          </Label>
          <div className="flex min-w-0 gap-2">
            <Input
              id="repository-clone-url"
              readOnly
              value={clone ?? ''}
              className="font-mono"
            />
            <Button
              type="button"
              variant="outline"
              onClick={() => {
                void navigator.clipboard
                  .writeText(clone ?? '')
                  .then(() => {
                    setError(undefined)
                    setCopied(true)
                  })
                  .catch(() => {
                    setCopied(false)
                    setError(
                      'Не удалось скопировать. Выделите URL и скопируйте вручную.',
                    )
                  })
              }}
            >
              {copied ? 'Скопировано' : 'Копировать'}
            </Button>
          </div>
        </div>
        {repo.kind === 'external' && (
          <p className="text-sm text-text-muted">
            Используется внешний Git и существующий CI. Список PR остаётся у
            провайдера.
          </p>
        )}
        {error && (
          <p role="alert" className="text-danger">
            {error}
          </p>
        )}
        {repo.kind === 'hosted' && (
          <section className="space-y-3">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <h2 className="font-semibold">Pull requests</h2>
              {repo.state === 'active' && !repo.availability && (
                <Button type="button" onClick={() => setShowPullForm(true)}>
                  Создать PR
                </Button>
              )}
            </div>
            {pulls.isPending ? (
              <LoadingState message="Загружаем PR…" />
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
                  #{pr.number} {pr.title} ·{' '}
                  {{ open: 'Открыт', closed: 'Закрыт', merged: 'Слит' }[
                    pr.status
                  ] ?? pr.status}
                </Link>
              ))
            )}
          </section>
        )}
        {repo.state === 'active' &&
          !repo.availability &&
          repo.kind === 'hosted' && (
            <Dialog
              open={showPullForm}
              onOpenChange={(open) => {
                if (!busy) setShowPullForm(open)
              }}
            >
              <DialogContent aria-describedby={undefined}>
                <DialogTitle>Создать PR</DialogTitle>
                <form
                  onSubmit={createPull}
                  className="max-w-xl space-y-3 rounded-lg border border-border bg-surface p-4"
                >
                  <Label htmlFor="pr-title">Название</Label>
                  <Input id="pr-title" name="title" required />
                  <Label htmlFor="pr-source">Исходная ветка</Label>
                  <Input id="pr-source" name="source_branch" required />
                  <Label htmlFor="pr-target">Целевая ветка</Label>
                  <Input
                    id="pr-target"
                    name="target_branch"
                    defaultValue="main"
                    required
                  />
                  <Button disabled={busy}>Создать PR</Button>
                </form>
              </DialogContent>
            </Dialog>
          )}
      </div>
    </div>
  )
}
