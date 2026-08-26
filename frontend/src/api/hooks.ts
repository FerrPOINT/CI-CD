import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, apiRetry } from './client'
import type {
  ApiToken,
  Artifact,
  AuditEvent,
  Commit,
  Comparison,
  CreatedApiToken,
  CreatePullRequestInput,
  Deployment,
  Environment,
  Job,
  JobLog,
  NotificationConfig,
  Pipeline,
  PipelineDetail,
  Project,
  ProjectReport,
  PullRequest,
  PullRequestAction,
  Repository,
  RepositoryRef,
  Runner,
  Schedule,
  SecretMetadata,
  Status,
  User,
  UserRole,
  Webhook,
} from './types'

const KEYS = {
  projects: ['projects'] as const,
  pipelines: (projectId: string) => ['pipelines', projectId] as const,
  pipeline: (id: string) => ['pipeline', id] as const,
  logs: (jobId: string) => ['logs', jobId] as const,
  repositories: ['repositories'] as const,
  refs: (repo: string) => ['repository-refs', repo] as const,
  commits: (repo: string, branch: string) => ['repository-commits', repo, branch] as const,
  comparison: (repo: string, from: string, to: string) => ['repository-comparison', repo, from, to] as const,
  pullRequests: (repo: string) => ['pull-requests', repo] as const,
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

export function useCancelPipeline() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (pipelineId: string) =>
      api<{ canceled: string }>(`/pipelines/${pipelineId}/cancel`, { method: 'POST' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['pipelines'] }),
  })
}

export function useRetryPipeline() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (pipelineId: string) =>
      api<{ retried: string }>(`/pipelines/${pipelineId}/retry`, { method: 'POST' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['pipelines'] }),
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

function repositoryPath(repo: string): string {
  return encodeURIComponent(repo)
}

export function useRepositoryRefs(repo: string | undefined) {
  return useQuery({
    queryKey: KEYS.refs(repo ?? ''),
    queryFn: () => api<RepositoryRef[]>(`/repos/${repositoryPath(repo ?? '')}/refs`),
    enabled: Boolean(repo),
    retry: apiRetry,
  })
}

export function useRepositoryCommits(repo: string | undefined, branch = 'main') {
  return useQuery({
    queryKey: KEYS.commits(repo ?? '', branch),
    queryFn: () => {
      const params = new URLSearchParams({ branch, limit: '50' })
      return api<Commit[]>(`/repos/${repositoryPath(repo ?? '')}/commits?${params}`)
    },
    enabled: Boolean(repo && branch),
    retry: apiRetry,
  })
}

export function useRepositoryComparison(repo: string | undefined, from: string, to: string) {
  return useQuery({
    queryKey: KEYS.comparison(repo ?? '', from, to),
    queryFn: () => {
      const params = new URLSearchParams({ from, to })
      return api<Comparison>(`/repos/${repositoryPath(repo ?? '')}/compare?${params}`)
    },
    enabled: Boolean(repo && from && to),
    retry: apiRetry,
  })
}

export function usePullRequests(repo: string | undefined) {
  return useQuery({
    queryKey: KEYS.pullRequests(repo ?? ''),
    queryFn: () => api<PullRequest[]>(`/repos/${repositoryPath(repo ?? '')}/pulls`),
    enabled: Boolean(repo),
    retry: apiRetry,
  })
}

export function useCreatePullRequest(repo: string | undefined) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: CreatePullRequestInput) =>
      api<PullRequest>(`/repos/${repositoryPath(repo ?? '')}/pulls`, {
        method: 'POST',
        body: JSON.stringify(input),
      }),
    onSuccess: () => qc.invalidateQueries({ queryKey: KEYS.pullRequests(repo ?? '') }),
  })
}

export function usePullRequestAction(repo: string | undefined) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ number, action }: { number: number; action: PullRequestAction }) =>
      api<PullRequest>(`/repos/${repositoryPath(repo ?? '')}/pulls/${number}/action`, {
        method: 'POST',
        body: JSON.stringify({ action }),
      }),
    onSuccess: () => qc.invalidateQueries({ queryKey: KEYS.pullRequests(repo ?? '') }),
  })
}

