import { keepPreviousData, useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useRef } from 'react'
import { api, apiRetry, authenticatedFetch } from './client'
import type {
  TreeEntry,
  BlobContent,
  TagInfo,
  Release,
  TestReport,

  ApiToken,
  Artifact,
  AuditLogPage,
  Commit,
  Comparison,
  CreateApiTokenInput,
  CreatedApiToken,
  CreatePullRequestInput,
  Deployment,
  DeploymentApproval,
  Environment,
  Job,
  JobAttempt,
  JobLog,
  JobLogPage,
  NotificationConfig,
  NotificationInput,
  NotificationEvent,
  OutboxDeliveryPage,
  OutboxDeliveryDetail,
  Pipeline,
  PipelineDetail,
  Project,
  ProjectReport,
  PullRequest,
  PullRequestAction,
  PullRequestPage,
  PullRequestStatus,
  RequeuedOutboxDelivery,
  Repository,
  RepositoryRef,
  Runner,
  Schedule,
  SecretMetadata,
  Status,
  User,
  UserInput,
  Webhook,
} from './types'

const KEYS = {
  projects: ['projects'] as const,
  pipelines: (projectId: string) => ['pipelines', projectId] as const,
  pipeline: (id: string) => ['pipeline', id] as const,
  logs: (jobId: string) => ['logs', jobId] as const,
  attempts: (jobId: string) => ['attempts', jobId] as const,
  attemptLogs: (jobId: string, attemptId: string) => ['logs', jobId, attemptId] as const,
  attemptLogPages: (jobId: string, attemptId: string, search: string) => ['logs', jobId, attemptId, 'page', search] as const,
  repositories: ['repositories'] as const,
  refs: (repo: string) => ['repository-refs', repo] as const,
  commits: (repo: string, branch: string) => ['repository-commits', repo, branch] as const,
  comparison: (repo: string, from: string, to: string) => ['repository-comparison', repo, from, to] as const,
  pullRequests: (repo: string) => ['pull-requests', repo] as const,
  pullRequestList: (repo: string, limit: number, offset: number, status: string, search: string) =>
    [...KEYS.pullRequests(repo), 'list', limit, offset, status, search] as const,
  pullRequest: (repo: string, number: number) => [...KEYS.pullRequests(repo), 'detail', number] as const,
  repositoryTree: (repo: string, gitRef: string, path: string) => ['repository-tree', repo, gitRef, path] as const,
  repositoryBlob: (repo: string, gitRef: string, path: string) => ['repository-blob', repo, gitRef, path] as const,
  repositoryTags: (repo: string) => ['repository-tags', repo] as const,
  releases: (repo: string) => ['releases', repo] as const,
  testReport: (jobId: string) => ['test-report', jobId] as const,
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

export function usePipelines(projectId: string | undefined, page = 0, pageSize = 20) {
  return useQuery({
    queryKey: [...KEYS.pipelines(projectId ?? ''), page, pageSize],
    queryFn: () => api<Pipeline[]>(`/projects/${projectId}/pipelines?limit=${pageSize + 1}&offset=${page * pageSize}`),
    enabled: !!projectId,
    refetchInterval: (query) =>
      query.state.data?.some((pipeline) => pipeline.status === 'queued' || pipeline.status === 'running')
        ? 3000
        : false,
  })
}

export function useCancelPipeline() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (pipelineId: string) =>
      api<{ canceled: string }>(`/pipelines/${pipelineId}/cancel`, { method: 'POST' }),
    onSuccess: (_result, pipelineId) => {
      qc.invalidateQueries({ queryKey: ['pipelines'] })
      qc.invalidateQueries({ queryKey: KEYS.pipeline(pipelineId) })
    },
  })
}

