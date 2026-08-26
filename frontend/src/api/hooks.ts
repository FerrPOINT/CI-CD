import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from './client'
import type { Project, Pipeline, PipelineDetail, Job, JobLog, Repository, Status } from './types'

const KEYS = {
  projects: ['projects'] as const,
  pipelines: (projectId: string) => ['pipelines', projectId] as const,
  pipeline: (id: string) => ['pipeline', id] as const,
  logs: (jobId: string) => ['logs', jobId] as const,
  repositories: ['repositories'] as const,
}

export function useProjects() {
  return useQuery({ queryKey: KEYS.projects, queryFn: () => api<Project[]>('/projects') })
}

export function useCreateProject() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: { name: string; repository_url: string; default_branch: string }) =>
      api<Project>('/projects', { method: 'POST', body: JSON.stringify(input) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: KEYS.projects }),
  })
}

export function useUpdateProject() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ id, ...input }: { id: string; name?: string; repository_url?: string; default_branch?: string }) =>
      api<Project>(`/projects/${id}`, { method: 'PATCH', body: JSON.stringify(input) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: KEYS.projects }),
  })
}

export function useDeleteProject() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => api<{ deleted: string }>(`/projects/${id}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: KEYS.projects }),
  })
}

export function usePipelines(projectId: string | undefined) {
  return useQuery({
    queryKey: KEYS.pipelines(projectId ?? ''),
    queryFn: () => api<Pipeline[]>(`/projects/${projectId}/pipelines`),
    enabled: !!projectId,
  })
}

export function useTriggerPipeline(projectId: string | undefined) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (gitRef: string) =>
      api<PipelineDetail>(`/projects/${projectId}/pipelines`, { method: 'POST', body: JSON.stringify({ git_ref: gitRef }) }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: KEYS.pipelines(projectId ?? '') })
    },
  })
}

export function usePipeline(id: string | undefined) {
  return useQuery({
    queryKey: KEYS.pipeline(id ?? ''),
    queryFn: () => api<PipelineDetail>(`/pipelines/${id}`),
    enabled: !!id,
  })
}

export function useUpdateJobStatus() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ jobId, status }: { jobId: string; status: Status }) =>
      api<Job>(`/jobs/${jobId}/status`, { method: 'POST', body: JSON.stringify({ status }) }),
    onSuccess: () => qc.invalidateQueries(),
  })
}

export function useJobLogs(jobId: string | undefined) {
  return useQuery({
    queryKey: KEYS.logs(jobId ?? ''),
    queryFn: () => api<JobLog[]>(`/jobs/${jobId}/logs`),
    enabled: !!jobId,
  })
}

export function useAppendLog() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ jobId, message }: { jobId: string; message: string }) =>
      api<JobLog>(`/jobs/${jobId}/logs`, { method: 'POST', body: JSON.stringify({ message }) }),
    onSuccess: (_data, vars) => qc.invalidateQueries({ queryKey: KEYS.logs(vars.jobId) }),
  })
}

export function useRepositories() {
  return useQuery({ queryKey: KEYS.repositories, queryFn: () => api<Repository[]>('/repositories') })
}

export function useCreateRepository() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: { name: string }) =>
      api<Repository>('/repositories', { method: 'POST', body: JSON.stringify(input) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: KEYS.repositories }),
  })
}

export function useDeleteRepository() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (name: string) => api<{ deleted: string }>(`/repositories/${name}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: KEYS.repositories }),
  })
}