// --- Platform hooks ---

const PLATFORM_KEYS = {
  runners: ['runners'] as const,
  secrets: (projectId: string) => ['secrets', projectId] as const,
  artifacts: (jobId: string) => ['artifacts', jobId] as const,
  environments: (projectId: string) => ['environments', projectId] as const,
  deployments: (environmentId: string) => ['deployments', environmentId] as const,
  schedules: (projectId: string) => ['schedules', projectId] as const,
  webhooks: (projectId: string) => ['webhooks', projectId] as const,
  notifications: (projectId: string) => ['notifications', projectId] as const,
  report: (projectId: string) => ['report', projectId] as const,
  auditLog: ['audit-log'] as const,
  users: ['users'] as const,
  tokens: ['api-tokens'] as const,
}

export function useRunners() {
  return useQuery({ queryKey: PLATFORM_KEYS.runners, queryFn: () => api<Runner[]>('/runners') })
}

export function useRegisterRunner() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: { name: string; tags?: string[] }) =>
      api<Runner>('/runners', { method: 'POST', body: JSON.stringify(input) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: PLATFORM_KEYS.runners }),
  })
}

export function useRunnerHeartbeat() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ id, status }: { id: string; status?: 'online' | 'offline' | 'paused' }) =>
      api<Runner>(`/runners/${id}/heartbeat`, { method: 'POST', body: JSON.stringify({ status }) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: PLATFORM_KEYS.runners }),
  })
}

export function useDeleteRunner() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => api<{ deleted: string }>(`/runners/${id}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: PLATFORM_KEYS.runners }),
  })
}

export function useSecrets(projectId: string | undefined) {
  return useQuery({
    queryKey: PLATFORM_KEYS.secrets(projectId ?? ''),
    queryFn: () => api<SecretMetadata[]>(`/projects/${projectId}/secrets`),
    enabled: Boolean(projectId),
  })
}

export function useUpsertSecret(projectId: string | undefined) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: { key: string; value: string }) =>
      api<SecretMetadata>(`/projects/${projectId}/secrets`, { method: 'POST', body: JSON.stringify(input) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: PLATFORM_KEYS.secrets(projectId ?? '') }),
  })
}

export function useDeleteSecret() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => api<{ deleted: string }>(`/secrets/${id}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['secrets'] }),
  })
}

export function useArtifacts(jobId: string | undefined) {
  return useQuery({
    queryKey: PLATFORM_KEYS.artifacts(jobId ?? ''),
    queryFn: () => api<Artifact[]>(`/jobs/${jobId}/artifacts`),
    enabled: Boolean(jobId),
  })
}

export function useEnvironments(projectId: string | undefined) {
  return useQuery({
    queryKey: PLATFORM_KEYS.environments(projectId ?? ''),
    queryFn: () => api<Environment[]>(`/projects/${projectId}/environments`),
    enabled: Boolean(projectId),
  })
}

export function useCreateEnvironment(projectId: string | undefined) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: { name: string; url?: string }) =>
      api<Environment>(`/projects/${projectId}/environments`, { method: 'POST', body: JSON.stringify(input) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: PLATFORM_KEYS.environments(projectId ?? '') }),
  })
}