export function useRetryPipeline() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (pipelineId: string) =>
      api<{ retried: string }>(`/pipelines/${pipelineId}/retry`, { method: 'POST' }),
    onSuccess: (_result, pipelineId) => {
      qc.invalidateQueries({ queryKey: ['pipelines'] })
      qc.invalidateQueries({ queryKey: KEYS.pipeline(pipelineId) })
    },
  })
}

export function useTriggerPipeline(projectId: string | undefined) {
  const qc = useQueryClient()
  const idempotencyKeyRef = useRef<string | null>(null)
  return useMutation({
    mutationFn: (gitRef: string) => {
      idempotencyKeyRef.current ??= crypto.randomUUID()
      return api<PipelineDetail>(`/projects/${projectId}/pipelines`, {
        method: 'POST',
        headers: { 'Idempotency-Key': idempotencyKeyRef.current },
        body: JSON.stringify({ git_ref: gitRef }),
      })
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: KEYS.pipelines(projectId ?? '') })
    },
    onSettled: () => {
      idempotencyKeyRef.current = null
    },
  })
}

export function usePipeline(id: string | undefined) {
  return useQuery({
    queryKey: KEYS.pipeline(id ?? ''),
    queryFn: () => api<PipelineDetail>(`/pipelines/${id}`),
    enabled: !!id,
    refetchInterval: (query) =>
      query.state.data?.pipeline.status === 'queued' || query.state.data?.pipeline.status === 'running'
        ? 3000
        : false,
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

export function useJobAttempts(jobId: string | undefined, live = false) {
  return useQuery({
    queryKey: KEYS.attempts(jobId ?? ''),
    queryFn: () => api<JobAttempt[]>(`/jobs/${jobId}/attempts`),
    enabled: !!jobId,
    refetchInterval: live ? 3000 : false,
  })
}

export function useJobLogs(jobId: string | undefined, attemptId?: string) {
  return useQuery({
    queryKey: attemptId ? KEYS.attemptLogs(jobId ?? '', attemptId) : KEYS.logs(jobId ?? ''),
    queryFn: () =>
      attemptId
        ? api<JobLog[]>(`/jobs/${jobId}/attempts/${attemptId}/logs`)
        : api<JobLog[]>(`/jobs/${jobId}/logs`),
    enabled: !!jobId,
  })
}

export function useJobLogPages(jobId: string | undefined, attemptId: string | undefined, search = '', live = false) {
  const q = search.trim()
  return useInfiniteQuery({
    queryKey: KEYS.attemptLogPages(jobId ?? '', attemptId ?? '', q),
    initialPageParam: 0,
    queryFn: ({ pageParam }) => {
      const params = new URLSearchParams({ limit: '200', after: String(pageParam) })
      if (q) params.set('q', q)
      return api<JobLogPage>(`/jobs/${jobId}/attempts/${attemptId}/logs/page?${params.toString()}`)
    },
    getNextPageParam: (lastPage) => lastPage.next_after ?? undefined,
    enabled: !!jobId && !!attemptId,
    refetchInterval: live ? 5000 : false,
  })
}

export function useAppendLog() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ jobId, message }: { jobId: string; message: string }) =>
      api<JobLog>(`/jobs/${jobId}/logs`, { method: 'POST', body: JSON.stringify({ message }) }),
    onSuccess: (_data, vars) => {
      qc.invalidateQueries({ queryKey: KEYS.logs(vars.jobId) })
      qc.invalidateQueries({ queryKey: KEYS.attempts(vars.jobId) })
    },
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

export function useRepositoryCommits(repo: string | undefined, branch = 'HEAD') {
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

export function usePullRequests(
  repo: string | undefined,
  options: { limit?: number; offset?: number; status?: PullRequestStatus; search?: string } = {},
) {
  const limit = options.limit ?? 20
  const offset = options.offset ?? 0
  const status = options.status ?? ''
  const search = options.search?.trim() ?? ''
  return useQuery({
    queryKey: KEYS.pullRequestList(repo ?? '', limit, offset, status, search),
    queryFn: () => {
      const params = new URLSearchParams({ limit: String(limit), offset: String(offset) })
      if (status) params.set('status', status)
      if (search) params.set('search', search)
      return api<PullRequestPage>(`/repos/${repositoryPath(repo ?? '')}/pulls/page?${params}`)
    },
    enabled: Boolean(repo),
    retry: apiRetry,
    placeholderData: keepPreviousData,
  })
}

export function usePullRequest(repo: string | undefined, number: number | undefined) {
  return useQuery({
    queryKey: KEYS.pullRequest(repo ?? '', number ?? 0),
    queryFn: () => api<PullRequest>(`/repos/${repositoryPath(repo ?? '')}/pulls/${number}`),
    enabled: Boolean(repo && number),
    retry: apiRetry,
  })
}

export function useRepositoryTree(repo: string | undefined, gitRef: string | undefined, path: string | undefined) {
  return useQuery({
    queryKey: KEYS.repositoryTree(repo ?? '', gitRef ?? '', path ?? ''),
    queryFn: () =>
      api<TreeEntry[]>(`/repos/${repositoryPath(repo ?? '')}/tree?${new URLSearchParams({
        ...(gitRef ? { ref: gitRef } : {}),
        ...(path ? { path } : {}),
      })}`),
    enabled: !!repo,
  })
}

export function useRepositoryBlob(repo: string | undefined, gitRef: string | undefined, path: string) {
  return useQuery({
    queryKey: KEYS.repositoryBlob(repo ?? '', gitRef ?? '', path),
    queryFn: () =>
      api<BlobContent>(`/repos/${repositoryPath(repo ?? '')}/blob?${new URLSearchParams({
        ...(gitRef ? { ref: gitRef } : {}),
        path,
      })}`),
    enabled: !!repo && !!path,
  })
}

export function useRepositoryTags(repo: string | undefined) {
  return useQuery({
    queryKey: KEYS.repositoryTags(repo ?? ''),
    queryFn: () => api<TagInfo[]>(`/repos/${repositoryPath(repo ?? '')}/tags`),
    enabled: !!repo,
  })
}

export function useReleases(repo: string | undefined) {
  return useQuery({
    queryKey: KEYS.releases(repo ?? ''),
    queryFn: () => api<Release[]>(`/repos/${repositoryPath(repo ?? '')}/releases`),
    enabled: !!repo,
  })
}

export function useCreateRelease(repo: string | undefined) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: { tag_name: string; name: string; description?: string; prerelease?: boolean }) =>
      api<Release>(`/repos/${repositoryPath(repo ?? '')}/releases`, { method: 'POST', body: JSON.stringify(input) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: KEYS.releases(repo ?? '') }),
  })
}

