import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { UsersPage } from './index'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    i18n: { language: 'ru' },
    t: (key: string) => key,
  }),
}))

vi.mock('sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}))

const projectId = '22222222-2222-4222-8222-222222222222'

function renderUsersPage(requests: Array<{ method: string; url: string; body?: unknown }>) {
  vi.stubGlobal('fetch', vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const url = typeof input === 'string' ? input : input.toString()
    const method = init?.method ?? 'GET'
    requests.push({
      method,
      url,
      body: init?.body ? JSON.parse(String(init.body)) : undefined,
    })

    if (url === '/api/v1/users' && method === 'GET') return json([])
    if (url === '/api/v1/users' && method === 'POST') {
      return json({
        id: '88888888-8888-4888-8888-888888888888',
        username: 'release-admin',
        role: 'developer',
        enabled: true,
        created_at: '2026-09-02T12:00:00Z',
      })
    }
    if (url === '/api/v1/api-tokens') return json([])
    if (url === '/api/v1/projects') {
      return json([{
        id: projectId,
        name: 'forge-api',
        repository_url: 'https://git.example.com/forge-api.git',
        default_branch: 'main',
        created_at: '2026-09-02T12:00:00Z',
      }])
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
      <UsersPage />
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

describe('UsersPage credentials', () => {
  it('[REQ-AUTH-001] sends the optional password when creating an interactive user', async () => {
    const requests: Array<{ method: string; url: string; body?: unknown }> = []
    renderUsersPage(requests)

    fireEvent.click(screen.getByRole('button', { name: 'users.create' }))
    fireEvent.change(screen.getByLabelText('users.username'), { target: { value: ' release-admin ' } })
    fireEvent.change(screen.getByLabelText('users.role'), { target: { value: 'developer' } })
    fireEvent.change(screen.getByLabelText('users.password'), { target: { value: 'ChangeMe-2026!' } })
    fireEvent.click(screen.getAllByRole('button', { name: 'users.create' }).at(-1)!)

    await waitFor(() => {
      expect(requests).toContainEqual({
        method: 'POST',
        url: '/api/v1/users',
        body: {
          username: 'release-admin',
          role: 'developer',
          password: 'ChangeMe-2026!',
        },
      })
    })
  })
})
