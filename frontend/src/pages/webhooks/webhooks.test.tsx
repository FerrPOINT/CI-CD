import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { WebhooksPage } from './index'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

vi.mock('sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}))

const projectId = '22222222-2222-4222-8222-222222222222'
const pipelineId = '11111111-1111-4111-8111-111111111111'
const deliveryId = '33333333-3333-4333-8333-333333333333'
const replayId = '44444444-4444-4444-8444-444444444444'

function renderWebhooksPage(requests: string[], options?: { notificationGetError?: boolean; writes?: string[] }) {
  vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const url = typeof input === 'string' ? input : input.toString()
    requests.push(`${init?.method ?? 'GET'} ${url}`)
    if (url === `/api/v1/projects/${projectId}/webhooks`) {
      return json([])
    }
    if (url === `/api/v1/projects/${projectId}/outbox-deliveries?limit=20`) {
      return json([
        {
          id: deliveryId,
          project_id: projectId,
          event_id: 'de1',
          replay_of_id: null,
          generation: 0,
          subscription_id: 'notification:external',
          channel: 'notification',
          destination: `project:${projectId}`,
          event_type: 'pipeline.failed',
          aggregate_type: 'pipeline',
          aggregate_id: pipelineId,
          status: 'failed',
          attempts: 8,
          next_attempt_at: '2026-08-31T12:00:00Z',
          delivered_at: null,
          failed_at: '2026-08-31T12:00:00Z',
          last_error: 'unsupported notification channel: email',
          created_at: '2026-08-31T12:00:00Z',
        },
      ])
    }
    if (url === `/api/v1/outbox-deliveries/${deliveryId}`) {
      return json({
        delivery: {
          id: deliveryId,
          project_id: projectId,
          event_id: 'de1',
          replay_of_id: null,
          generation: 0,
          subscription_id: 'notification:external',
          channel: 'notification',
          destination: `project:${projectId}`,
          event_type: 'pipeline.failed',
          aggregate_type: 'pipeline',
          aggregate_id: pipelineId,
          status: 'failed',
          attempts: 8,
          next_attempt_at: '2026-08-31T12:00:00Z',
          delivered_at: null,
          failed_at: '2026-08-31T12:00:00Z',
          last_error: 'unsupported notification channel: email',
          created_at: '2026-08-31T12:00:00Z',
        },
        attempts: [
          {
            id: 1,
            message_id: deliveryId,
            attempt_number: 8,
            started_at: '2026-08-31T12:00:00Z',
            finished_at: '2026-08-31T12:00:01Z',
            outcome: 'failed',
            http_status: null,
            error_message: 'unsupported notification channel: email',
            duration_ms: 7,
            created_at: '2026-08-31T12:00:01Z',
          },
        ],
      })
    }
    if (url === `/api/v1/outbox-deliveries/${deliveryId}/requeue`) {
      return json({ id: replayId, replay_of_id: deliveryId })
    }
    if (url === `/api/v1/projects/${projectId}/notifications`) {
      if (init?.method === 'PUT') {
        options?.writes?.push(String(init.body))
        return json([])
      }
      if (options?.notificationGetError) {
        return Promise.resolve(new Response('unavailable', { status: 503 }))
      }
      return json([{ id: 'n1', channel: 'in_app', target: 'dashboard', enabled: false,
        aggregation_window_secs: 120, quiet_start_min: 1320, quiet_end_min: 420,
        quiet_action: 'drop', quiet_bypass_statuses: ['failed', 'canceled'] }])
    }
    if (url === `/api/v1/projects/${projectId}/notification-events?limit=20`) {
      return json([
        {
          id: 'e1',
          event_id: 'de1',
          subscription_id: 'notification:n1',
          channel: 'in_app',
          target: 'dashboard',
          event_type: 'pipeline.failed',
          pipeline_id: pipelineId,
          status: 'failed',
          message: 'Pipeline failed',
          attempts: 0,
          delivered_at: '2026-08-31T12:00:00Z',
          last_error: null,
          created_at: '2026-08-31T12:00:00Z',
        },
      ])
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
      <MemoryRouter initialEntries={[`/projects/${projectId}/webhooks`]}>
        <Routes>
          <Route path="/projects/:projectId/webhooks" element={<WebhooksPage />} />
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

describe('WebhooksPage notifications', () => {
  it('preserves delivery policy when editing a channel', async () => {
    const writes: string[] = []
    renderWebhooksPage([], { writes })

    fireEvent.change(await screen.findByLabelText('notifications.target'), { target: { value: 'team-dashboard' } })
    expect(screen.getByRole('checkbox', { name: 'notifications.enabled' })).not.toBeChecked()
    fireEvent.click(screen.getByRole('button', { name: 'common.save' }))

    await waitFor(() => expect(writes).toHaveLength(1))
    expect(JSON.parse(writes[0])).toEqual([{
      channel: 'in_app', target: 'team-dashboard', enabled: false,
      aggregation_window_secs: 120, quiet_start_min: 1320, quiet_end_min: 420,
      quiet_action: 'drop', quiet_bypass_statuses: ['failed', 'canceled'],
    }])
  })

  it('does not offer replacement when current settings cannot load', async () => {
    const requests: string[] = []
    renderWebhooksPage(requests, { notificationGetError: true })

    expect(await screen.findByText('notifications.loadFailed')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'common.save' })).not.toBeInTheDocument()
    expect(requests).not.toContain(`PUT /api/v1/projects/${projectId}/notifications`)
  })

  it('shows delivered in-app notification events', async () => {
    const requests: string[] = []
    renderWebhooksPage(requests)

    expect(await screen.findByText('Pipeline failed')).toBeInTheDocument()
    expect(screen.getByText('in_app / dashboard')).toBeInTheDocument()
    expect(screen.getByText('notifications.delivered')).toBeInTheDocument()
    expect(screen.getByText(`notification / project:${projectId}`)).toBeInTheDocument()
    expect(screen.getAllByText('deliveries.failed').length).toBeGreaterThan(0)
    expect(requests).toContain(`GET /api/v1/projects/${projectId}/notification-events?limit=20`)
    expect(requests).toContain(`GET /api/v1/projects/${projectId}/outbox-deliveries?limit=20`)

    fireEvent.click(screen.getByText(`notification / project:${projectId}`))

    expect(await screen.findByText('deliveries.details')).toBeInTheDocument()
    expect(requests).toContain(`GET /api/v1/outbox-deliveries/${deliveryId}`)

    fireEvent.click(screen.getAllByText('deliveries.requeue')[0])

    await waitFor(() => expect(requests).toContain(`POST /api/v1/outbox-deliveries/${deliveryId}/requeue`))
  })
})
