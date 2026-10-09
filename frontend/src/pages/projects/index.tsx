import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Button, Input, ProjectAvatar, NamespaceLink as Link } from '@sdlc/ui/ui'
import { withNamespaceLocation } from '@sdlc/ui/lib'
import { api } from '@/api/client'
import type { components } from '@/api/schema'
import { workspacePath } from '@/api/workspaces'
import { WorkspaceLinks } from '@/widgets/workspace-links'
export function ProjectsPage() {
  const [search, setSearch] = useState('')
  const [offset, setOffset] = useState(0)
  const catalog = useQuery({
    queryKey: ['workspace-projects', search, offset],
    queryFn: ({ signal }) =>
      api<components['schemas']['WorkspacePage']>(
        `/workspace-projects?limit=50&offset=${offset}&search=${encodeURIComponent(search)}`,
        { signal },
      ),
  })
  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-2xl font-semibold">Проекты</h1>
        <WorkspaceLinks />
      </div>
      <Input
        aria-label="Поиск проектов"
        placeholder="Поиск по имени или ключу…"
        value={search}
        onChange={(event) => {
          setSearch(event.target.value)
          setOffset(0)
        }}
        className="max-w-md"
      />
      {catalog.isPending ? (
        <p role="status">Загружаем проекты…</p>
      ) : catalog.isError ? (
        <p role="alert" className="text-danger">
          Каталог проектов недоступен: {catalog.error.message}
        </p>
      ) : (
        <>
          <ul className="divide-y divide-border rounded-lg border border-border bg-surface">
            {catalog.data.items.map((project) => (
              <li key={`${project.registry_instance_id}/${project.namespace_id}`}>
                <Link
                  className="flex flex-wrap items-center gap-3 p-4 hover:bg-surface-raised"
                  to={withNamespaceLocation(workspacePath(project) + '?project_scope=all', project)}
                >
                  <ProjectAvatar projectKey={project.project_key} size="md" />
                  <div className="min-w-0 flex-1">
                    <span className="font-semibold">{project.name}</span>
                    <p className="text-sm text-text-muted">
                      {project.project_key} ·{' '}
                      {project.state === 'archived' ? 'В архиве' : 'Активен'}
                      {project.stale ? ' · Метаданные устарели' : ''}
                    </p>
                  </div>
                  <span className="text-sm text-text-muted">
                    {project.group_id
                      ? `${project.repositories} репозиториев · Git-группа ${project.group_slug}`
                      : 'Git не подключён'}
                  </span>
                  <span className="text-sm">{project.latest_status ?? 'Нет запусков'}</span>
                </Link>
              </li>
            ))}
          </ul>
          {!catalog.data.total && (
            <p className="text-text-muted">
              Подключённых проектов пока нет. Создайте проект или подключите существующий через
              Admin.
            </p>
          )}
          <p className="text-sm text-text-muted">Всего: {catalog.data.total}</p>
        </>
      )}
      <div className="flex gap-2">
        <Button
          variant="outline"
          disabled={!offset}
          onClick={() => setOffset(Math.max(0, offset - 50))}
        >
          Назад
        </Button>
        <Button
          variant="outline"
          disabled={!catalog.data || offset + 50 >= catalog.data.total}
          onClick={() => setOffset(offset + 50)}
        >
          Далее
        </Button>
      </div>
    </div>
  )
}
