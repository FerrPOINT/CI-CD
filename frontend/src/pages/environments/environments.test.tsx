import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { EnvironmentsPage } from './index'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

vi.mock('sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}))

const projectId = '22222222-2222-4222-8222-222222222222'
const environmentId = '66666666-6666-4666-8666-666666666666'
const pendingDeploymentId = '77777777-7777-4777-8777-777777777777'
const successDeploymentId = '99999999-9999-4999-8999-999999999999'
const pipelineId = '11111111-1111-4111-8111-111111111111'
const now = '2026-09-01T12:00:00Z'

function renderEnvironmentsPage(requests: string[]) {
  vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const url = typeof input === 'string' ? input : input.toString()
    requests.push(`${init?.method ?? 'GET'} ${url}`)
    if (url === `/api/v1/projects/${projectId}/environments`) {
      return json([
        {
          id: environmentId,
          project_id: projectId,
          name: 'production',
          url: 'https://prod.example.invalid',
          status: 'available',
          protected: true,
          required_approvals: 1,
          created_at: now,
        },
      ])
    }
    if (url === `/api/v1/environments/${environmentId}/deployments`) {
      return json([
        {
          id: pendingDeploymentId,
          environment_id: environmentId,
          pipeline_id: null,
          rollback_of_id: null,
          git_ref: 'main',
          status: 'pending',
          approval_required: true,
          approval_state: 'pending',
          approval_count: 0,
          required_approvals: 1,
          created_at: now,
        },
        {
          id: successDeploymentId,
          environment_id: environmentId,
          pipeline_id: pipelineId,
          rollback_of_id: null,
          git_ref: 'release-2026-08',
          status: 'success',
          approval_required: false,
          approval_state: 'not_required',
          approval_count: 0,
          required_approvals: 0,
          created_at: now,
        },
      ])
    }
    if (url === `/api/v1/deployments/${pendingDeploymentId}/approvals`) {
      return json({
        id: pendingDeploymentId,
        environment_id: environmentId,
        pipeline_id: pipelineId,
        rollback_of_id: null,
        git_ref: 'main',
        status: 'pending',
        approval_required: true,
        approval_state: 'approved',
        approval_count: 1,
        required_approvals: 1,
        created_at: now,
      })
    }
    if (url === `/api/v1/deployments/${successDeploymentId}/rollback`) {
      return json({
        id: 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa',
        environment_id: environmentId,
        pipeline_id: 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb',
        rollback_of_id: successDeploymentId,
        git_ref: 'release-2026-08',
        status: 'pending',
        approval_required: false,
        approval_state: 'not_required',
        approval_count: 0,
        required_approvals: 0,
        created_at: now,
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
      <MemoryRouter initialEntries={[`/projects/${projectId}/environments`]}>
        <Routes>
          <Route path="/projects/:projectId/environments" element={<EnvironmentsPage />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

function json(value: unknown): Promise<Response> {
  return Promise.resolve(new Response(JSON.stringify(value), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  }))
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('EnvironmentsPage protected delivery controls', () => {
  it('shows approval state and sends approval and rollback actions', async () => {
    const requests: string[] = []
    renderEnvironmentsPage(requests)

    expect(await screen.findByText('production')).toBeInTheDocument()
    expect(screen.getByText('environments.protected · 1')).toBeInTheDocument()

    fireEvent.click(screen.getByText('environments.deployments'))

    expect(await screen.findByText('deployments.approvalPending 0/1')).toBeInTheDocument()
    fireEvent.click(screen.getByText('deployments.approve'))
    fireEvent.click(screen.getByText('deployments.rollback'))

    await waitFor(() => {
      expect(requests).toContain(`POST /api/v1/deployments/${pendingDeploymentId}/approvals`)
      expect(requests).toContain(`POST /api/v1/deployments/${successDeploymentId}/rollback`)
    })
  })
})
