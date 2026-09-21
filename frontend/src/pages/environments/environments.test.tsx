import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { EnvironmentsPage } from './index'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string, options?: { name?: string; ref?: string; count?: number; total?: number }) =>
    options?.name ? `${key} ${options.name}` : options?.ref ? `${key} ${options.ref}` : options?.count !== undefined ? `${key} ${options.count}/${options.total}` : key }),
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

function renderEnvironmentsPage(requests: string[], override?: (url: string, method: string) => Promise<Response> | undefined) {
  vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const url = typeof input === 'string' ? input : input.toString()
    const method = init?.method ?? 'GET'
    requests.push(`${method} ${url}`)
    const response = override?.(url, method)
    if (response) return response
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
  return client
}

function json(value: unknown): Promise<Response> {
  return Promise.resolve(new Response(JSON.stringify(value), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  }))
}

function failure(): Promise<Response> {
  return Promise.resolve(new Response('failure', { status: 500 }))
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
    expect(screen.queryByText('environments.capabilityTitle')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'environments.deploymentsFor production' }))

    expect(await screen.findByText(/deployments.approvalPending 0\/1/)).toBeInTheDocument()
    fireEvent.click(screen.getByText('deployments.approve'))
    expect(screen.getByText('deployments.confirmApproved main')).toBeInTheDocument()
    fireEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'deployments.approve' }))
    await waitFor(() => expect(requests).toContain(`POST /api/v1/deployments/${pendingDeploymentId}/approvals`))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
    fireEvent.click(screen.getByText('deployments.rollback'))
    expect(screen.getByText('deployments.confirmRollback release-2026-08')).toBeInTheDocument()
    fireEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'deployments.rollback' }))

    await waitFor(() => {
      expect(requests).toContain(`POST /api/v1/deployments/${pendingDeploymentId}/approvals`)
      expect(requests).toContain(`POST /api/v1/deployments/${successDeploymentId}/rollback`)
    })
  })

  it('paginates and filters long environment lists without unsafe URL links', async () => {
    const environments = Array.from({ length: 25 }, (_, index) => ({
      id: `env-${index + 1}`, project_id: projectId, name: `env-${String(index + 1).padStart(2, '0')}`,
      url: index === 0 ? 'javascript:alert(1)' : null,
      status: index === 1 ? 'stopped' : 'available', protected: false, required_approvals: 0, created_at: now,
    }))
    renderEnvironmentsPage([], (url) => url.endsWith('/environments') ? json(environments) : undefined)
    expect(await screen.findByText('env-20')).toBeInTheDocument()
    expect(screen.getAllByRole('listitem')).toHaveLength(20)
    expect(screen.queryByRole('link', { name: 'javascript:alert(1)' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'environments.deploymentsFor env-01' })).toHaveClass('h-10', 'w-10')
    fireEvent.click(screen.getByRole('button', { name: 'environments.next' }))
    expect(screen.getAllByRole('listitem')).toHaveLength(5)
    fireEvent.change(screen.getByRole('searchbox', { name: 'environments.search' }), { target: { value: 'env-07' } })
    expect(screen.getAllByRole('listitem')).toHaveLength(1)
    fireEvent.change(screen.getByRole('searchbox', { name: 'environments.search' }), { target: { value: '' } })
    fireEvent.change(screen.getByRole('combobox', { name: 'environments.statusFilter' }), { target: { value: 'stopped' } })
    expect(screen.getAllByRole('listitem')).toHaveLength(1)
    expect(screen.getAllByText('environments.statusStopped')).toHaveLength(2)
  })

  it('hides stale rows on loading failure and retries successfully', async () => {
    const requests: string[] = []
    let failed = false
    const client = renderEnvironmentsPage(requests, (url, method) =>
      method === 'GET' && url.endsWith('/environments') && failed ? failure() : undefined)
    expect(await screen.findByText('production')).toBeInTheDocument()
    failed = true
    await client.invalidateQueries({ queryKey: ['environments'] })
    expect(await screen.findByRole('alert')).toHaveTextContent('environments.loadFailed')
    expect(screen.queryByText('production')).not.toBeInTheDocument()
    failed = false
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(await screen.findByText('production')).toBeInTheDocument()
  })

  it('preserves form input on create failure and keeps deletion open on failure', async () => {
    let deleteCount = 0
    let finishDelete: ((response: Response) => void) | undefined
    renderEnvironmentsPage([], (url, method) => {
      if (method === 'POST' && url.endsWith('/environments')) return failure()
      if (method === 'DELETE' && url.endsWith(`/environments/${environmentId}`)) {
        deleteCount++
        return deleteCount === 1 ? new Promise(resolve => { finishDelete = resolve }) : json({ deleted: environmentId })
      }
      return undefined
    })
    expect(await screen.findByText('production')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'environments.create' }))
    fireEvent.change(screen.getByRole('textbox', { name: 'environments.name' }), { target: { value: 'staging' } })
    fireEvent.click(screen.getByRole('textbox', { name: 'environments.name' }).closest('form')!.querySelector('button[type="submit"]')!)
    expect(await screen.findByRole('alert')).toHaveTextContent('environments.createFailed')
    expect(screen.getByRole('textbox', { name: 'environments.name' })).toHaveValue('staging')

    fireEvent.click(screen.getByRole('button', { name: 'environments.deleteFor production' }))
    expect(screen.getByText('environments.deleteWarning')).toBeInTheDocument()
    fireEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'common.delete' }))
    await waitFor(() => expect(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'common.delete' })).toBeDisabled())
    finishDelete!(new Response('failure', { status: 500 }))
    expect(await screen.findByText(/environments.deleteFailed/)).toBeInTheDocument()
    fireEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'common.delete' }))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
    expect(deleteCount).toBe(2)
  })

  it('shows deployment load errors and retries an approval decision in place', async () => {
    let failedList = true
    let approvalCount = 0
    renderEnvironmentsPage([], (url, method) => {
      if (url.endsWith(`/environments/${environmentId}/deployments`) && failedList) return failure()
      if (method === 'POST' && url.endsWith(`/deployments/${pendingDeploymentId}/approvals`)) {
        approvalCount++
        return approvalCount === 1 ? failure() : json({ id: pendingDeploymentId, environment_id: environmentId })
      }
      return undefined
    })
    expect(await screen.findByText('production')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'environments.deploymentsFor production' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('deployments.loadFailed')
    failedList = false
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(await screen.findByText(/deployments.approvalPending 0\/1/)).toBeInTheDocument()
    fireEvent.click(screen.getByText('deployments.approve'))
    fireEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'deployments.approve' }))
    expect(await screen.findByText(/deployments.actionFailed/)).toBeInTheDocument()
    fireEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'deployments.approve' }))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
    expect(approvalCount).toBe(2)
  })
})
