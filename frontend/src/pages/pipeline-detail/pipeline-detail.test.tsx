import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { PipelineDetailPage } from './index'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string, fallback?: string) => fallback ?? key }),
}))

vi.mock('sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}))

const pipelineId = '11111111-1111-4111-8111-111111111111'
const projectId = '22222222-2222-4222-8222-222222222222'
const stageId = '33333333-3333-4333-8333-333333333333'
const jobId = '44444444-4444-4444-8444-444444444444'
const attemptId = '55555555-5555-4555-8555-555555555555'
const now = '2026-08-31T12:00:00Z'

function renderPipelineDetail(requests: string[]) {
  vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
    const url = typeof input === 'string' ? input : input.toString()
    requests.push(url)
    if (url === `/api/v1/pipelines/${pipelineId}`) {
      return json({
        pipeline: {
          id: pipelineId,
          project_id: projectId,
          git_ref: 'main',
          status: 'running',
          created_at: now,
          started_at: now,
          finished_at: null,
        },
        plan: {
          pipeline_id: pipelineId,
          config_source: 'legacy_template',
          parser_version: 'forge-legacy-linear/1',
          git_ref: 'main',
          resolved_commit_sha: null,
          config_sha256: 'a'.repeat(64),
          plan_sha256: 'b'.repeat(64),
          raw_config: 'stages:\n  - name: build\n',
          plan: {
            format: 'legacy-linear',
            dependencies: [{ from: 'stage-0/job-0', to: 'stage-1/job-0' }],
          },
          created_at: now,
        },
        stages: [
          {
            id: stageId,
            pipeline_id: pipelineId,
            name: 'build',
            position: 0,
            status: 'running',
            jobs: [
              {
                id: jobId,
                stage_id: stageId,
                name: 'compile',
                image: 'alpine:3.21',
                command: 'echo test',
                required_tags: ['docker', 'linux'],
                required_secrets: ['DEPLOY_TOKEN'],
                artifact_paths: ['target/release/app.tar.gz'],
                position: 0,
                status: 'running',
                started_at: now,
                finished_at: null,
              },
            ],
          },
        ],
      })
    }
    if (url === `/api/v1/jobs/${jobId}/attempts`) {
      return json([
        {
          id: attemptId,
          job_id: jobId,
          attempt_no: 1,
          status: 'running',
          trigger: 'initial',
          exit_code: null,
          error_tail: null,
          created_at: now,
          started_at: now,
          finished_at: null,
        },
      ])
    }
    if (url === `/api/v1/jobs/${jobId}/test-report`) {
      return json([])
    }
    if (url.startsWith(`/api/v1/jobs/${jobId}/attempts/${attemptId}/logs/page?`)) {
      const params = new URLSearchParams(url.split('?')[1])
      if (params.get('q') === 'error') {
        return json({
          items: [logRow(2, 'unit error: expected status')],
          next_after: null,
        })
      }
      if (params.get('after') === '2') {
        return json({
          items: [logRow(3, 'package artifacts')],
          next_after: null,
        })
      }
      return json({
        items: [logRow(1, 'checkout sources'), logRow(2, 'unit error: expected status')],
        next_after: 2,
      })
    }
    return Promise.resolve(new Response('not found', { status: 404 }))
  }))

  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })

  render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[`/pipelines/${pipelineId}`]}>
        <Routes>
          <Route path="/pipelines/:pipelineId" element={<PipelineDetailPage />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

function logRow(sequence: number, message: string) {
  return {
    id: sequence,
    job_id: jobId,
    attempt_id: attemptId,
    sequence,
    message,
    created_at: now,
  }
}

function json(value: unknown): Promise<Response> {
  return Promise.resolve(new Response(JSON.stringify(value), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  }))
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('PipelineDetailPage logs', () => {
  it('shows immutable pipeline plan evidence', async () => {
    const requests: string[] = []
    renderPipelineDetail(requests)

    expect(await screen.findByText('pipelines.planTitle')).toBeInTheDocument()
    expect(screen.getByText('legacy_template')).toBeInTheDocument()
    expect(screen.getByText('forge-legacy-linear/1')).toBeInTheDocument()
    expect(screen.getByText('a'.repeat(64))).toBeInTheDocument()
    expect(screen.getByText('b'.repeat(64))).toBeInTheDocument()
    expect(screen.getByText('1')).toBeInTheDocument()
    expect(screen.getByText('jobs.runnerTags:')).toBeInTheDocument()
    expect(screen.getByText('docker')).toBeInTheDocument()
    expect(screen.getByText('linux')).toBeInTheDocument()
    expect(screen.getByText('jobs.secrets:')).toBeInTheDocument()
    expect(screen.getByText('DEPLOY_TOKEN')).toBeInTheDocument()
    expect(screen.getByText('jobs.artifacts:')).toBeInTheDocument()
    expect(screen.getByText('target/release/app.tar.gz')).toBeInTheDocument()
  })

  it('loads log pages and refetches them with search', async () => {
    const requests: string[] = []
    renderPipelineDetail(requests)

    fireEvent.click(await screen.findByRole('button', { name: /jobs\.logs/ }))

    await waitFor(() => expect(logOutput()).toContain('001  checkout sources'))
    expect(logOutput()).toContain('002  unit error: expected status')

    fireEvent.click(screen.getByRole('button', { name: 'jobs.loadMoreLogs' }))
    await waitFor(() => expect(logOutput()).toContain('003  package artifacts'))
    expect(requests.some((url) => url.includes('after=2'))).toBe(true)

    fireEvent.change(screen.getByPlaceholderText('jobs.searchLogs'), {
      target: { value: 'error' },
    })

    await waitFor(() => {
      expect(requests.some((url) => url.includes('q=error'))).toBe(true)
      expect(logOutput()).toContain('002  unit error: expected status')
      expect(logOutput()).not.toContain('001  checkout sources')
    })
  })
})

function logOutput(): string {
  const block = document.querySelector('pre')
  if (!block) throw new Error('log output was not rendered')
  return block.textContent ?? ''
}
