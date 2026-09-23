import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
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

function renderPipelineDetail(
  requests: string[],
  options: {
    jobStatus?: 'running' | 'failed'
    attemptErrorTail?: string | null
    failFirstDetail?: boolean
    failFirstLogs?: boolean
    failFirstOlderLogs?: boolean
    newLogOnRefresh?: boolean
  } = {},
) {
  const jobStatus = options.jobStatus ?? 'running'
  const attemptErrorTail = options.attemptErrorTail ?? null
  let detailCalls = 0
  let logCalls = 0
  let tailCalls = 0
  let olderLogCalls = 0
  vi.stubGlobal(
    'fetch',
    vi.fn((input: RequestInfo | URL) => {
      const url = typeof input === 'string' ? input : input.toString()
      requests.push(url)
      if (url === `/api/v1/pipelines/${pipelineId}`) {
        if (options.failFirstDetail && detailCalls++ === 0) {
          return Promise.resolve(new Response('unavailable', { status: 503 }))
        }
        return json({
          pipeline: {
            id: pipelineId,
            project_id: projectId,
            git_ref: 'main',
            status: jobStatus,
            created_at: now,
            started_at: now,
            finished_at: jobStatus === 'failed' ? now : null,
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
              status: jobStatus,
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
                  status: jobStatus,
                  started_at: now,
                  finished_at: jobStatus === 'failed' ? now : null,
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
            status: jobStatus,
            trigger: 'initial',
            exit_code: null,
            error_tail: attemptErrorTail,
            created_at: now,
            started_at: now,
            finished_at: jobStatus === 'failed' ? now : null,
          },
        ])
      }
      if (url === `/api/v1/jobs/${jobId}/test-report`) {
        return json([])
      }
      if (url === `/api/v1/pipelines/${pipelineId}/cancel`) {
        return json({ canceled: pipelineId })
      }
      if (url === `/api/v1/pipelines/${pipelineId}/retry`) {
        return json({ retried: pipelineId })
      }
      if (url.startsWith(`/api/v1/jobs/${jobId}/attempts/${attemptId}/logs/page?`)) {
        if (options.failFirstLogs && logCalls++ === 0) {
          return Promise.resolve(new Response('unavailable', { status: 503 }))
        }
        const params = new URLSearchParams(url.split('?')[1])
        if (params.get('q') === 'error') {
          return json({
            items: [logRow(2, 'unit error: expected status')],
            next_after: null,
            total: 1,
            has_more_before: false,
          })
        }
        if (params.get('before') === '201') {
          if (options.failFirstOlderLogs && olderLogCalls++ === 0) {
            return Promise.resolve(new Response('unavailable', { status: 503 }))
          }
          return json({
            items: [logRow(199, 'compile sources'), logRow(200, 'package artifacts')],
            next_after: null,
            total: 200,
            has_more_before: false,
          })
        }
        if (params.get('before') === '2147483647' && options.newLogOnRefresh && tailCalls++ > 0) {
          return json({
            items: [logRow(202, 'deploy complete'), logRow(203, 'cleanup complete')],
            next_after: 202,
            total: 203,
            has_more_before: true,
          })
        }
        return json({
          items: [logRow(201, 'upload artifacts'), logRow(202, 'deploy complete')],
          next_after: 201,
          total: 202,
          has_more_before: true,
        })
      }
      return Promise.resolve(new Response('not found', { status: 404 }))
    }),
  )

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
  return Promise.resolve(
    new Response(JSON.stringify(value), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    }),
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
  vi.clearAllMocks()
})

