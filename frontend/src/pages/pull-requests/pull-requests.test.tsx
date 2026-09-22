import { act, fireEvent, render, screen, within } from '@testing-library/react'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { toast } from 'sonner'
import { PullRequestsPage } from './index'
import { PullRequestDetailPage } from '@/pages/pull-request-detail'
import type { PullRequest } from '@/api/types'

const mocks = vi.hoisted(() => ({
  list: vi.fn(),
  detail: vi.fn(),
  refs: vi.fn(),
  comparison: vi.fn(),
  create: vi.fn(),
  action: vi.fn(),
  refetchList: vi.fn(),
  refetchDetail: vi.fn(),
  refetchRefs: vi.fn(),
  refetchDiff: vi.fn(),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: { count?: number; total?: number; number?: number; source?: string; target?: string }) =>
      options?.count !== undefined ? `${key} ${options.count}/${options.total}` :
        options?.number !== undefined ? `${key} #${options.number} ${options.source} ${options.target}` : key,
    i18n: { language: 'en' },
  }),
}))
vi.mock('@/api/hooks', () => ({
  usePullRequests: mocks.list,
  usePullRequest: mocks.detail,
  useRepositoryRefs: mocks.refs,
  useRepositoryComparison: mocks.comparison,
  useCreatePullRequest: () => ({ mutate: mocks.create, isPending: false }),
  usePullRequestAction: () => ({ mutate: mocks.action, isPending: false }),
}))
vi.mock('@/api/auth', () => ({ currentSession: () => ({ username: 'reviewer' }) }))
vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }))

function pullRequest(number: number, status: PullRequest['status'] = 'open'): PullRequest {
  return {
    id: `pr-${number}`, repository_name: 'platform', number, title: `Change ${number}`,
    description: '', source_branch: `feature/${number}`, target_branch: 'main', status,
    created_by: 'reviewer', created_at: '2026-09-19T00:00:00Z', updated_at: '2026-09-19T00:00:00Z',
    merged_at: null, merge_commit_sha: null,
  }
}

function setup(
  pullRequests: PullRequest[],
  path = '/repositories/platform/pulls',
  options: { list?: Error; detail?: Error; diff?: Error; comparison?: unknown } = {},
) {
  mocks.list.mockImplementation((_repo: string, query: { limit?: number; offset?: number; status?: PullRequest['status']; search?: string } = {}) => {
    const search = query.search?.toLocaleLowerCase() ?? ''
    const filtered = [...pullRequests]
      .sort((left, right) => right.number - left.number)
      .filter((pr) => (!query.status || pr.status === query.status) && (
        !search || [String(pr.number), pr.title, pr.source_branch, pr.target_branch, pr.created_by]
          .some((value) => value.toLocaleLowerCase().includes(search))
      ))
    const offset = query.offset ?? 0
    const limit = query.limit ?? 20
    return {
      data: options.list ? undefined : { items: filtered.slice(offset, offset + limit), total: filtered.length, limit, offset },
      isLoading: false,
      isFetching: false,
      isPlaceholderData: false,
      error: options.list ?? null,
      refetch: mocks.refetchList,
    }
  })
  mocks.detail.mockImplementation((_repo: string, number: number) => ({
    data: options.detail ? undefined : pullRequests.find((pr) => pr.number === number),
    isLoading: false,
    error: options.detail ?? null,
    refetch: mocks.refetchDetail,
  }))
  mocks.refs.mockReturnValue({
    data: [
      { name: 'main', kind: 'branch', sha: 'abc', target: '' },
      { name: 'feature/1', kind: 'branch', sha: 'def', target: '' },
      { name: 'v1.0.0', kind: 'tag', sha: 'fed', target: '' },
    ],
    isLoading: false,
    error: null,
    refetch: mocks.refetchRefs,
  })
  mocks.comparison.mockReturnValue({ data: options.comparison ?? null, isLoading: false, isError: Boolean(options.diff), error: options.diff ?? null, refetch: mocks.refetchDiff })
  render(
    <MemoryRouter initialEntries={[path]}>
      <LocationProbe />
      <Routes>
        <Route path="/repositories/:repo/pulls" element={<PullRequestsPage />} />
        <Route path="/repositories/:repo/pulls/:number" element={<PullRequestDetailPage />} />
      </Routes>
    </MemoryRouter>,
  )
}

