import { useEffect, useState } from 'react'
import { NavLink, useLocation } from 'react-router'
import { FolderGit2, GitFork, LayoutDashboard, Play, Rocket } from 'lucide-react'
import { ProjectNavigationGroup, SidebarItem } from '@sdlc/ui/ui'
import { withNamespaceLocation } from '@sdlc/ui/lib'
import { useWorkspaceCatalog, workspaceKey, workspacePath, type Workspace } from '@/api/workspaces'
import { useNamespaceRef } from './namespace-context'

export const disclosureKey = (project: Workspace) =>
  `${project.tracker_instance_id}/${project.tracker_project_id}`
export function useProjectDisclosures() {
  const [collapsed, setCollapsed] = useState<string[]>(() => {
    try {
      const value: unknown = JSON.parse(localStorage.getItem('forge:collapsed-projects:v1') ?? '[]')
      return Array.isArray(value)
        ? value.filter((item): item is string => typeof item === 'string')
        : []
    } catch {
      return []
    }
  })
  useEffect(() => {
    try {
      localStorage.setItem('forge:collapsed-projects:v1', JSON.stringify(collapsed))
    } catch {
      /* Navigation still works when browser storage is unavailable. */
    }
  }, [collapsed])
  return {
    collapsed,
    toggle: (key: string) =>
      setCollapsed((current) =>
        current.includes(key) ? current.filter((item) => item !== key) : [...current, key],
      ),
  }
}
export function WorkspaceNavigation({
  collapsed,
  toggle,
  onNavigate,
  compact = false,
}: ReturnType<typeof useProjectDisclosures> & { onNavigate?: () => void; compact?: boolean }) {
  const catalog = useWorkspaceCatalog()
  const { ref, all, malformed } = useNamespaceRef()
  const location = useLocation()
  const sections = [
    ['', 'Обзор', LayoutDashboard],
    ['repositories', 'Репозитории', GitFork],
    ['pipelines', 'Пайплайны', Play],
    ['deployments', 'Деплои', Rocket],
  ] as const
  if (malformed || catalog.isError)
    return (
      <p role="alert" className="p-3 text-sm text-warning">
        Проекты недоступны
      </p>
    )
  if (catalog.isPending)
    return (
      <p role="status" className="p-3 text-sm text-text-muted">
        Загружаем проекты…
      </p>
    )
  const projects = catalog.data.filter(
    (project) => all || (ref && workspaceKey(project) === workspaceKey(ref)),
  )
  return (
    <nav className="mt-4 space-y-1 border-t border-border pt-3" aria-label="Проекты">
      {projects.map((project) => (
        <ProjectNavigationGroup
          key={disclosureKey(project)}
          name={project.name}
          projectKey={project.project_key}
          compact={compact}
          open={!collapsed.includes(disclosureKey(project))}
          active={!!ref && workspaceKey(ref) === workspaceKey(project)}
          onToggle={() => toggle(disclosureKey(project))}
        >
          {sections.map(([section, label, Icon]) => {
            const path = workspacePath(project, section)
            const to = withNamespaceLocation(path + (all ? '?project_scope=all' : ''), project)
            return (
              <SidebarItem
                key={section}
                asChild
                compact={compact}
                active={location.pathname === path}
              >
                <NavLink to={to} end onClick={onNavigate} title={label} aria-label={label}>
                  <Icon aria-hidden />
                  <span className="base-sidebar-item-label">{label}</span>
                </NavLink>
              </SidebarItem>
            )
          })}
        </ProjectNavigationGroup>
      ))}
      {!projects.length && (
        <span className="flex gap-2 p-3 text-sm text-text-muted">
          <FolderGit2 size={16} />
          Нет подключённых проектов
        </span>
      )}
    </nav>
  )
}
