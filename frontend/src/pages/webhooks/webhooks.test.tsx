import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen } from '@testing-library/react'
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

function renderWebhooksPage(requests: string[]) {
  vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL) => {
    const url = typeof input === 'string' ? input : input.toString()
    requests.push(url)
    if (url === `/api/v1/projects/${projectId}/webhooks`) {
      return json([])
    }
    if (url === `/api/v1/projects/${projectId}/notifications`) {
      return json([{ id: 'n1', channel: 'in_app', target: 'dashboard', enabled: true }])
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
  it('shows delivered in-app notification events', async () => {
    const requests: string[] = []
    renderWebhooksPage(requests)

    expect(await screen.findByText('Pipeline failed')).toBeInTheDocument()
    expect(screen.getByText('in_app / dashboard')).toBeInTheDocument()
    expect(screen.getByText('notifications.delivered')).toBeInTheDocument()
    expect(requests).toContain(`/api/v1/projects/${projectId}/notification-events?limit=20`)
  })
})