function LocationProbe() {
  const location = useLocation()
  return <div data-testid="location">{location.search}</div>
}

afterEach(() => {
  vi.clearAllMocks()
  vi.useRealTimers()
})

describe('pull request workflow', () => {
  it('loads compact server pages and keeps search and page state in the URL', () => {
    vi.useFakeTimers()
    setup(Array.from({ length: 21 }, (_, index) => pullRequest(index + 1)))
    expect(screen.getAllByRole('link', { name: /^#\d+ Change/ })).toHaveLength(20)
    expect(screen.getByText('pulls.shown 20/21')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'pulls.next' }))
    expect(screen.getAllByRole('link', { name: /^#\d+ Change/ })).toHaveLength(1)
    expect(screen.getByRole('link', { name: '#1 Change 1' })).toBeInTheDocument()
    expect(screen.getByTestId('location')).toHaveTextContent('?page=2')
    expect(mocks.list).toHaveBeenLastCalledWith('platform', expect.objectContaining({ limit: 20, offset: 20 }))
    fireEvent.change(screen.getByRole('searchbox', { name: 'pulls.search' }), { target: { value: 'feature/7' } })
    act(() => vi.advanceTimersByTime(300))
    expect(screen.getByRole('link', { name: '#7 Change 7' })).toBeInTheDocument()
    expect(screen.getByTestId('location')).toHaveTextContent('?q=feature%2F7')
    expect(screen.queryByRole('navigation', { name: 'pulls.pages' })).not.toBeInTheDocument()
    fireEvent.change(screen.getByRole('searchbox', { name: 'pulls.search' }), { target: { value: 'missing' } })
    act(() => vi.advanceTimersByTime(300))
    expect(screen.getByRole('status')).toHaveTextContent('pulls.noMatches')
  })

  it('requests a server status filter and resets pagination', () => {
    setup([...Array.from({ length: 20 }, (_, index) => pullRequest(index + 1)), pullRequest(21, 'closed')], '/repositories/platform/pulls?page=2')
    fireEvent.change(screen.getByLabelText('pulls.statusFilter'), { target: { value: 'closed' } })
    expect(screen.getByRole('link', { name: '#21 Change 21' })).toBeInTheDocument()
    expect(screen.getByTestId('location')).toHaveTextContent('?status=closed')
    expect(mocks.list).toHaveBeenLastCalledWith('platform', expect.objectContaining({ offset: 0, status: 'closed' }))
    expect(screen.queryByRole('navigation', { name: 'pulls.pages' })).not.toBeInTheDocument()
  })

  it('requires distinct, user-entered branches and closes the form only after create succeeds', () => {
    setup([])
    fireEvent.click(screen.getByRole('button', { name: 'pulls.create' }))
    const form = screen.getByRole('form', { name: 'pulls.create' })
    expect([...document.querySelectorAll<HTMLOptionElement>('#pr-source-refs option')].map((option) => option.value))
      .toEqual(['main', 'feature/1'])
    expect(screen.getByLabelText('pulls.sourceBranch')).toHaveValue('')
    expect(screen.getByLabelText('pulls.targetBranch')).toHaveValue('')
    fireEvent.change(screen.getByLabelText('pulls.titleField'), { target: { value: '  Add cache  ' } })
    fireEvent.change(screen.getByLabelText('pulls.sourceBranch'), { target: { value: 'main' } })
    fireEvent.change(screen.getByLabelText('pulls.targetBranch'), { target: { value: 'main' } })
    expect(screen.getByRole('alert')).toHaveTextContent('pulls.branchesMustDiffer')
    expect(within(form).getByRole('button', { name: 'pulls.create' })).toBeDisabled()
    fireEvent.change(screen.getByLabelText('pulls.sourceBranch'), { target: { value: ' feature/1 ' } })
    fireEvent.submit(form)
    expect(mocks.create).toHaveBeenCalledWith(expect.objectContaining({ title: 'Add cache', source_branch: 'feature/1', target_branch: 'main' }), expect.any(Object))
    expect(form).toBeInTheDocument()
    act(() => mocks.create.mock.calls[0][1].onSuccess())
    expect(screen.queryByRole('form', { name: 'pulls.create' })).not.toBeInTheDocument()
    expect(toast.success).toHaveBeenCalledWith('pulls.created')
  })

  it('confirms merge with branch context and keeps confirmation open after failure', () => {
    setup([pullRequest(1)])
    fireEvent.click(screen.getByRole('button', { name: 'pulls.merge' }))
    const dialog = screen.getByRole('alertdialog')
    expect(dialog).toHaveTextContent('pulls.confirmDescription #1 feature/1 main')
    expect(mocks.action).not.toHaveBeenCalled()
    fireEvent.click(within(dialog).getByRole('button', { name: 'pulls.merge' }))
    expect(mocks.action).toHaveBeenCalledWith({ number: 1, action: 'merge' }, expect.any(Object))
    act(() => mocks.action.mock.calls[0][1].onError(new Error('Pipeline required')))
    expect(dialog).toBeInTheDocument()
    expect(toast.error).toHaveBeenCalledWith('Pipeline required')
    act(() => mocks.action.mock.calls[0][1].onSuccess())
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument()
  })

  it('requires confirmation before closing from the detail page', () => {
    setup([pullRequest(1)], '/repositories/platform/pulls/1')
    fireEvent.click(screen.getByRole('button', { name: 'pulls.close' }))
    expect(mocks.action).not.toHaveBeenCalled()
    fireEvent.click(within(screen.getByRole('alertdialog')).getByRole('button', { name: 'pulls.close' }))
    expect(mocks.action).toHaveBeenCalledWith({ number: 1, action: 'close' }, expect.any(Object))
  })

  it('keeps manual branch entry available when ref suggestions fail', () => {
    mocks.list.mockReturnValue({
      data: { items: [], total: 0, limit: 20, offset: 0 },
      isLoading: false,
      isFetching: false,
      isPlaceholderData: false,
      error: null,
      refetch: mocks.refetchList,
    })
    mocks.refs.mockReturnValue({ data: [], isLoading: false, error: new Error('Unavailable'), refetch: mocks.refetchRefs })
    render(<MemoryRouter initialEntries={['/repositories/platform/pulls']}><Routes><Route path="/repositories/:repo/pulls" element={<PullRequestsPage />} /></Routes></MemoryRouter>)
    fireEvent.click(screen.getByRole('button', { name: 'pulls.create' }))
    expect(screen.getByRole('alert')).toHaveTextContent('pulls.refsUnavailable')
    expect(screen.getByLabelText('pulls.sourceBranch')).not.toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(mocks.refetchRefs).toHaveBeenCalled()
  })

  it('distinguishes a detail load error from a missing pull request and retries', () => {
    setup([], '/repositories/platform/pulls/1', { detail: new Error('Unavailable') })
    expect(screen.getByRole('alert')).toHaveTextContent('Unavailable')
    expect(screen.queryByText('pulls.notFound')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(mocks.refetchDetail).toHaveBeenCalled()
  })

  it('retries a failed diff request without losing the pull request context', () => {
    setup([pullRequest(1)], '/repositories/platform/pulls/1?view=diff', { diff: new Error('Diff unavailable') })
    expect(screen.getByRole('alert')).toHaveTextContent('Diff unavailable')
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(mocks.refetchDiff).toHaveBeenCalled()
    expect(screen.getByRole('link', { name: '#1' })).toHaveAttribute('href', '/repositories/platform/pulls/1')
  })

  it('labels binary changes instead of presenting invented line counts', () => {
    setup([pullRequest(1)], '/repositories/platform/pulls/1?view=diff', {
      comparison: {
        from: 'main',
        to: 'feature/1',
        merge_base: 'abc123',
        patch: '',
        files: [
          {
            path: 'assets/screenshot.png',
            status: 'modified',
            additions: 0,
            deletions: 0,
            binary: true,
          },
        ],
      },
    })

    const file = screen.getByText('assets/screenshot.png').closest('li')
    expect(file).toHaveTextContent('compare.binaryFile')
    expect(screen.queryByText('+0')).not.toBeInTheDocument()
    expect(screen.queryByText('−0')).not.toBeInTheDocument()
  })
})
