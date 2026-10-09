import type { ReactNode } from 'react'
import { useParams } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { NamespaceLink as Link, ProjectAvatar } from '@sdlc/ui/ui'
import { withNamespaceLocation } from '@sdlc/ui/lib'
import { api } from '@/api/client'
import { useWorkspace, workspaceKey, workspacePath, type Configuration } from '@/api/workspaces'
import { useNamespaceRef } from './namespace-context'
import type { components } from '@/api/schema'
export function ConfigurationContext({
  children,
  configurationId,
  showSettings = true,
}: {
  children: ReactNode
  configurationId?: string
  showSettings?: boolean
}) {
  const { projectId: routeId } = useParams()
  const projectId = configurationId ?? routeId
  const { ref, malformed } = useNamespaceRef()
  const config = useQuery({
    queryKey: ['delivery-config', ref?.registry_instance_id, ref?.namespace_id, projectId],
    queryFn: ({ signal }) =>
      api<Configuration>(`/delivery-configurations/${projectId}`, { signal }),
  })
  const repository = useQuery({
    queryKey: [
      'catalog-repository',
      ref?.registry_instance_id,
      ref?.namespace_id,
      config.data?.repository_id,
    ],
    enabled: !!config.data?.repository_id,
    queryFn: ({ signal }) =>
      api<components['schemas']['CatalogRepository']>(
        `/catalog/repositories/${config.data!.repository_id}`,
        { signal },
      ),
  })
  const namespace = repository.data?.namespace ?? null
  const project = useWorkspace(namespace)
  if (
    malformed ||
    (ref && repository.data && (!namespace || workspaceKey(ref) !== workspaceKey(namespace)))
  )
    return (
      <p role="alert" className="text-danger">
        Конфигурация принадлежит другому проекту.
      </p>
    )
  if (config.isError || repository.isError)
    return (
      <p role="alert" className="text-danger">
        Конфигурация недоступна.
      </p>
    )
  if (config.isPending || (config.data.repository_id && repository.isPending))
    return <p role="status">Загружаем конфигурацию…</p>
  return (
    <div className="space-y-4">
      <nav
        aria-label="Контекст CI-конфигурации"
        className="flex flex-wrap items-center gap-2 text-sm"
      >
        {project.data && (
          <>
            <ProjectAvatar projectKey={project.data.project_key} size="xs" />
            <Link
              className="text-accent"
              to={withNamespaceLocation(workspacePath(project.data), project.data)}
            >
              {project.data.name} · {project.data.project_key}
            </Link>
            <span>/</span>
          </>
        )}
        {repository.data ? (
          <>
            <Link
              className="text-accent"
              to={withNamespaceLocation(`/catalog/repositories/${repository.data.id}`, namespace)}
            >
              {repository.data.public_name}
            </Link>
            <span>/</span>
          </>
        ) : (
          <Link className="text-accent" to="/delivery-configs">
            Непривязанные конфигурации /
          </Link>
        )}
        <span>CI: {config.data.name}</span>
      </nav>
      {showSettings && (
        <div className="flex flex-wrap gap-3 text-sm">
          {[
            ['settings', 'Настройки'],
            ['pipelines', 'Запуски'],
            ['secrets', 'Secrets'],
            ['environments', 'Окружения'],
            ['schedules', 'Расписания'],
            ['webhooks', 'Hooks'],
            ['reports', 'Отчёты'],
          ].map(([section, label]) => (
            <Link
              key={section}
              className="text-accent"
              to={withNamespaceLocation(`/delivery-configs/${projectId}/${section}`, namespace)}
            >
              {label}
            </Link>
          ))}
        </div>
      )}
      {children}
    </div>
  )
}
