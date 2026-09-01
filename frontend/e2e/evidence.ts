import { expect, type APIRequestContext } from '@playwright/test'

const apiBaseURL = process.env.E2E_API_URL ?? 'http://127.0.0.1:22801/api/v1'
export const expectedArtifactName = 'target__release__app.tar.gz'

export type Project = {
  id: string
  name: string
  repository_url: string
  default_branch: string
}

export type Pipeline = {
  id: string
  project_id: string
  git_ref: string
  status: string
}

export type Job = {
  id: string
  name: string
  status: string
  image: string
  command: string
  required_tags: string[]
  required_secrets: string[]
  artifact_paths: string[]
}

export type PipelineDetail = {
  pipeline: Pipeline
  stages: Array<{ jobs: Job[] }>
}

export type Artifact = {
  id: string
  job_id: string
  name: string
}

export type EvidenceContext = {
  project: Project
  pipeline: Pipeline
  job: Job
  artifacts: Artifact[]
}

async function apiGet<T>(request: APIRequestContext, path: string): Promise<T> {
  const response = await request.get(`${apiBaseURL}${path}`)
  if (!response.ok()) {
    const body = await response.text()
    expect(response.ok(), `${path} -> ${response.status()} ${body.slice(0, 300)}`).toBeTruthy()
  }
  return response.json() as Promise<T>
}

function sleep(ms: number): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, ms))
}

export async function waitForEvidence(request: APIRequestContext): Promise<EvidenceContext> {
  const deadline = Date.now() + 90_000
  let lastError = 'seed data is not visible yet'

  while (Date.now() < deadline) {
    try {
      const projects = await apiGet<Project[]>(request, '/projects')
      const project = projects.find(candidate => candidate.name === 'forge-demo-platform')
      if (!project) throw new Error('missing forge-demo-platform project')

      const pipelines = await apiGet<Pipeline[]>(request, `/projects/${project.id}/pipelines`)
      for (const pipeline of pipelines) {
        const detail = await apiGet<PipelineDetail>(request, `/pipelines/${pipeline.id}`)
        const jobs = detail.stages.flatMap(stage => stage.jobs)
        const job = jobs.find(candidate => candidate.name === 'compile')
          ?? jobs.find(candidate => candidate.artifact_paths.length > 0)
        if (!job) continue

        const artifacts = await apiGet<Artifact[]>(request, `/jobs/${job.id}/artifacts`)
        if (artifacts.some(artifact => artifact.name === expectedArtifactName)) {
          return { project, pipeline: detail.pipeline, job, artifacts }
        }
      }

      throw new Error('missing completed compile job artifact')
    } catch (error) {
      lastError = error instanceof Error ? error.message : String(error)
      await sleep(2_000)
    }
  }

  throw new Error(`Evidence seed was not ready before timeout: ${lastError}`)
}