describe('PipelineDetailPage logs', () => {
  it('shows immutable pipeline plan evidence', async () => {
    const requests: string[] = []
    renderPipelineDetail(requests)

    expect(await screen.findByText('pipelines.planTitle')).toBeInTheDocument()
    const planToggle = screen.getByText('pipelines.planTitle').closest('summary')
    expect(planToggle?.closest('details')).not.toHaveAttribute('open')
    fireEvent.click(planToggle!)
    expect(planToggle?.closest('details')).toHaveAttribute('open')
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

  it('opens at the latest logs, loads older rows in order, and searches forward', async () => {
    const requests: string[] = []
    renderPipelineDetail(requests)

    fireEvent.click(await screen.findByRole('button', { name: /jobs\.logs/ }))

    await waitFor(() => expect(logOutput()).toContain('201  upload artifacts'))
    expect(screen.getByRole('log')).toHaveAttribute('tabindex', '0')
    expect(logOutput()).toContain('202  deploy complete')
    expect(
      requests.some((url) => url.includes('before=2147483647') && !url.includes('after=')),
    ).toBe(true)

    fireEvent.click(screen.getByRole('button', { name: 'jobs.loadEarlierLogs' }))
    await waitFor(() => expect(logOutput()).toContain('199  compile sources'))
    expect(requests.some((url) => url.includes('before=201'))).toBe(true)
    expect(logOutput().indexOf('199  compile sources')).toBeLessThan(
      logOutput().indexOf('201  upload artifacts'),
    )

    fireEvent.change(screen.getByPlaceholderText('jobs.searchLogs'), {
      target: { value: 'error' },
    })

    await waitFor(() => {
      expect(requests.some((url) => url.includes('q=error'))).toBe(true)
      expect(requests.some((url) => url.includes('q=error') && url.includes('after=0'))).toBe(true)
      expect(logOutput()).toContain('002  unit error: expected status')
      expect(logOutput()).not.toContain('201  upload artifacts')
    })
  })

  it('shows latest attempt diagnostic on failed job cards', async () => {
    const requests: string[] = []
    renderPipelineDetail(requests, {
      jobStatus: 'failed',
      attemptErrorTail: 'no compatible runner before queue timeout',
    })

    expect(await screen.findByText('no compatible runner before queue timeout')).toBeInTheDocument()
    expect(requests).toContain(`/api/v1/jobs/${jobId}/attempts`)
  })

  it('keeps a reader in place when new live output arrives', async () => {
    const requests: string[] = []
    renderPipelineDetail(requests, { newLogOnRefresh: true })

    fireEvent.click(await screen.findByRole('button', { name: /jobs\.logs/ }))
    const output = await screen.findByRole('log')
    await waitFor(() => expect(output.textContent).toContain('202  deploy complete'))
    Object.defineProperties(output, {
      scrollHeight: { configurable: true, value: 1_000 },
      clientHeight: { configurable: true, value: 300 },
      scrollTop: { configurable: true, writable: true, value: 100 },
    })

    fireEvent.scroll(output)
    fireEvent.click(screen.getByRole('button', { name: 'jobs.refreshLogs' }))

    await waitFor(() => expect(output.textContent).toContain('203  cleanup complete'))
    expect(output.scrollTop).toBe(100)
    fireEvent.click(screen.getByRole('button', { name: 'jobs.latestLogs' }))
    expect(output.scrollTop).toBe(1_000)
    expect(screen.queryByRole('button', { name: 'jobs.latestLogs' })).not.toBeInTheDocument()
  })

  it('keeps loaded logs visible and retries a failed older page', async () => {
    const requests: string[] = []
    renderPipelineDetail(requests, { failFirstOlderLogs: true })

    fireEvent.click(await screen.findByRole('button', { name: /jobs\.logs/ }))
    await waitFor(() => expect(logOutput()).toContain('202  deploy complete'))
    fireEvent.click(screen.getByRole('button', { name: 'jobs.loadEarlierLogs' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('jobs.logsPageError')
    expect(logOutput()).toContain('202  deploy complete')
    fireEvent.click(within(screen.getByRole('alert')).getByRole('button', { name: 'common.retry' }))
    await waitFor(() => expect(logOutput()).toContain('199  compile sources'))
  })

  it('shows a recoverable error instead of an endless loading state', async () => {
    const requests: string[] = []
    renderPipelineDetail(requests, { failFirstDetail: true })

    expect(await screen.findByRole('alert')).toHaveTextContent('pipelines.loadError')
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(await screen.findByText('pipelines.planTitle')).toBeInTheDocument()
    expect(requests.filter((url) => url === `/api/v1/pipelines/${pipelineId}`)).toHaveLength(2)
  })

  it('requires confirmation before canceling a running pipeline', async () => {
    const requests: string[] = []
    renderPipelineDetail(requests)
    fireEvent.click(await screen.findByRole('button', { name: 'pipelines.cancel' }))
    const dialog = screen.getByRole('alertdialog')
    expect(requests).not.toContain(`/api/v1/pipelines/${pipelineId}/cancel`)
    fireEvent.click(within(dialog).getByRole('button', { name: 'pipelines.keepRunning' }))
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'pipelines.cancel' }))
    fireEvent.click(
      within(screen.getByRole('alertdialog')).getByRole('button', {
        name: 'pipelines.confirmCancel',
      }),
    )
    await waitFor(() => expect(requests).toContain(`/api/v1/pipelines/${pipelineId}/cancel`))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
    await waitFor(() =>
      expect(requests.filter((url) => url === `/api/v1/pipelines/${pipelineId}`)).toHaveLength(2),
    )
  })

  it('refreshes a failed pipeline after retry', async () => {
    const requests: string[] = []
    renderPipelineDetail(requests, { jobStatus: 'failed' })
    fireEvent.click(await screen.findByRole('button', { name: 'pipelines.retry' }))
    await waitFor(() => expect(requests).toContain(`/api/v1/pipelines/${pipelineId}/retry`))
    await waitFor(() =>
      expect(requests.filter((url) => url === `/api/v1/pipelines/${pipelineId}`)).toHaveLength(2),
    )
  })

  it('keeps manual job transitions out of the primary actions', async () => {
    const requests: string[] = []
    renderPipelineDetail(requests)
    const summary = await screen.findByText('jobs.manualStatus')
    expect(summary.closest('details')).not.toHaveAttribute('open')
    fireEvent.click(summary)
    expect(summary.closest('details')).toHaveAttribute('open')
    expect(screen.getByRole('button', { name: 'jobs.pass' })).toBeInTheDocument()
  })

  it('shows log errors with a retry action', async () => {
    const requests: string[] = []
    renderPipelineDetail(requests, { failFirstLogs: true })
    fireEvent.click(await screen.findByRole('button', { name: /jobs\.logs/ }))
    expect(await screen.findByRole('alert')).toHaveTextContent('jobs.logsError')
    fireEvent.click(within(screen.getByRole('alert')).getByRole('button', { name: 'common.retry' }))
    await waitFor(() => expect(logOutput()).toContain('201  upload artifacts'))
  })
})

function logOutput(): string {
  const block = document.querySelector('pre')
  if (!block) throw new Error('log output was not rendered')
  return block.textContent ?? ''
}
