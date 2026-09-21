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
const webhookId = '55555555-5555-4555-8555-555555555555'
const webhookUrl = 'https://example.invalid/hooks/pipeline'

function renderWebhooksPage(requests: string[], options: {
  initialView?: 'webhooks' | 'deliveries' | 'notifications'
  failWebhookLoadOnce?: boolean
  failDeliveryLoadOnce?: boolean
  failDetailLoadOnce?: boolean
  failEventsLoadOnce?: boolean
  failDeleteOnce?: boolean
  writes?: string[]
  notificationGetError?: boolean
} = {}) {
  let webhookReads = 0
  let deliveryReads = 0
  let detailReads = 0
  let eventsReads = 0
  let deleteRequests = 0
  vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const url = typeof input === 'string' ? input : input.toString()
    requests.push(`${init?.method ?? 'GET'} ${url}`)
    if (url === `/api/v1/projects/${projectId}/webhooks`) {
      webhookReads++
      if (options.failWebhookLoadOnce && webhookReads === 1) return failure()
      return json([{
        id: webhookId, project_id: projectId, url: webhookUrl,
        events: ['pipeline.started', 'pipeline.finished'], enabled: true,
        created_at: '2026-08-31T12:00:00Z',
      }])
    }
    if (url === `/api/v1/webhooks/${webhookId}` && init?.method === 'DELETE') {
      deleteRequests++
      if (options.failDeleteOnce && deleteRequests === 1) return failure()
      return json({ deleted: webhookId })
    }
    if (url === `/api/v1/projects/${projectId}/outbox-deliveries?limit=20`) {
      deliveryReads++
      if (options.failDeliveryLoadOnce && deliveryReads === 1) return failure()
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
      detailReads++
      if (options.failDetailLoadOnce && detailReads === 1) return failure()
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
      eventsReads++
      if (options.failEventsLoadOnce && eventsReads === 1) return failure()
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
      <MemoryRouter initialEntries={[`/projects/${projectId}/webhooks${options.initialView ? `?view=${options.initialView}` : ''}`]}>
        <Routes>
          <Route path="/projects/:projectId/webhooks" element={<WebhooksPage />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

function failure(): Promise<Response> {
  return Promise.resolve(new Response('unavailable', { status: 503 }))
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

describe('WebhooksPage', () => {
  it('opens URL-backed views and shows notification events', async () => {
    const requests: string[] = []
    renderWebhooksPage(requests)

    expect(await screen.findAllByText(webhookUrl)).toHaveLength(2)
    expect(requests).not.toContain(`GET /api/v1/projects/${projectId}/notification-events?limit=20`)
    fireEvent.mouseDown(screen.getByRole('tab', { name: 'notifications.title' }), { button: 0 })

    expect(await screen.findAllByText('Pipeline failed')).toHaveLength(2)
    expect(screen.getAllByText('in_app / dashboard')).toHaveLength(2)
    expect(screen.getAllByText('notifications.delivered')).toHaveLength(2)
    expect(requests).toContain(`GET /api/v1/projects/${projectId}/notification-events?limit=20`)
    fireEvent.mouseDown(screen.getByRole('tab', { name: 'deliveries.title' }), { button: 0 })
    expect(await screen.findAllByRole('button', { name: 'deliveries.openDetails' })).toHaveLength(2)
    expect(requests).toContain(`GET /api/v1/projects/${projectId}/outbox-deliveries?limit=20`)
  })

  it('opens delivery details through a real button and requeues a failure', async () => {
    const requests: string[] = []
    renderWebhooksPage(requests, { initialView: 'deliveries' })

    const open = (await screen.findAllByRole('button', { name: 'deliveries.openDetails' }))[0]
    expect(open).toHaveAttribute('aria-expanded', 'false')
    fireEvent.click(open)
    expect(open).toHaveAttribute('aria-expanded', 'true')
    expect(await screen.findByText('deliveries.details')).toBeInTheDocument()
    expect(requests).toContain(`GET /api/v1/outbox-deliveries/${deliveryId}`)

    fireEvent.click(screen.getAllByRole('button', { name: 'deliveries.requeue' })[0])
    await waitFor(() => expect(requests).toContain(`POST /api/v1/outbox-deliveries/${deliveryId}/requeue`))
  })

  it('shows retry when the webhook list fails', async () => {
    renderWebhooksPage([], { failWebhookLoadOnce: true })

    expect(await screen.findByText('webhooks.loadFailed')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(await screen.findAllByText(webhookUrl)).toHaveLength(2)
  })

  it('shows retry for delivery list and detail failures', async () => {
    const requests: string[] = []
    renderWebhooksPage(requests, { initialView: 'deliveries', failDeliveryLoadOnce: true, failDetailLoadOnce: true })

    expect(await screen.findByText('deliveries.loadFailed')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    fireEvent.click((await screen.findAllByRole('button', { name: 'deliveries.openDetails' }))[0])
    expect(await screen.findByText('deliveries.detailFailed')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    await waitFor(() => expect(requests.filter(request => request === `GET /api/v1/outbox-deliveries/${deliveryId}`)).toHaveLength(2))
  })

  it('shows retry when notification events fail', async () => {
    renderWebhooksPage([], { initialView: 'notifications', failEventsLoadOnce: true })

    expect(await screen.findByText('notifications.eventsFailed')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(await screen.findAllByText('Pipeline failed')).toHaveLength(2)
  })

  it('keeps deletion confirmation open on failure and allows retry', async () => {
    const requests: string[] = []
    renderWebhooksPage(requests, { failDeleteOnce: true })

    fireEvent.click((await screen.findAllByRole('button', { name: `common.delete ${webhookUrl}` }))[0])
    expect(await screen.findByText('webhooks.deleteConfirm')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'common.delete' }))
    expect(await screen.findByText('webhooks.deleteFailed')).toBeInTheDocument()
    expect(screen.getByRole('alertdialog')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'common.delete' }))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
    expect(requests.filter(request => request === `DELETE /api/v1/webhooks/${webhookId}`)).toHaveLength(2)
  })
})
