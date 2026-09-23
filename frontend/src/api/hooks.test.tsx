import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const clientMocks = vi.hoisted(() => ({
  api: vi.fn(),
  apiRetry: vi.fn(),
  authenticatedFetch: vi.fn(),
}))

vi.mock('./client', () => clientMocks)

import { useNotificationEvents, useRepositoryRefs, useRepositoryTags } from './hooks'

const projectId = '22222222-2222-4222-8222-222222222222'
const originalUserAgent = window.navigator.userAgent

function NotificationEventsProbe() {
  useNotificationEvents(projectId)
  return null
}

function RepositoryRefsProbe() {
  useRepositoryRefs('demo', { limit: 101, offset: 200, search: ' release ', kind: 'branch' })
  useRepositoryTags('demo', { limit: 51, offset: 100, search: ' v1 ' })
  return null
}

describe('notification event stream', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    Object.defineProperty(window.navigator, 'userAgent', {
      configurable: true,
      value: 'Mozilla/5.0',
    })
    clientMocks.api.mockResolvedValue([])
    clientMocks.authenticatedFetch.mockResolvedValue(new Response('expired', { status: 401 }))
  })

  afterEach(() => {
    Object.defineProperty(window.navigator, 'userAgent', {
      configurable: true,
      value: originalUserAgent,
    })
  })

  it('[REQ-NOTIFY-401] routes an unauthorized stream through the terminal auth transport', async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(
      <QueryClientProvider client={queryClient}>
        <NotificationEventsProbe />
      </QueryClientProvider>,
    )

    await waitFor(() => expect(clientMocks.authenticatedFetch).toHaveBeenCalledWith(
      `/projects/${projectId}/notifications/stream`,
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    ))
  })
})

describe('repository ref queries', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    clientMocks.api.mockResolvedValue([])
  })

  it('sends bounded kind, search, and offset parameters', async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(
      <QueryClientProvider client={queryClient}>
        <RepositoryRefsProbe />
      </QueryClientProvider>,
    )

    await waitFor(() => {
      expect(clientMocks.api).toHaveBeenCalledWith('/repos/demo/refs?limit=101&offset=200&search=release&kind=branch')
      expect(clientMocks.api).toHaveBeenCalledWith('/repos/demo/tags?limit=51&offset=100&search=v1')
    })
  })
})
