import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const clientMocks = vi.hoisted(() => ({
  api: vi.fn(),
  apiRetry: vi.fn(),
  authenticatedFetch: vi.fn(),
}))

vi.mock('./client', () => clientMocks)

import {
  useNotificationEvents,
  usePullRequest,
  usePullRequests,
  useRepositoryCommits,
  useRepositoryTree,
} from './hooks'

const projectId = '22222222-2222-4222-8222-222222222222'
const originalUserAgent = window.navigator.userAgent

function NotificationEventsProbe() {
  useNotificationEvents(projectId)
  return null
}

function RepositoryTreeProbe() {
  useRepositoryTree('demo', 'refs/heads/main', 'src', { limit: 101, offset: 200, search: ' release ' })
  return null
}

function RepositoryCommitsProbe() {
  useRepositoryCommits('platform core', 'refs/heads/release/v1', 2, 50)
  return null
}

function PullRequestsProbe() {
  usePullRequests('platform core', {
    limit: 25,
    offset: 50,
    status: 'closed',
    search: ' release branch ',
  })
  usePullRequest('platform core', 7)
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
describe('repository tree query', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    clientMocks.api.mockResolvedValue([])
  })

  it('sends bounded pagination and trimmed search parameters', async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(
      <QueryClientProvider client={queryClient}>
        <RepositoryTreeProbe />
      </QueryClientProvider>,
    )

    await waitFor(() => expect(clientMocks.api).toHaveBeenCalledWith(
      '/repos/demo/tree?ref=refs%2Fheads%2Fmain&path=src&limit=101&offset=200&search=release',
    ))
  })
})

describe('repository commit queries', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    clientMocks.api.mockResolvedValue([])
  })

  it('requests one look-ahead row at the requested history offset', async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(
      <QueryClientProvider client={queryClient}>
        <RepositoryCommitsProbe />
      </QueryClientProvider>,
    )

    await waitFor(() => expect(clientMocks.api).toHaveBeenCalledWith(
      '/repos/platform%20core/commits?branch=refs%2Fheads%2Frelease%2Fv1&limit=51&offset=50',
    ))
  })
})

describe('pull request queries', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    clientMocks.api.mockImplementation((path: string) => Promise.resolve(
      path.includes('/pulls/page')
        ? { items: [], total: 0, limit: 25, offset: 50 }
        : { id: 'pr-7', number: 7 },
    ))
  })

  it('uses the paged list contract and a dedicated detail endpoint', async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(
      <QueryClientProvider client={queryClient}>
        <PullRequestsProbe />
      </QueryClientProvider>,
    )

    await waitFor(() => expect(clientMocks.api).toHaveBeenCalledTimes(2))
    expect(clientMocks.api).toHaveBeenCalledWith(
      '/repos/platform%20core/pulls/page?limit=25&offset=50&status=closed&search=release+branch',
    )
    expect(clientMocks.api).toHaveBeenCalledWith('/repos/platform%20core/pulls/7')
  })
})