export function useDeleteRelease(repo: string | undefined) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (tag: string) =>
      api<{ deleted: string }>(`/repos/${repositoryPath(repo ?? '')}/releases/${encodeURIComponent(tag)}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: KEYS.releases(repo ?? '') }),
  })
}

export function useTestReport(jobId: string | undefined) {
  return useQuery({
    queryKey: KEYS.testReport(jobId ?? ''),
    queryFn: () => api<TestReport[]>(`/jobs/${jobId}/test-report`),
    enabled: !!jobId,
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
  deploymentApprovals: (deploymentId: string) => ['deployment-approvals', deploymentId] as const,
  schedules: (projectId: string) => ['schedules', projectId] as const,
  webhooks: (projectId: string) => ['webhooks', projectId] as const,
  outboxDeliveries: (projectId: string) => ['outbox-deliveries', projectId] as const,
  outboxDelivery: (deliveryId: string) => ['outbox-delivery', deliveryId] as const,
  notifications: (projectId: string) => ['notifications', projectId] as const,
  notificationEvents: (projectId: string) => ['notification-events', projectId] as const,
  report: (projectId: string) => ['report', projectId] as const,
  auditLog: ['audit-log'] as const,
  users: ['users'] as const,
  tokens: ['api-tokens'] as const,
}

function browserNotificationStreamEnabled(): boolean {
  return typeof window !== 'undefined' && !window.navigator.userAgent.toLowerCase().includes('jsdom')
}

export function useRunners() {
  return useQuery({ queryKey: PLATFORM_KEYS.runners, queryFn: () => api<Runner[]>('/runners'), refetchInterval: 30_000 })
}

export function useRegisterRunner() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: { name: string; tags?: string[] }) =>
      api<Runner>('/runners', { method: 'POST', body: JSON.stringify(input) }),
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
    mutationFn: (input: { name: string; url?: string; protected?: boolean; required_approvals?: number }) =>
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

export function useDeploymentApprovals(deploymentId: string | undefined) {
  return useQuery({
    queryKey: PLATFORM_KEYS.deploymentApprovals(deploymentId ?? ''),
    queryFn: () => api<DeploymentApproval[]>(`/deployments/${deploymentId}/approvals`),
    enabled: Boolean(deploymentId),
  })
}

export function useRecordDeploymentApproval(environmentId: string | undefined) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: { deploymentId: string; decision: 'approved' | 'rejected'; actor?: string; comment?: string }) =>
      api<Deployment>(`/deployments/${input.deploymentId}/approvals`, {
        method: 'POST',
        body: JSON.stringify({ decision: input.decision, actor: input.actor, comment: input.comment }),
      }),
    onSuccess: (deployment) => {
      qc.invalidateQueries({ queryKey: PLATFORM_KEYS.deployments(environmentId ?? deployment.environment_id) })
      qc.invalidateQueries({ queryKey: PLATFORM_KEYS.deploymentApprovals(deployment.id) })
    },
  })
}

export function useRollbackDeployment(environmentId: string | undefined) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: { deploymentId: string; git_ref?: string }) =>
      api<Deployment>(`/deployments/${input.deploymentId}/rollback`, {
        method: 'POST',
        body: JSON.stringify({ git_ref: input.git_ref }),
      }),
    onSuccess: (deployment) => {
      qc.invalidateQueries({ queryKey: PLATFORM_KEYS.deployments(environmentId ?? deployment.environment_id) })
    },
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

export function useOutboxDeliveries(projectId: string | undefined, filters?: { status?: string; channel?: string; limit?: number; offset?: number }) {
  return useQuery({
    queryKey: [
      ...PLATFORM_KEYS.outboxDeliveries(projectId ?? ''),
      filters?.status ?? 'all',
      filters?.channel ?? 'all',
      filters?.limit ?? 20,
      filters?.offset ?? 0,
    ],
    queryFn: () => {
      const params = new URLSearchParams({
        limit: String(filters?.limit ?? 20),
        offset: String(filters?.offset ?? 0),
      })
      if (filters?.status) params.set('status', filters.status)
      if (filters?.channel) params.set('channel', filters.channel)
      return api<OutboxDeliveryPage>(`/projects/${projectId}/outbox-deliveries/page?${params.toString()}`)
    },
    enabled: Boolean(projectId),
    placeholderData: keepPreviousData,
    refetchInterval: browserNotificationStreamEnabled() ? 10_000 : false,
  })
}

export function useOutboxDelivery(deliveryId: string | null | undefined) {
  return useQuery({
    queryKey: PLATFORM_KEYS.outboxDelivery(deliveryId ?? ''),
    queryFn: () => api<OutboxDeliveryDetail>(`/outbox-deliveries/${deliveryId}`),
    enabled: Boolean(deliveryId),
  })
}

export function useRequeueOutboxDelivery() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => api<RequeuedOutboxDelivery>(`/outbox-deliveries/${id}/requeue`, { method: 'POST' }),
    onSuccess: (_result, id) => {
      void qc.invalidateQueries({ queryKey: ['outbox-deliveries'] })
      void qc.invalidateQueries({ queryKey: PLATFORM_KEYS.outboxDelivery(id) })
    },
  })
}

export function useNotifications(projectId: string | undefined) {
  return useQuery({
    queryKey: PLATFORM_KEYS.notifications(projectId ?? ''),
    queryFn: () => api<NotificationConfig[]>(`/projects/${projectId}/notifications`),
    enabled: Boolean(projectId),
  })
}

export function useNotificationEvents(projectId: string | undefined) {
  const qc = useQueryClient()
  const streamEnabled = browserNotificationStreamEnabled()
  const query = useQuery({
    queryKey: PLATFORM_KEYS.notificationEvents(projectId ?? ''),
    queryFn: () => api<NotificationEvent[]>(`/projects/${projectId}/notification-events?limit=20`),
    enabled: Boolean(projectId),
    refetchInterval: streamEnabled ? 10_000 : false,
  })
  useEffect(() => {
    if (!projectId || !browserNotificationStreamEnabled()) return
    const streamProjectId = projectId
    const controller = new AbortController()
    let stopped = false
    let retryTimer: number | undefined

    async function connect() {
      while (!stopped) {
        try {
          const response = await authenticatedFetch(`/projects/${streamProjectId}/notifications/stream`, {
            signal: controller.signal,
          })
          if (!response.ok || !response.body) throw new Error('notification stream unavailable')
          const reader = response.body.getReader()
          const decoder = new TextDecoder()
          let buffer = ''
          while (!stopped) {
            const { done, value } = await reader.read()
            if (done) break
            buffer += decoder.decode(value, { stream: true }).replace(/\r\n/g, '\n')
            let frameEnd = buffer.indexOf('\n\n')
            while (frameEnd >= 0) {
              const frame = buffer.slice(0, frameEnd)
              buffer = buffer.slice(frameEnd + 2)
              if (frame.includes('event: notification')) {
                void qc.invalidateQueries({ queryKey: PLATFORM_KEYS.notificationEvents(streamProjectId) })
              }
              frameEnd = buffer.indexOf('\n\n')
            }
          }
        } catch {
          if (stopped || controller.signal.aborted) return
        }
        if (!stopped) {
          await new Promise<void>((resolve) => {
            retryTimer = window.setTimeout(resolve, 5000)
          })
        }
      }
    }

    void connect()
    return () => {
      stopped = true
      controller.abort()
      if (retryTimer) window.clearTimeout(retryTimer)
    }
  }, [projectId, qc])

  return query
}

export function useSaveNotifications(projectId: string | undefined) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (inputs: NotificationInput[]) =>
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

export function useAuditLog(filters?: { action?: string; q?: string; limit?: number; offset?: number }) {
  return useQuery({
    queryKey: [
      ...PLATFORM_KEYS.auditLog,
      filters?.action ?? 'all',
      filters?.q ?? '',
      filters?.limit ?? 20,
      filters?.offset ?? 0,
    ],
    queryFn: () => {
      const params = new URLSearchParams({
        limit: String(filters?.limit ?? 20),
        offset: String(filters?.offset ?? 0),
      })
      if (filters?.action) params.set('action', filters.action)
      if (filters?.q) params.set('q', filters.q)
      return api<AuditLogPage>(`/audit-log/page?${params.toString()}`)
    },
    placeholderData: keepPreviousData,
  })
}

export function useUsers() {
  return useQuery({ queryKey: PLATFORM_KEYS.users, queryFn: () => api<User[]>('/users') })
}

export function useCreateUser() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: UserInput) =>
      api<User>('/users', { method: 'POST', body: JSON.stringify(input) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: PLATFORM_KEYS.users }),
  })
}

export function useUpdateUser() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ id, ...input }: { id: string } & UserInput) =>
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
    mutationFn: (input: CreateApiTokenInput) =>
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
