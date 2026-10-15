import { useLocation, useMatch, useNavigate } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { NamespacePicker } from '@pdlc/ui/ui'
import {
  projectNavigationScope,
  parseNamespaceLocation,
  withNamespaceLocation,
} from '@pdlc/ui/lib'
import type { components } from '@/api/schema'
import { api } from '@/api/client'
import {
  useWorkspaceCatalog,
  workspaceKey,
  workspacePath,
} from '@/api/workspaces'
export type ResourceContext = components['schemas']['ResourceContextSummary']
export function useNamespaceRef() {
  const location = useLocation()
  const match = useMatch('/workspaces/:registry/:namespace/*')
  const pathRef = match
    ? parseNamespaceLocation(
        `?registry_instance_id=${match.params.registry}&namespace_id=${match.params.namespace}`,
      )
    : null
  const scope = projectNavigationScope(location.search, pathRef)
  return { ...scope, malformed: scope.malformed || Boolean(match && !pathRef) }
}
export function useNamespaceContext() {
  const { ref, malformed } = useNamespaceRef()
  const query = useQuery({
    queryKey: [
      'namespace-context',
      ref?.registry_instance_id,
      ref?.namespace_id,
    ],
    enabled: !!ref && !malformed,
    queryFn: ({ signal }) =>
      api<ResourceContext>(`/namespace-contexts/${workspaceKey(ref!)}`, {
        signal,
      }),
  })
  return { ref, malformed, query }
}
export function NamespaceShellContext() {
  const navigate = useNavigate()
  const { ref, all } = useNamespaceRef()
  const catalog = useWorkspaceCatalog()
  return (
    <NamespacePicker
      projectIcons
      value={all ? '' : ref ? workspaceKey(ref) : 'invalid'}
      loading={catalog.isPending}
      unavailable={catalog.isError}
      options={(catalog.data ?? []).map((item) => ({
        value: workspaceKey(item),
        label: `${item.name} · ${item.project_key}`,
        projectKey: item.project_key,
      }))}
      onChange={(next) => {
        const [registry_instance_id = '', namespace_id = ''] = next.split('/')
        navigate(
          next
            ? withNamespaceLocation(
                workspacePath({ registry_instance_id, namespace_id }),
                {
                  registry_instance_id,
                  namespace_id,
                },
              )
            : '/projects?project_scope=all',
        )
      }}
    />
  )
}
