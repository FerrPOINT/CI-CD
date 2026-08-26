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
