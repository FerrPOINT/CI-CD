import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ThemeProvider } from '@/shared/lib/theme'
import { appRoutes } from './router'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    i18n: { language: 'ru' },
    t: (key: string, fallback?: string | object) => typeof fallback === 'string' ? fallback : key,
  }),
}))

vi.mock('sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}))

const projectId = '22222222-2222-4222-8222-222222222222'
const pipelineId = '11111111-1111-4111-8111-111111111111'
const stageId = '33333333-3333-4333-8333-333333333333'
const jobId = '44444444-4444-4444-8444-444444444444'
const attemptId = '55555555-5555-4555-8555-555555555555'
const environmentId = '66666666-6666-4666-8666-666666666666'
const userId = '88888888-8888-4888-8888-888888888888'
const repoName = 'forge-api'
const now = '2026-08-31T12:00:00Z'

const project = {
  id: projectId,
  name: 'forge-api',
  repository_url: 'https://git.example.com/platform/forge-api.git',
  default_branch: 'main',
  created_at: now,
}

const pipeline = {
  id: pipelineId,
  project_id: projectId,
  git_ref: 'feature/route-smoke',
  commit_sha: 'abcdef1234567890abcdef1234567890abcdef12',
  status: 'running',
  created_at: now,
  updated_at: now,
}

const job = {
  id: jobId,
  stage_id: stageId,
  name: 'build-linux',
  image: 'rust:1.86-bookworm',
  command: 'cargo test --all-targets',
  status: 'running',
  required_tags: ['linux'],
  required_secrets: ['DEPLOY_TOKEN'],
  artifact_paths: ['target/release/app.tar.gz'],
}

const comparison = {
  from: 'main',
  to: 'feature/route-smoke',
  merge_base: 'abc0000abc0000abc0000abc0000abc0000abc0',
  files: [{ path: 'src/main.rs', status: 'modified', additions: 12, deletions: 3 }],
  patch: 'diff --git a/src/main.rs b/src/main.rs\n@@ -1 +1 @@\n-route smoke old\n+route smoke new\n',
}

const pullRequest = {
  id: '99999999-9999-4999-8999-999999999999',
  repository_name: repoName,
  number: 7,
  title: 'Add route smoke',
  description: 'Exercise every dashboard route in CI.',
  source_branch: 'feature/route-smoke',
  target_branch: 'main',
  status: 'open',
  created_by: 'admin',
  created_at: now,
  updated_at: now,
  merged_at: null,
  merge_commit_sha: null,
}

const routeCases = [
  { entry: '/', marker: 'forge-api' },
  { entry: '/projects', marker: 'forge-api' },
  { entry: `/projects/${projectId}/pipelines`, marker: 'feature/route-smoke' },
  { entry: `/pipelines/${pipelineId}`, marker: 'build-linux' },
  { entry: '/repositories', marker: 'forge-api' },
  { entry: `/repositories/${repoName}`, marker: 'Add route smoke' },
  { entry: `/repositories/${repoName}/compare?from=main&to=feature/route-smoke`, marker: 'src/main.rs' },
  { entry: `/repositories/${repoName}/pulls`, marker: 'Add route smoke' },
  { entry: `/repositories/${repoName}/pulls/7`, marker: 'Exercise every dashboard route in CI.' },
  { entry: '/settings', marker: 'CICD_RUNNER_REGISTRATION_TOKEN' },
  { entry: '/runners', marker: 'linux-runner-1' },
  { entry: `/projects/${projectId}/secrets`, marker: 'DEPLOY_TOKEN' },
  { entry: `/projects/${projectId}/members`, marker: 'admin' },
  { entry: `/jobs/${jobId}/artifacts`, marker: 'app.tar.gz' },
  { entry: `/projects/${projectId}/environments`, marker: 'https://prod.example.com' },
  { entry: `/projects/${projectId}/schedules`, marker: '0 4 * * 1' },
  { entry: `/projects/${projectId}/webhooks`, marker: 'Pipeline failed' },
  { entry: `/projects/${projectId}/reports`, marker: '75.0%' },
  { entry: '/audit-log', marker: 'project.created' },
  { entry: '/users', marker: 'admin-token' },
  { entry: '/login', marker: 'login.description' },
] as const

afterEach(() => {
  cleanup()
  window.localStorage.clear()
  vi.unstubAllGlobals()
})

describe('app router smoke', () => {
  it.each(routeCases)('renders $entry', async ({ entry, marker }) => {
    renderRoute(entry)
    await expectText(marker)
  })
})

function renderRoute(entry: string) {
  installLocalStorage()
  vi.stubGlobal('fetch', vi.fn(mockFetch))

  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })
  const router = createMemoryRouter(appRoutes, { initialEntries: [entry] })

  render(
    <QueryClientProvider client={client}>
      <ThemeProvider>
        <RouterProvider router={router} />
      </ThemeProvider>
    </QueryClientProvider>,
  )
}