export function useDeleteEnvironment() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => api<{ deleted: string }>(`/environments/${id}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['environments'] }),
  })
}

export function useDeployments(environmentId: string | undefined) {
  return useQuery({
    queryKey: PLATFORM_KEYS.deployments(environmentId ?? ''),
    queryFn: () => api<Deployment[]>(`/environments/${environmentId}/deployments`),
    enabled: Boolean(environmentId),
  })
}

export function useCreateDeployment(environmentId: string | undefined) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: { git_ref: string; status?: 'pending' | 'running' | 'success' | 'failed' }) =>
      api<Deployment>(`/environments/${environmentId}/deployments`, { method: 'POST', body: JSON.stringify(input) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: PLATFORM_KEYS.deployments(environmentId ?? '') }),
  })
}

export function useSchedules(projectId: string | undefined) {
  return useQuery({
    queryKey: PLATFORM_KEYS.schedules(projectId ?? ''),
    queryFn: () => api<Schedule[]>(`/projects/${projectId}/schedules`),
    enabled: Boolean(projectId),
  })
}

export function useCreateSchedule(projectId: string | undefined) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: { cron: string; git_ref: string; enabled?: boolean }) =>
      api<Schedule>(`/projects/${projectId}/schedules`, { method: 'POST', body: JSON.stringify(input) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: PLATFORM_KEYS.schedules(projectId ?? '') }),
  })
}

export function useUpdateSchedule() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ id, ...input }: { id: string; cron: string; git_ref: string; enabled?: boolean }) =>
      api<Schedule>(`/schedules/${id}`, { method: 'PATCH', body: JSON.stringify(input) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['schedules'] }),
  })
}

export function useDeleteSchedule() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => api<{ deleted: string }>(`/schedules/${id}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['schedules'] }),
  })
}

export function useWebhooks(projectId: string | undefined) {
  return useQuery({
    queryKey: PLATFORM_KEYS.webhooks(projectId ?? ''),
    queryFn: () => api<Webhook[]>(`/projects/${projectId}/webhooks`),
    enabled: Boolean(projectId),
  })
}

export function useCreateWebhook(projectId: string | undefined) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: { url: string; events?: string[]; enabled?: boolean }) =>
      api<Webhook>(`/projects/${projectId}/webhooks`, { method: 'POST', body: JSON.stringify(input) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: PLATFORM_KEYS.webhooks(projectId ?? '') }),
  })
}

export function useDeleteWebhook() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => api<{ deleted: string }>(`/webhooks/${id}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['webhooks'] }),
  })
}

export function useNotifications(projectId: string | undefined) {
  return useQuery({
    queryKey: PLATFORM_KEYS.notifications(projectId ?? ''),
    queryFn: () => api<NotificationConfig[]>(`/projects/${projectId}/notifications`),
    enabled: Boolean(projectId),
  })
}

export function useSaveNotifications(projectId: string | undefined) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (inputs: { channel: string; target: string; enabled?: boolean }[]) =>
      api<NotificationConfig[]>(`/projects/${projectId}/notifications`, { method: 'PUT', body: JSON.stringify(inputs) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: PLATFORM_KEYS.notifications(projectId ?? '') }),
  })
}

export function useProjectReport(projectId: string | undefined) {
  return useQuery({
    queryKey: PLATFORM_KEYS.report(projectId ?? ''),
    queryFn: () => api<ProjectReport>(`/projects/${projectId}/reports/summary`),
    enabled: Boolean(projectId),
  })
}

export function useAuditLog() {
  return useQuery({ queryKey: PLATFORM_KEYS.auditLog, queryFn: () => api<AuditEvent[]>('/audit-log') })
}

export function useUsers() {
  return useQuery({ queryKey: PLATFORM_KEYS.users, queryFn: () => api<User[]>('/users') })
}

export function useCreateUser() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: { username: string; role: UserRole; enabled?: boolean }) =>
      api<User>('/users', { method: 'POST', body: JSON.stringify(input) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: PLATFORM_KEYS.users }),
  })
}

export function useUpdateUser() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ id, ...input }: { id: string; username: string; role: UserRole; enabled?: boolean }) =>
      api<User>(`/users/${id}`, { method: 'PATCH', body: JSON.stringify(input) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: PLATFORM_KEYS.users }),
  })
}

export function useApiTokens() {
  return useQuery({ queryKey: PLATFORM_KEYS.tokens, queryFn: () => api<ApiToken[]>('/api-tokens') })
}

export function useCreateApiToken() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: { name: string; user_id?: string }) =>
      api<CreatedApiToken>('/api-tokens', { method: 'POST', body: JSON.stringify(input) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: PLATFORM_KEYS.tokens }),
  })
}

export function useDeleteApiToken() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => api<{ deleted: string }>(`/api-tokens/${id}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: PLATFORM_KEYS.tokens }),
  })
}
