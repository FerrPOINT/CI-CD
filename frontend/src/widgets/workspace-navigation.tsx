import { useProjectDisclosures as useDisclosures } from '@pdlc/ui/lib'
import { NavLink, useLocation } from 'react-router'
import {
  FolderGit2,
  GitFork,
  LayoutDashboard,
  Play,
  RefreshCw,
  Rocket,
  Monitor,
} from 'lucide-react'
import {
  SidebarList,
  ProjectNavigationGroup,
  SidebarItemLabel,
  SidebarItem,
  Notice,
  LoadingState,
} from "@pdlc/ui/ui";
import { withNamespaceLocation } from '@pdlc/ui/lib'
import {
  useWorkspaceCatalog,
  workspaceKey,
  workspacePath,
  type Workspace,
} from '@/api/workspaces'
import { useNamespaceRef } from './namespace-context'

export const disclosureKey = (project: Workspace) =>
  `${project.tracker_instance_id}/${project.tracker_project_id}`
export function useProjectDisclosures() {
  return useDisclosures('forge:collapsed-projects:v1')
}
export function WorkspaceNavigation({
  collapsed,
  toggle,
  onNavigate,
  compact = false,
}: ReturnType<typeof useProjectDisclosures> & {
  onNavigate?: () => void
  compact?: boolean
}) {
  const catalog = useWorkspaceCatalog()
  const { ref, all, malformed } = useNamespaceRef()
  const location = useLocation()
  const sections = [
    ['', 'Обзор', LayoutDashboard],
    ['repositories', 'Репозитории', GitFork],
    ['syncs', 'Синхронизации', RefreshCw],
    ['pipelines', 'Пайплайны', Play],
    ['deployments', 'Деплои', Rocket],
    ['development-workspaces', 'Рабочие окружения', Monitor],
  ] as const
  if (malformed || catalog.isError)
    return (
      <Notice role="alert" tone="warning" appearance="inline">
        Проекты недоступны
      </Notice>
    )
  if (catalog.isPending)
    return (
      <LoadingState message="Загружаем проекты…" />
    )
  const projects = catalog.data.filter(
    (project) => all || (ref && workspaceKey(project) === workspaceKey(ref)),
  )
  return (
    <SidebarList asChild separated className="mt-4">
      <nav aria-label="Проекты">
        {projects.map((project) => (
          <ProjectNavigationGroup
            key={disclosureKey(project)}
            name={project.name}
            projectKey={project.project_key}
            compact={compact}
            open={!collapsed.includes(disclosureKey(project))}
            status={
              !project.group_id
                ? 'Git не подключён'
                : project.state === 'archived'
                  ? 'В архиве'
                  : project.stale
                    ? 'Метаданные устарели'
                    : undefined
            }
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
                    <SidebarItemLabel>{label}</SidebarItemLabel>
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
    </SidebarList>
  )
}
