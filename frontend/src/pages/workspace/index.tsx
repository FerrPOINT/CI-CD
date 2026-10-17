import { Navigate, useLocation, useParams } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import {
  NamespaceLink as Link,
  ProjectAvatar,
  SectionNavigation,
  SectionNavigationItem,
  Button,
} from '@pdlc/ui/ui'
import { withNamespaceLocation } from '@pdlc/ui/lib'
import { useWorkspace, workspaceKey, workspacePath } from '@/api/workspaces'
import { api } from '@/api/client'
import type { components } from '@/api/schema'
import { useNamespaceRef } from '@/widgets/namespace-context'
import { WorkspaceLinks } from '@/widgets/workspace-links'
import { ExecutionList } from '@/widgets/execution-list'
import { RepositorySyncsPage } from '@/pages/repository-syncs'
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
          ? withNamespaceLocation(
              workspacePath(ref) + (all ? '?project_scope=all' : ''),
              ref,
            )
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
        location.pathname.replace(
          `/projects/${projectId}`,
          `/delivery-configs/${projectId}`,
        ) + location.search
      }
    />
  )
}
export function WorkspacePage() {
  const { ref, malformed, all } = useNamespaceRef()
  const { '*': section = '' } = useParams()
  const project = useWorkspace(malformed ? null : ref)
  const summary = useQuery({
    queryKey: [
      'workspace-summary',
      ref?.registry_instance_id,
      ref?.namespace_id,
    ],
    enabled: !!ref && !!project.data?.group_id,
    queryFn: ({ signal }) =>
      api<components['schemas']['WorkspaceSummary']>(
        `/workspace-projects/${workspaceKey(ref!)}/summary`,
        { signal },
      ),
    refetchInterval: (current) =>
      current.state.data &&
      (current.state.data.queued || current.state.data.running)
        ? 3000
        : false,
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
    ['syncs', 'Синхронизации'],
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
            {data.project_key} ·{' '}
            {data.state === 'archived' ? 'В архиве' : 'Активен'} ·{' '}
            {data.group_id
              ? `Git-группа: ${data.group_slug}`
              : 'Git не подключён'}
          </p>
        </div>
      </div>
      {data.stale && (
        <p role="status" className="text-warning">
          Показываем сохранённые метаданные Tracker. Существующие репозитории и
          CI доступны.
        </p>
      )}
      <WorkspaceLinks namespace={ref} />
      <SectionNavigation aria-label="Разделы проекта">
        {sections.map(([path, label]) => (
          <SectionNavigationItem asChild active={section === path} key={path}>
            <Link
              key={path}
              to={withNamespaceLocation(
                workspacePath(ref, path) + (all ? '?project_scope=all' : ''),
                ref,
              )}
            >
              {label}
            </Link>
          </SectionNavigationItem>
        ))}
      </SectionNavigation>
      {!data.group_id ? (
        <p>
          Git не подключён. Подключите группу репозиториев в настройках проекта
          Admin.
        </p>
      ) : section === 'repositories' ? (
        <NamespacePage embedded />
      ) : section === 'syncs' ? (
        <RepositorySyncsPage groupId={data.group_id} />
      ) : section === 'pipelines' ? (
        <ExecutionList key={workspaceKey(ref) + section} namespace={ref} />
      ) : section === 'deployments' ? (
        <ExecutionList
          key={workspaceKey(ref) + section}
          namespace={ref}
          deployment
        />
      ) : section === '' ? (
        <>
          {summary.isError && !summary.data ? (
            <div role="alert" className="flex gap-2">
              <p>Сводка недоступна.</p>
              <Button variant="outline" onClick={() => void summary.refetch()}>
                Повторить
              </Button>
            </div>
          ) : summary.isPending ? (
            <p role="status">Загружаем сводку…</p>
          ) : (
            <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-5">
              {[
                ['Репозитории', summary.data.repositories, 'repositories'],
                [
                  'CI-конфигурации',
                  summary.data.configurations,
                  'repositories',
                ],
                ['В очереди', summary.data.queued, 'pipelines?status=queued'],
                [
                  'Выполняются',
                  summary.data.running,
                  'pipelines?status=running',
                ],
                ['С ошибкой', summary.data.failed, 'pipelines?status=failed'],
              ].map(([label, count, target]) => (
                <Link
                  key={label}
                  to={withNamespaceLocation(
                    workspacePath(ref, String(target)) +
                      (all
                        ? String(target).includes('?')
                          ? '&project_scope=all'
                          : '?project_scope=all'
                        : ''),
                    ref,
                  )}
                  className="rounded-lg border border-border bg-surface p-4 hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-accent"
                >
                  <p className="text-sm text-text-muted">{label}</p>
                  <p className="mt-2 text-2xl font-semibold">{count}</p>
                </Link>
              ))}
            </div>
          )}
          <p className="text-sm text-text-muted">
            Сводка по всем репозиториям проекта за всё время.
          </p>
          {summary.isError && summary.data && (
            <p role="alert">
              Не удалось обновить сводку. Показаны последние загруженные данные.
            </p>
          )}
          <div className="flex flex-wrap items-center justify-between gap-2">
            <h2 className="font-semibold">Последние пайплайны</h2>
            <Link
              className="text-accent underline"
              to={withNamespaceLocation(
                workspacePath(ref, 'pipelines') +
                  (all ? '?project_scope=all' : ''),
                ref,
              )}
            >
              Все пайплайны
            </Link>
          </div>
          <ExecutionList key={workspaceKey(ref)} namespace={ref} preview />
        </>
      ) : (
        <p role="alert">Раздел не найден.</p>
      )}
    </div>
  )
}
