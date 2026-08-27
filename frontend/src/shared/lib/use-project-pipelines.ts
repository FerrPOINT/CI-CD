import type { Pipeline, Project } from '@/api/types'
import { useQueries } from '@tanstack/react-query'
import { api } from '@/api/client'

/// Aggregates recent pipelines across the given projects with a stable hooks
/// call count (one useQueries regardless of project list length).
export function useProjectPipelines(projects: Project[]): Pipeline[][] {
  const lists = useQueries({
    queries: projects.map(p => ({
      queryKey: ['pipelines', p.id] as const,
      queryFn: () => api<Pipeline[]>(`/projects/${p.id}/pipelines`),
    })),
  })
  return lists.map(q => q.data ?? [])
}
