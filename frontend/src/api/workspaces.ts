import { useQuery } from '@tanstack/react-query'
import type { NamespaceLocation } from '@sdlc/ui/lib'
import type { components } from './schema'
import { api } from './client'

export type Workspace = components['schemas']['WorkspaceProject']
export type Configuration = components['schemas']['DeliveryConfig']
export const workspaceKey = (ref: NamespaceLocation) =>
  `${ref.registry_instance_id}/${ref.namespace_id}`
export const workspacePath = (ref: NamespaceLocation, section = '') =>
  `/workspaces/${workspaceKey(ref)}${section ? '/' + section : ''}`
export function useWorkspace(ref: NamespaceLocation | null) {
  return useQuery({
    queryKey: ['workspace', ref?.registry_instance_id, ref?.namespace_id],
    enabled: !!ref,
    queryFn: ({ signal }) =>
      api<Workspace>(`/workspace-projects/${workspaceKey(ref!)}`, { signal }),
  })
}
export function useWorkspaceCatalog() {
  return useQuery({
    queryKey: ['workspace-catalog'],
    refetchInterval: 60000,
    queryFn: async ({ signal }) => {
      const items: Workspace[] = []
      for (let offset = 0; ; offset += 100) {
        const page = await api<components['schemas']['WorkspacePage']>(
          `/workspace-projects?limit=100&offset=${offset}`,
          { signal },
        )
        items.push(...page.items)
        if (items.length >= page.total) return items
        if (!page.items.length) throw new Error('Каталог изменился. Обновите страницу.')
      }
    },
  })
}
export function useConfigurations(repositoryId?: string, offset = 0, search = '') {
  return useQuery({
    queryKey: ['delivery-configurations', repositoryId, offset, search],
    queryFn: ({ signal }) =>
      api<components['schemas']['ConfigPage']>(
        `${repositoryId ? '/catalog/repositories/' + repositoryId + '/delivery-configs' : '/delivery-configurations?unbound=true'}${repositoryId ? '?' : '&'}limit=50&offset=${offset}&search=${encodeURIComponent(search)}`,
        { signal },
      ),
  })
}