function installLocalStorage() {
  const store = new Map<string, string>()
  const storage = {
    get length() {
      return store.size
    },
    clear: vi.fn(() => store.clear()),
    getItem: vi.fn((key: string) => store.get(key) ?? null),
    key: vi.fn((index: number) => Array.from(store.keys())[index] ?? null),
    removeItem: vi.fn((key: string) => {
      store.delete(key)
    }),
    setItem: vi.fn((key: string, value: string) => {
      store.set(key, value)
    }),
  } satisfies Storage
  Object.defineProperty(window, 'localStorage', { configurable: true, value: storage })
  Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: storage })
}

async function expectText(text: string) {
  await waitFor(
    () => {
      expect(
        screen.getAllByText((_, element) => element?.textContent?.includes(text) ?? false).length,
      ).toBeGreaterThan(0)
    },
    { timeout: 5000 },
  )
}

function mockFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  const url = new URL(typeof input === 'string' ? input : input.toString(), 'http://localhost')
  const path = url.pathname.replace(/^\/api\/v1/, '')
  const method = init?.method ?? 'GET'

  if (method !== 'GET') return json({})

  if (path === '/projects') return json([project])
  if (path === `/projects/${projectId}/pipelines`) return json([pipeline])
  if (path === `/pipelines/${pipelineId}`) {
    return json({
      pipeline,
      plan: {
        id: 'plan-1',
        pipeline_id: pipelineId,
        config_source: 'repository',
        parser_version: 'forge-ci-v1',
        resolved_commit_sha: pipeline.commit_sha,
        config_sha256: 'c'.repeat(64),
        plan_sha256: 'p'.repeat(64),
        plan: { dependencies: [['build-linux', 'deploy-prod']] },
        created_at: now,
      },
      stages: [{ id: stageId, pipeline_id: pipelineId, name: 'build', status: 'running', jobs: [job] }],
    })
  }
  if (path === `/jobs/${jobId}/attempts`) {
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
  if (path === `/jobs/${jobId}/attempts/${attemptId}/logs/page`) {
    return json({
      items: [{ id: 1, job_id: jobId, attempt_id: attemptId, sequence: 1, message: 'cargo test started', created_at: now }],
      next_after: null,
    })
  }
  if (path === `/jobs/${jobId}/test-report`) {
    return json([
      {
        id: 'test-report-1',
        job_id: jobId,
        suite_name: 'unit',
        tests_total: 24,
        tests_passed: 24,
        tests_failed: 0,
        tests_skipped: 0,
        duration_ms: 1200,
        created_at: now,
      },
    ])
  }
  if (path === '/repositories') {
    return json([{ id: 'repo-1', name: repoName, created_at: now }])
  }
  if (path === `/repos/${repoName}/refs`) {
    return json([
      { name: 'main', sha: 'abcdef1234567890', target: 'refs/heads/main' },
      { name: 'feature/route-smoke', sha: 'fedcba0987654321', target: 'refs/heads/feature/route-smoke' },
    ])
  }
  if (path === `/repos/${repoName}/commits`) {
    return json([
      {
        sha: 'abcdef1234567890abcdef1234567890abcdef12',
        short_sha: 'abcdef1',
        author: 'admin',
        email: 'admin@example.com',
        message: 'Add route smoke',
        date: now,
      },
    ])
  }
  if (path === `/repos/${repoName}/compare`) return json(comparison)
  if (path === `/repos/${repoName}/pulls`) return json([pullRequest])
  if (path === `/repos/${repoName}/tree`) {
    return json([{ path: 'src/main.rs', name: 'main.rs', kind: 'blob', size: 42, sha: 'abcdef1234567890' }])
  }
  if (path === `/repos/${repoName}/blob`) {
    return json({ path: 'src/main.rs', sha: 'abcdef1234567890', size: 42, content: 'fn main() {}', binary: false, truncated: false })
  }
  if (path === `/repos/${repoName}/tags`) {
    return json([{ name: 'v0.1.0', sha: 'abcdef1234567890', message: 'Initial release' }])
  }
  if (path === `/repos/${repoName}/releases`) {
    return json([
      {
        id: 'release-1',
        repository_name: repoName,
        tag_name: 'v0.1.0',
        name: 'Forge 0.1.0',
        description: 'Initial release',
        prerelease: false,
        created_by: userId,
        created_at: now,
      },
    ])
  }
  if (path === '/runners') {
    return json([{ id: 'runner-1', name: 'linux-runner-1', tags: ['linux', 'docker'], status: 'online', last_seen_at: now, created_at: now }])
  }
  if (path === `/projects/${projectId}/secrets`) {
    return json([{ id: 'secret-1', project_id: projectId, key: 'DEPLOY_TOKEN', created_at: now, updated_at: now }])
  }
  if (path === '/users') {
    return json([{ id: userId, username: 'admin', role: 'admin', enabled: true, created_at: now }])
  }
  if (path === `/projects/${projectId}/memberships`) {
    return json([{ project_id: projectId, user_id: userId, username: 'admin', role: 'maintainer', user_enabled: true, updated_at: now }])
  }
  if (path === `/jobs/${jobId}/artifacts`) {
    return json([
      {
        id: 'artifact-1',
        job_id: jobId,
        attempt_id: attemptId,
        name: 'app.tar.gz',
        content_type: 'application/gzip',
        sha256: 'a'.repeat(64),
        size_bytes: 4096,
        created_at: now,
      },
    ])
  }
  if (path === `/projects/${projectId}/environments`) {
    return json([{ id: environmentId, project_id: projectId, name: 'production', url: 'https://prod.example.com', status: 'available', created_at: now }])
  }
  if (path === `/environments/${environmentId}/deployments`) {
    return json([{ id: 'deploy-1', environment_id: environmentId, pipeline_id: pipelineId, git_ref: 'main', status: 'success', created_at: now }])
  }
  if (path === `/projects/${projectId}/schedules`) {
    return json([
      {
        id: 'schedule-1',
        project_id: projectId,
        cron: '0 4 * * 1',
        git_ref: 'main',
        enabled: true,
        next_fire_at: '2026-09-07T04:00:00Z',
        last_fired_at: now,
        last_fire_error: null,
        created_at: now,
      },
    ])
  }
  if (path === `/projects/${projectId}/webhooks`) {
    return json([{ id: 'webhook-1', project_id: projectId, url: 'https://hooks.example.com/ci', events: ['pipeline.failed'], enabled: true, created_at: now }])
  }
  if (path === `/projects/${projectId}/outbox-deliveries`) {
    return json([
      {
        id: 'delivery-1',
        project_id: projectId,
        event_id: 'event-1',
        replay_of_id: null,
        generation: 0,
        subscription_id: 'notification:external',
        channel: 'notification',
        destination: `project:${projectId}`,
        event_type: 'pipeline.failed',
        aggregate_type: 'pipeline',
        aggregate_id: pipelineId,
        status: 'failed',
        attempts: 3,
        next_attempt_at: null,
        delivered_at: null,
        failed_at: now,
        last_error: 'receiver returned 500',
        created_at: now,
      },
    ])
  }
  if (path === `/projects/${projectId}/notifications`) {
    return json([{ id: 'notification-1', channel: 'in_app', target: 'dashboard', enabled: true }])
  }
  if (path === `/projects/${projectId}/notification-events`) {
    return json([
      {
        id: 'notification-event-1',
        event_id: 'event-1',
        subscription_id: 'notification:in_app',
        channel: 'in_app',
        target: 'dashboard',
        event_type: 'pipeline.failed',
        pipeline_id: pipelineId,
        status: 'failed',
        message: 'Pipeline failed',
        attempts: 1,
        delivered_at: now,
        last_error: null,
        created_at: now,
      },
    ])
  }
  if (path === `/projects/${projectId}/reports/summary`) {
    return json({ total_pipelines: 4, successful_pipelines: 3, failed_pipelines: 1, success_rate: 0.75, average_duration_seconds: 90 })
  }
  if (path === '/audit-log') {
    return json([{ id: 1, action: 'project.created', resource_type: 'project', resource_id: projectId, actor: 'admin', created_at: now }])
  }
  if (path === '/api-tokens') {
    return json([
      {
        id: 'token-1',
        name: 'admin-token',
        token_hint: 'fgp_...abcd',
        user_id: userId,
        project_id: projectId,
        scopes: ['api:read', 'api:write'],
        expires_at: '2026-09-30T12:00:00Z',
        revoked_at: null,
        created_at: now,
        last_used_at: now,
      },
    ])
  }

  return Promise.resolve(new Response('not found', { status: 404 }))
}

function json(value: unknown): Promise<Response> {
  return Promise.resolve(new Response(JSON.stringify(value), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  }))
}
