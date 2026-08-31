import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { SchedulesPage } from './index'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

vi.mock('sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}))

const projectId = '22222222-2222-4222-8222-222222222222'

function renderSchedulesPage(requests: string[]) {
  vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const url = typeof input === 'string' ? input : input.toString()
    requests.push(`${init?.method ?? 'GET'} ${url}`)
    if (url === `/api/v1/projects/${projectId}/schedules`) {
      return json([
        {
          id: '33333333-3333-4333-8333-333333333333',
          project_id: projectId,
          cron: '0 4 * * *',
          git_ref: 'main',
          enabled: true,
          next_fire_at: '2026-09-01T04:00:00Z',
          last_fired_at: '2026-08-31T04:00:00Z',
          last_fire_error: null,
          created_at: '2026-08-30T10:00:00Z',
        },
        {
          id: '44444444-4444-4444-8444-444444444444',
          project_id: projectId,
          cron: 'bad cron',
          git_ref: 'main',
          enabled: true,
          next_fire_at: null,
          last_fired_at: null,
          last_fire_error: 'cron must have five fields',
          created_at: '2026-08-30T10:00:00Z',
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
      <MemoryRouter initialEntries={[`/projects/${projectId}/schedules`]}>
        <Routes>
          <Route path="/projects/:projectId/schedules" element={<SchedulesPage />} />
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

describe('SchedulesPage', () => {
  it('shows next fire, last fire and scheduler errors', async () => {
    const requests: string[] = []
    renderSchedulesPage(requests)

    expect(await screen.findByText('0 4 * * *')).toBeInTheDocument()
    expect(screen.getByText('schedules.nextFire')).toBeInTheDocument()
    expect(screen.getByText('schedules.lastFire')).toBeInTheDocument()
    expect(screen.getByText('bad cron')).toBeInTheDocument()
    expect(screen.getByTitle('cron must have five fields')).toHaveTextContent('schedules.error')
    expect(requests).toContain(`GET /api/v1/projects/${projectId}/schedules`)
  })
})
