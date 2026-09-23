import type { Pipeline, Project } from '@/api/types'
import { useQueries } from '@tanstack/react-query'
import { api } from '@/api/client'

// Keep one useQueries call while the visible project list changes.
export function useProjectPipelines(projects: Project[]) {
  const lists = useQueries({
    queries: projects.map(p => ({
      queryKey: ['pipelines', p.id] as const,
      queryFn: () => api<Pipeline[]>(`/projects/${p.id}/pipelines`),
    })),
  })
  return {
    runs: lists.flatMap(q => q.data ?? []),
    isLoading: lists.some(q => q.isPending),
    error: lists.find(q => q.error)?.error ?? null,
    failedCount: lists.filter(q => q.error).length,
    hasData: lists.some(q => q.data !== undefined),
    refetch: () => Promise.all(lists.map(q => q.refetch())),
  }
}
