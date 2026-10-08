import { useLocation, useNavigate } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { NamespacePicker, usePlatformServices } from '@sdlc/ui/ui'
import { parseNamespaceLocation, withNamespaceLocation } from '@sdlc/ui/lib'
import type { components } from '@/api/schema'
import { api } from '@/api/client'
export type ResourceContext = components['schemas']['ResourceContextSummary']
async function get<T>(path: string, signal?: AbortSignal): Promise<T> {
  return api<T>(path.replace('/api/v1', ''), { signal })
}
export function useNamespaceContext() {
  const location = useLocation()
  const ref = parseNamespaceLocation(location.search)
  const malformed =
    !ref &&
    ['namespace_id', 'registry_instance_id'].some((key) =>
      new URLSearchParams(location.search).has(key),
    )
  const query = useQuery({
    queryKey: ['namespace-context', ref?.registry_instance_id, ref?.namespace_id],
    enabled: Boolean(ref),
    queryFn: ({ signal }) =>
      get<ResourceContext>(
        `/api/v1/namespace-contexts/${ref!.registry_instance_id}/${ref!.namespace_id}`,
        signal,
      ),
  })
  return { ref, malformed, query }
}
export function NamespaceShellContext() {
  const navigate = useNavigate()
  const { services } = usePlatformServices()
  const { ref, malformed, query } = useNamespaceContext()
  const catalog = useQuery({
    queryKey: ['namespace-contexts', 'ci-cd'],
    queryFn: ({ signal }) => get<ResourceContext[]>('/api/v1/namespace-contexts?limit=100', signal),
  })
  const items = [...(catalog.data ?? [])]
  if (
    query.data &&
    !items.some(
      (item) =>
        item.binding.namespace.namespace_id === query.data.binding.namespace.namespace_id &&
        item.binding.namespace.registry_instance_id ===
          query.data.binding.namespace.registry_instance_id,
    )
  )
    items.push(query.data)
  const admin = services.find((service) => service.key === 'admin-panel')?.ui_url
  const value = ref ? `${ref.registry_instance_id}/${ref.namespace_id}` : malformed ? 'invalid' : ''
  return (
    <NamespacePicker
      value={value}
      loading={catalog.isPending}
      unavailable={malformed || catalog.isError || Boolean(ref && query.isError)}
      options={items.map((item) => ({
        value: `${item.binding.namespace.registry_instance_id}/${item.binding.namespace.namespace_id}`,
        label: item.label,
      }))}
      manageUrl={
        admin
          ? withNamespaceLocation(
              admin + (ref ? `/namespaces/${ref.namespace_id}` : '/namespaces'),
              ref,
            )
          : '/namespace'
      }
      onChange={(next) => {
        const [registry_instance_id = '', namespace_id = ''] = next.split('/')
        navigate(
          withNamespaceLocation('/namespace', next ? { registry_instance_id, namespace_id } : null),
        )
      }}
    />
  )
}
