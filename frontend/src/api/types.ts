// Re-export the generated OpenAPI contract types (pnpm openapi:generate).
// Hand-written aliases below stay until all consumers migrate.
import type { components } from './schema'

export type Project = components['schemas']['Project']
export type Pipeline = components['schemas']['Pipeline']
export type Job = components['schemas']['Job']

export type Status = 'queued' | 'running' | 'success' | 'failed' | 'canceled'

export type StageDetail = components['schemas']['StageDetail']

export interface Stage extends Omit<StageDetail, 'jobs'> {
  jobs: Job[]
}

export interface PipelineDetail {
  pipeline: Pipeline
  stages: Stage[]
}

export interface JobLog {
  id: number
  job_id: string
  sequence: number
  message: string
  created_at: string
}

export interface Repository {
  id: string
  name: string
  created_at: string
}

export interface RepositoryRef {
  name: string
  sha: string
  target: string
}

export interface Commit {
  sha: string
  short_sha: string
  author: string
  email: string
  message: string
  date: string
}

export type ChangeStatus = 'added' | 'modified' | 'deleted'

export interface ChangedFile {
  path: string
  status: ChangeStatus
  additions: number
  deletions: number
}

export interface Comparison {
  from: string
  to: string
  merge_base: string
  files: ChangedFile[]
  patch: string
}

export type PullRequestStatus = 'open' | 'closed' | 'merged'
export type PullRequestAction = 'merge' | 'close' | 'reopen'

export interface PullRequest {
  id: string
  repository_name: string
  number: number
  title: string
  description: string
  source_branch: string
  target_branch: string
  status: PullRequestStatus
  created_by: string
  created_at: string
  updated_at: string
  merged_at: string | null
  merge_commit_sha: string | null
}

export interface CreatePullRequestInput {
  repository_name: string
  title: string
  description?: string
  source_branch: string
  target_branch: string
  author?: string
}

// --- Platform: runners / secrets / artifacts / environments / schedules / webhooks / reports / audit / users / tokens ---

export type RunnerStatus = 'online' | 'offline' | 'paused'

export interface Runner {
  id: string
  name: string
  tags: string[]
  status: RunnerStatus
  last_seen_at: string | null
  created_at: string
}

export interface SecretMetadata {
  id: string
  project_id: string
  key: string
  created_at: string
  updated_at: string
}

export interface Artifact {
  id: string
  job_id: string
  name: string
  content_type: string
  size_bytes: number
  created_at: string
}

export type EnvironmentStatus = 'available' | 'stopped' | 'degraded'

export interface Environment {
  id: string
  project_id: string
  name: string
  url: string | null
  status: EnvironmentStatus
  created_at: string
}

export type DeploymentStatus = 'pending' | 'running' | 'success' | 'failed'

export interface Deployment {
  id: string
  environment_id: string
  pipeline_id: string | null
  git_ref: string
  status: DeploymentStatus
  created_at: string
}

export interface Schedule {
  id: string
  project_id: string
  cron: string
  git_ref: string
  enabled: boolean
  created_at: string
}

export interface Webhook {
  id: string
  project_id: string
  url: string
  events: string[]
  enabled: boolean
  created_at: string
}

export interface NotificationConfig {
  id: string
  channel: string
  target: string
  enabled: boolean
}

export interface ProjectReport {
  total_pipelines: number
  successful_pipelines: number
  failed_pipelines: number
  success_rate: number
  average_duration_seconds: number
}

export interface AuditEvent {
  id: number
  action: string
  resource_type: string
  resource_id: string | null
  actor: string | null
  created_at: string
}

export type UserRole = 'admin' | 'maintainer' | 'developer' | 'viewer'

export interface User {
  id: string
  username: string
  role: UserRole
  enabled: boolean
  created_at: string
}

export interface ApiToken {
  id: string
  name: string
  token_hint: string
  user_id: string | null
  created_at: string
  last_used_at: string | null
}

export interface CreatedApiToken extends ApiToken {
  value: string
}
