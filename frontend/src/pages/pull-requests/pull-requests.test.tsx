import { act, fireEvent, render, screen, within } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { toast } from 'sonner'
import { PullRequestsPage } from './index'
import { PullRequestDetailPage } from '@/pages/pull-request-detail'
import type { PullRequest } from '@/api/types'

const mocks = vi.hoisted(() => ({
  list: vi.fn(),
  refs: vi.fn(),
  comparison: vi.fn(),
  create: vi.fn(),
  action: vi.fn(),
  refetchList: vi.fn(),
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
  options: { list?: Error; diff?: Error; comparison?: unknown } = {},
) {
  mocks.list.mockReturnValue({ data: pullRequests, isLoading: false, error: options.list ?? null, refetch: mocks.refetchList })
  mocks.refs.mockReturnValue({ data: [{ name: 'main', sha: 'abc', target: '' }, { name: 'feature/1', sha: 'def', target: '' }], isLoading: false, error: null, refetch: mocks.refetchRefs })
  mocks.comparison.mockReturnValue({ data: options.comparison ?? null, isLoading: false, isError: Boolean(options.diff), error: options.diff ?? null, refetch: mocks.refetchDiff })
  render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route path="/repositories/:repo/pulls" element={<PullRequestsPage />} />
        <Route path="/repositories/:repo/pulls/:number" element={<PullRequestDetailPage />} />
      </Routes>
    </MemoryRouter>,
  )
}

afterEach(() => vi.clearAllMocks())

describe('pull request workflow', () => {
  it('shows compact filtered pages and an explicit no-matches state', () => {
    setup(Array.from({ length: 21 }, (_, index) => pullRequest(index + 1)))
    expect(screen.getAllByRole('link', { name: /^#\d+ Change/ })).toHaveLength(20)
    fireEvent.click(screen.getByRole('button', { name: 'pulls.next' }))
    expect(screen.getAllByRole('link', { name: /^#\d+ Change/ })).toHaveLength(1)
    fireEvent.change(screen.getByRole('searchbox', { name: 'pulls.search' }), { target: { value: 'feature/7' } })
    expect(screen.getByRole('link', { name: '#7 Change 7' })).toBeInTheDocument()
    expect(screen.queryByRole('navigation', { name: 'pulls.pages' })).not.toBeInTheDocument()
    fireEvent.change(screen.getByRole('searchbox', { name: 'pulls.search' }), { target: { value: 'missing' } })
    expect(screen.getByRole('status')).toHaveTextContent('pulls.noMatches')
  })

  it('filters by status and resets pagination when the filter changes', () => {
    setup([...Array.from({ length: 20 }, (_, index) => pullRequest(index + 1)), pullRequest(21, 'closed')])
    fireEvent.click(screen.getByRole('button', { name: 'pulls.next' }))
    fireEvent.change(screen.getByLabelText('pulls.statusFilter'), { target: { value: 'closed' } })
    expect(screen.getByRole('link', { name: '#21 Change 21' })).toBeInTheDocument()
    expect(screen.queryByRole('navigation', { name: 'pulls.pages' })).not.toBeInTheDocument()
  })

  it('requires distinct, user-entered branches and closes the form only after create succeeds', () => {
    setup([])
    fireEvent.click(screen.getByRole('button', { name: 'pulls.create' }))
    const form = screen.getByRole('form', { name: 'pulls.create' })
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
    mocks.list.mockReturnValue({ data: [], isLoading: false, error: null, refetch: mocks.refetchList })
    mocks.refs.mockReturnValue({ data: [], isLoading: false, error: new Error('Unavailable'), refetch: mocks.refetchRefs })
    render(<MemoryRouter initialEntries={['/repositories/platform/pulls']}><Routes><Route path="/repositories/:repo/pulls" element={<PullRequestsPage />} /></Routes></MemoryRouter>)
    fireEvent.click(screen.getByRole('button', { name: 'pulls.create' }))
    expect(screen.getByRole('alert')).toHaveTextContent('pulls.refsUnavailable')
    expect(screen.getByLabelText('pulls.sourceBranch')).not.toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(mocks.refetchRefs).toHaveBeenCalled()
  })

  it('distinguishes a detail load error from a missing pull request and retries', () => {
    setup([], '/repositories/platform/pulls/1', { list: new Error('Unavailable') })
    expect(screen.getByRole('alert')).toHaveTextContent('Unavailable')
    expect(screen.queryByText('pulls.notFound')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(mocks.refetchList).toHaveBeenCalled()
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
        patch: '+bounded change',
        patch_truncated: true,
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
    expect(screen.getByRole('status')).toHaveTextContent('compare.patchTruncated')
    expect(screen.getByLabelText('compare.patch')).toHaveAttribute('tabindex', '0')
  })
})
