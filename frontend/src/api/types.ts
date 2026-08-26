export type Status = 'queued' | 'running' | 'success' | 'failed' | 'canceled'

export interface Project {
  id: string
  name: string
  repository_url: string
  default_branch: string
  created_at: string
}

export interface Pipeline {
  id: string
  project_id: string
  git_ref: string
  status: Status
  created_at: string
  started_at: string | null
  finished_at: string | null
}

export interface Job {
  id: string
  stage_id: string
  name: string
  image: string
  command: string
  position: number
  status: Status
  started_at: string | null
  finished_at: string | null
}

export interface Stage {
  id: string
  pipeline_id: string
  name: string
  position: number
  status: Status
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
}
