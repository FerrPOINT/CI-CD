import { Navigate, useLocation, useParams } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { NamespaceLink as Link, ProjectAvatar } from '@sdlc/ui/ui'
import { withNamespaceLocation } from '@sdlc/ui/lib'
import { useWorkspace, workspaceKey, workspacePath } from '@/api/workspaces'
import { api } from '@/api/client'
import type { components } from '@/api/schema'
import { useNamespaceRef } from '@/widgets/namespace-context'
import { WorkspaceLinks } from '@/widgets/workspace-links'
import { ExecutionList } from '@/widgets/execution-list'
import { NamespacePage } from '@/pages/namespace'
export function NamespaceRedirect() {
  const { ref, malformed, all } = useNamespaceRef()
  if (malformed)
    return (
      <p role="alert" className="text-danger">
        Некорректная ссылка на проект.
      </p>
    )
  return (
    <Navigate
      replace
      to={
        ref
          ? withNamespaceLocation(workspacePath(ref) + (all ? '?project_scope=all' : ''), ref)
          : '/projects?project_scope=all'
      }
    />
  )
}
export function LegacyConfigRedirect() {
  const { projectId } = useParams()
  const location = useLocation()
  return (
    <Navigate
      replace
      to={
        location.pathname.replace(`/projects/${projectId}`, `/delivery-configs/${projectId}`) +
        location.search
      }
    />
  )
}
export function WorkspacePage() {
  const { ref, malformed, all } = useNamespaceRef()
  const { '*': section = '' } = useParams()
  const project = useWorkspace(malformed ? null : ref)
  const summary = useQuery({
    queryKey: ['workspace-summary', ref?.registry_instance_id, ref?.namespace_id],
    enabled: !!ref && !!project.data?.group_id,
    queryFn: ({ signal }) =>
      api<components['schemas']['WorkspaceSummary']>(
        `/workspace-projects/${workspaceKey(ref!)}/summary`,
        { signal },
      ),
  })
  if (malformed || !ref)
    return (
      <p role="alert" className="text-danger">
        Некорректная ссылка на проект.
      </p>
    )
  if (project.isPending) return <p role="status">Загружаем проект…</p>
  if (project.isError)
    return (
      <p role="alert" className="text-danger">
        Проект недоступен: {project.error.message}
      </p>
    )
  const data = project.data
  const sections = [
    ['', 'Обзор'],
    ['repositories', 'Репозитории'],
    ['pipelines', 'Пайплайны'],
    ['deployments', 'Деплои'],
  ]
  return (
    <div className="space-y-5">
      <Link to="/projects?project_scope=all" className="text-accent">
        Все проекты
      </Link>
      <div className="flex items-center gap-3">
        <ProjectAvatar projectKey={data.project_key} size="md" />
        <div>
          <h1 className="text-2xl font-semibold">{data.name}</h1>
          <p className="text-sm text-text-muted">
            {data.project_key} · {data.state === 'archived' ? 'В архиве' : 'Активен'} ·{' '}
            {data.group_id ? `Git-группа: ${data.group_slug}` : 'Git не подключён'}
          </p>
        </div>
      </div>
      {data.stale && (
        <p role="status" className="text-warning">
          Показываем сохранённые метаданные Tracker. Существующие репозитории и CI доступны.
        </p>
      )}
      <WorkspaceLinks namespace={ref} />
      <nav
        aria-label="Разделы проекта"
        className="flex flex-wrap gap-2 border-b border-border pb-3"
      >
        {sections.map(([path, label]) => (
          <Link
            key={path}
            aria-current={section === path ? 'page' : undefined}
            className={
              section === path
                ? 'rounded-md bg-surface-raised px-3 py-2 font-semibold'
                : 'rounded-md px-3 py-2 text-text-muted hover:bg-surface-raised'
            }
            to={withNamespaceLocation(
              workspacePath(ref, path) + (all ? '?project_scope=all' : ''),
              ref,
            )}
          >
            {label}
          </Link>
        ))}
      </nav>
      {!data.group_id ? (
        <p>Git не подключён. Подключите группу репозиториев в настройках проекта Admin.</p>
      ) : section === 'repositories' ? (
        <NamespacePage embedded />
      ) : section === 'pipelines' ? (
        <ExecutionList key={workspaceKey(ref) + section} namespace={ref} />
      ) : section === 'deployments' ? (
        <ExecutionList key={workspaceKey(ref) + section} namespace={ref} deployment />
      ) : section === '' ? (
        <>
          {summary.isError ? (
            <p role="alert" className="text-danger">
              Сводка недоступна.
            </p>
          ) : summary.isPending ? (
            <p role="status">Загружаем сводку…</p>
          ) : (
            <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-5">
              {[
                ['Репозитории', summary.data.repositories],
                ['CI-конфигурации', summary.data.configurations],
                ['В очереди', summary.data.queued],
                ['Выполняются', summary.data.running],
                ['С ошибкой', summary.data.failed],
              ].map(([label, count]) => (
                <div key={label} className="rounded-lg border border-border bg-surface p-4">
                  <p className="text-sm text-text-muted">{label}</p>
                  <p className="mt-2 text-2xl font-semibold">{count}</p>
                </div>
              ))}
            </div>
          )}
          <h2 className="font-semibold">Последние пайплайны</h2>
          <ExecutionList namespace={ref} />
        </>
      ) : (
        <p role="alert">Раздел не найден.</p>
      )}
    </div>
  )
}
