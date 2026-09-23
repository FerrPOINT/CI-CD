import { act, fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { toast } from 'sonner'
import { RepositoryBrowserPage } from './index'

const mocks = vi.hoisted(() => ({
  refs: vi.fn(),
  commits: vi.fn(),
  tree: vi.fn(),
  blob: vi.fn(),
  tags: vi.fn(),
  releases: vi.fn(),
  refetchRefs: vi.fn(),
  refetchCommits: vi.fn(),
  refetchTree: vi.fn(),
  refetchBlob: vi.fn(),
  refetchTags: vi.fn(),
  createRelease: vi.fn(),
  deleteRelease: vi.fn(),
  create: vi.fn(),
  remove: vi.fn(),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: 'en' } }),
}))
vi.mock('@/api/hooks', () => ({
  useRepositoryRefs: mocks.refs,
  useRepositoryCommits: mocks.commits,
  useRepositoryTree: mocks.tree,
  useRepositoryBlob: mocks.blob,
  useRepositoryTags: mocks.tags,
  useReleases: mocks.releases,
  useCreateRelease: mocks.createRelease,
  useDeleteRelease: mocks.deleteRelease,
}))
vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }))

function result<T>(data: T, refetch = vi.fn()) {
  return { data, isLoading: false, error: null, refetch }
}

function Location() {
  const location = useLocation()
  return <output data-testid="location">{location.pathname}{location.search}</output>
}

function setup(path = '/repositories/demo') {
  render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route path="/repositories/:repo" element={<><RepositoryBrowserPage /><Location /></>} />
      </Routes>
    </MemoryRouter>,
  )
}

function release() {
  return {
    id: 'release-1', repository_name: 'demo', tag_name: 'v1.0.0', name: 'Version 1',
    description: 'First release', prerelease: false, created_by: null, created_at: '2026-09-19T00:00:00Z',
  }
}

beforeEach(() => {
  mocks.refs.mockReturnValue(result([
    { name: 'main', kind: 'branch', sha: 'abc123456', target: '' },
    { name: 'v1', kind: 'tag', sha: 'def123456', target: '' },
  ], mocks.refetchRefs))
  mocks.tree.mockReturnValue(result([], mocks.refetchTree))
  mocks.blob.mockReturnValue(result({ path: 'empty.txt', sha: 'abc123456', size: 0, content: '', binary: false, truncated: false }, mocks.refetchBlob))
  mocks.commits.mockReturnValue(result({ items: [], hasMore: false }, mocks.refetchCommits))
  mocks.tags.mockReturnValue(result([], mocks.refetchTags))
  mocks.releases.mockReturnValue(result([]))
  mocks.createRelease.mockReturnValue({ mutate: mocks.create, isPending: false })
  mocks.deleteRelease.mockReturnValue({ mutate: mocks.remove, isPending: false })
})

afterEach(() => vi.clearAllMocks())

describe('RepositoryBrowserPage', () => {
  it('opens code by default and keeps nested directories and files in the URL', () => {
    mocks.tree.mockImplementation((_repo: string, _ref: string, dir?: string) => result(dir === 'src'
      ? [{ path: 'src/index.tsx', name: 'index.tsx', kind: 'blob', sha: 'def123456', size: 12 }]
      : [{ path: 'src', name: 'src', kind: 'tree', sha: 'abc123456', size: null }]))
    setup()

    expect(screen.getByRole('tab', { name: 'repositoryBrowser.code' })).toHaveAttribute('data-state', 'active')
    expect(screen.getByRole('link', { name: 'navigation.repositories' })).toHaveClass('min-h-10')
    expect(screen.getByRole('link', { name: 'repositoryBrowser.compareChanges' })).toHaveClass('h-10')
    expect(screen.getByRole('link', { name: 'repositoryBrowser.createPullRequest' })).toHaveClass('h-10')
    expect(screen.getByRole('button', { name: '/' })).toHaveClass('min-h-10', 'min-w-10')
    fireEvent.click(screen.getByRole('button', { name: 'src' }))
    expect(screen.getByTestId('location')).toHaveTextContent('dir=src')
    expect(mocks.tree).toHaveBeenLastCalledWith('demo', 'HEAD', 'src', { limit: 101, offset: 0, search: '' })
    fireEvent.click(screen.getByRole('button', { name: 'index.tsx' }))
    expect(screen.getByTestId('location')).toHaveTextContent('file=src%2Findex.tsx')
    expect(mocks.blob).toHaveBeenCalledWith('demo', 'HEAD', 'src/index.tsx')
    expect(screen.getByRole('button', { name: 'repositoryBrowser.backToTree' })).toBeInTheDocument()
  })

  it('bounds large directories with URL pagination and server-side search', () => {
    mocks.tree.mockReturnValue(result(Array.from({ length: 101 }, (_, index) => ({
      path: `file-${String(index + 1).padStart(4, '0')}.txt`,
      name: `file-${String(index + 1).padStart(4, '0')}.txt`,
      kind: 'blob',
      sha: `sha-${index}`,
      size: index,
    }))))
    setup('/repositories/demo?treePage=2&treeSearch=release')

    expect(mocks.tree).toHaveBeenLastCalledWith('demo', 'HEAD', undefined, { limit: 101, offset: 100, search: 'release' })
    expect(screen.getByText('file-0100.txt')).toBeInTheDocument()
    expect(screen.queryByText('file-0101.txt')).not.toBeInTheDocument()
    expect(screen.getByText('repositoryBrowser.treePage')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'repositoryBrowser.nextTreePage' }))
    expect(screen.getByTestId('location')).toHaveTextContent('treePage=3')
    expect(mocks.tree).toHaveBeenLastCalledWith('demo', 'HEAD', undefined, { limit: 101, offset: 200, search: 'release' })

    fireEvent.change(screen.getByRole('searchbox', { name: 'repositoryBrowser.searchTree' }), { target: { value: 'hotfix' } })
    fireEvent.click(screen.getByRole('button', { name: 'repositoryBrowser.applyTreeSearch' }))
    expect(screen.getByTestId('location')).toHaveTextContent('treeSearch=hotfix')
    expect(screen.getByTestId('location')).not.toHaveTextContent('treePage=')
    expect(mocks.tree).toHaveBeenLastCalledWith('demo', 'HEAD', undefined, { limit: 101, offset: 0, search: 'hotfix' })
  })

  it('opens a deep-linked empty file without a tree request or endless loading state', () => {
    setup('/repositories/demo?file=src%2Fempty.txt&ref=refs%2Fheads%2Fmain')

    expect(mocks.tree).not.toHaveBeenCalled()
    expect(mocks.blob).toHaveBeenCalledWith('demo', 'refs/heads/main', 'src/empty.txt')
    expect(screen.getByText('repositoryBrowser.emptyFile')).toBeInTheDocument()
    expect(screen.queryByText('common.loading')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'repositoryBrowser.backToTree' }))
    expect(screen.getByTestId('location')).toHaveTextContent('dir=src')
    expect(screen.getByTestId('location')).not.toHaveTextContent('file=')
  })

  it('keeps code usable when refs fail, and retries a file error in place', () => {
    mocks.refs.mockReturnValue({ ...result(undefined, mocks.refetchRefs), error: new Error('offline') })
    mocks.blob.mockReturnValue({ ...result(undefined, mocks.refetchBlob), error: new Error('missing') })
    setup('/repositories/demo?file=README.md')

    expect(screen.getAllByRole('alert')[0]).toHaveTextContent('repositoryBrowser.refsLoadError')
    expect(screen.getByText('repositoryBrowser.fileLoadError')).toBeInTheDocument()
    fireEvent.click(screen.getAllByRole('button', { name: 'common.retry' })[1])
    expect(mocks.refetchBlob).toHaveBeenCalledOnce()
  })

  it('distinguishes binary and truncated file previews', () => {
    mocks.blob.mockReturnValue(result({ path: 'image.png', sha: 'abc123456', size: 40, content: '', binary: true, truncated: false }))
    setup('/repositories/demo?file=image.png')
    expect(screen.getByText('repositoryBrowser.binaryFile')).toBeInTheDocument()
  })

  it('warns when the displayed text is truncated', () => {
    mocks.blob.mockReturnValue(result({ path: 'large.txt', sha: 'abc123456', size: 600000, content: 'partial text', binary: false, truncated: true }))
    setup('/repositories/demo?file=large.txt')
    expect(screen.getByText('repositoryBrowser.truncatedFile')).toHaveAttribute('role', 'status')
    expect(screen.getByText('partial text')).toBeInTheDocument()
    expect(screen.getByLabelText('repositoryBrowser.filePreview')).toHaveAttribute('tabindex', '0')
    expect(screen.getByText(/585.9 KiB/)).toBeInTheDocument()
  })

  it('retries a tree error without hiding the other tabs', () => {
    mocks.tree.mockReturnValue({ ...result(undefined, mocks.refetchTree), error: new Error('offline') })
    setup()
    expect(screen.getByText('repositoryBrowser.codeLoadError')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(mocks.refetchTree).toHaveBeenCalledOnce()
    expect(screen.getByRole('tab', { name: 'repositoryBrowser.branches' })).toBeInTheDocument()
  })

  it('shows only branches in the branch tab and opens the selected ref', () => {
    setup('/repositories/demo?tab=branches')

    expect(screen.getByRole('button', { name: 'main' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'v1' })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'main' }))
    expect(screen.getByTestId('location')).toHaveTextContent('ref=refs%2Fheads%2Fmain')
    expect(screen.getByRole('tab', { name: 'repositoryBrowser.code' })).toHaveAttribute('data-state', 'active')
    expect(mocks.tree).toHaveBeenLastCalledWith('demo', 'refs/heads/main', undefined, { limit: 101, offset: 0, search: '' })
  })

  it('isolates commits and tags errors from the code tab', async () => {
    mocks.commits.mockReturnValue({ ...result(undefined, mocks.refetchCommits), error: new Error('offline') })
    mocks.tags.mockReturnValue({ ...result(undefined, mocks.refetchTags), error: new Error('offline') })
    setup()

    fireEvent.mouseDown(screen.getByRole('tab', { name: 'repositoryBrowser.commits' }), { button: 0 })
    expect(screen.getByTestId('location')).toHaveTextContent('tab=commits')
    expect(screen.getByRole('tab', { name: 'repositoryBrowser.commits' })).toHaveAttribute('data-state', 'active')
    expect(await screen.findByText('repositoryBrowser.commitsLoadError')).toBeInTheDocument()
    expect(mocks.commits).toHaveBeenCalledWith('demo', 'HEAD', 1, 25)
    fireEvent.mouseDown(screen.getByRole('tab', { name: 'repositoryBrowser.tags' }), { button: 0 })
    expect(await screen.findByText('repositoryBrowser.tagsLoadError')).toBeInTheDocument()
    fireEvent.mouseDown(screen.getByRole('tab', { name: 'repositoryBrowser.code' }), { button: 0 })
    expect(screen.getByText('repositoryBrowser.emptyTree')).toBeInTheDocument()
  })

  it('falls back to the first commit page when the URL offset exceeds the API range', () => {
    mocks.commits.mockReturnValue(result({ items: [], hasMore: false }, mocks.refetchCommits))
    setup('/repositories/demo?tab=commits&commitPage=9007199254740991')

    expect(mocks.commits).toHaveBeenLastCalledWith('demo', 'HEAD', 1, 25)
  })

  it('opens commit history beyond the first 50 rows and keeps the page in the URL', () => {
    const commits = Array.from({ length: 51 }, (_, index) => ({
      sha: String(index + 1).padStart(40, '0'),
      short_sha: String(index + 1).padStart(7, '0'),
      author: 'Reviewer',
      email: 'reviewer@example.test',
      message: `Commit ${index + 1}`,
      date: '2026-09-22T12:00:00Z',
    }))
    mocks.commits.mockImplementation((_repo: string, _ref: string, page: number) => result({
      items: commits.slice((page - 1) * 25, page * 25),
      hasMore: page * 25 < commits.length,
    }, mocks.refetchCommits))
    setup('/repositories/demo?tab=commits')

    expect(screen.getByText('Commit 1')).toBeInTheDocument()
    expect(screen.queryByText('Commit 51')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'repositoryBrowser.nextCommits' }))
    expect(screen.getByTestId('location')).toHaveTextContent('tab=commits&commitPage=2')
    expect(mocks.commits).toHaveBeenLastCalledWith('demo', 'HEAD', 2, 25)
    expect(screen.getByText('Commit 50')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'repositoryBrowser.nextCommits' }))
    expect(screen.getByTestId('location')).toHaveTextContent('tab=commits&commitPage=3')
    expect(mocks.commits).toHaveBeenLastCalledWith('demo', 'HEAD', 3, 25)
    expect(screen.getByText('Commit 51')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'repositoryBrowser.previousCommits' })).toBeEnabled()
    expect(screen.getByRole('button', { name: 'repositoryBrowser.nextCommits' })).toBeDisabled()
  })

  it('retries a failed older commit page or returns to the loaded range', () => {
    mocks.commits.mockReturnValue({ ...result(undefined, mocks.refetchCommits), error: new Error('offline') })
    setup('/repositories/demo?tab=commits&commitPage=2')

    expect(screen.getByRole('alert')).toHaveTextContent('repositoryBrowser.commitsLoadError')
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(mocks.refetchCommits).toHaveBeenCalledOnce()
    fireEvent.click(screen.getByRole('button', { name: 'repositoryBrowser.previousCommits' }))
    expect(screen.getByTestId('location')).toHaveTextContent('?tab=commits')
    expect(screen.getByTestId('location')).not.toHaveTextContent('commitPage')
  })

  it('prevents blank or duplicate release tags and keeps the form open on API failure', () => {
    mocks.releases.mockReturnValue(result([release()]))
    mocks.tags.mockReturnValue(result([{ name: 'v1.0.0', sha: 'abc123456', message: '' }]))
    setup('/repositories/demo?tab=releases')
    fireEvent.click(screen.getByRole('button', { name: 'releases.create' }))
    const form = screen.getByRole('form', { name: 'releases.create' })
    const tag = screen.getByLabelText('releases.tag')

    fireEvent.change(tag, { target: { value: '   ' } })
    fireEvent.submit(form)
    expect(screen.getByText('releases.tagRequired')).toBeInTheDocument()
    fireEvent.change(tag, { target: { value: ' v1.0.0 ' } })
    fireEvent.submit(form)
    expect(screen.getByText('releases.alreadyExists')).toBeInTheDocument()
    expect(mocks.create).not.toHaveBeenCalled()

    fireEvent.change(tag, { target: { value: ' v2.0.0 ' } })
    fireEvent.submit(form)
    expect(mocks.create).toHaveBeenCalledWith(
      { tag_name: 'v2.0.0', name: 'v2.0.0', description: '', prerelease: false },
      { onSuccess: expect.any(Function), onError: expect.any(Function) },
    )
    act(() => mocks.create.mock.calls[0][1].onError(new Error('Unavailable')))
    expect(form).toBeInTheDocument()
    expect(toast.error).toHaveBeenCalledWith('Unavailable')
    act(() => mocks.create.mock.calls[0][1].onSuccess())
    expect(screen.queryByRole('form', { name: 'releases.create' })).not.toBeInTheDocument()
    expect(toast.success).toHaveBeenCalledWith('releases.created')
  })

  it('edits existing release metadata without changing its tag', () => {
    mocks.releases.mockReturnValue(result([release()]))
    setup('/repositories/demo?tab=releases')
    fireEvent.click(screen.getByRole('button', { name: 'releases.edit v1.0.0' }))
    const form = screen.getByRole('form', { name: 'releases.edit' })
    expect(screen.getByLabelText('releases.tag')).toHaveAttribute('readonly')
    expect(screen.getByLabelText('releases.name')).toHaveValue('Version 1')
    fireEvent.change(screen.getByLabelText('releases.name'), { target: { value: 'Version 1 updated' } })
    fireEvent.submit(form)
    expect(mocks.create).toHaveBeenCalledWith(
      { tag_name: 'v1.0.0', name: 'Version 1 updated', description: 'First release', prerelease: false },
      { onSuccess: expect.any(Function), onError: expect.any(Function) },
    )
    act(() => mocks.create.mock.calls[0][1].onSuccess())
    expect(toast.success).toHaveBeenCalledWith('releases.updated')
  })

  it('keeps delete confirmation open on failure and closes it after success', () => {
    mocks.releases.mockReturnValue(result([release()]))
    setup('/repositories/demo?tab=releases')
    fireEvent.click(screen.getByRole('button', { name: 'releases.deleteAction v1.0.0' }))
    expect(screen.getByRole('alertdialog')).toHaveTextContent('v1.0.0')
    fireEvent.click(screen.getByRole('button', { name: 'common.delete' }))
    expect(mocks.remove).toHaveBeenCalledWith('v1.0.0', { onSuccess: expect.any(Function), onError: expect.any(Function) })
    act(() => mocks.remove.mock.calls[0][1].onError(new Error('Unavailable')))
    expect(screen.getByRole('alertdialog')).toBeInTheDocument()
    expect(toast.error).toHaveBeenCalledWith('Unavailable')
    act(() => mocks.remove.mock.calls[0][1].onSuccess())
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument()
    expect(toast.success).toHaveBeenCalledWith('releases.deleted')
  })

  it('blocks creation until the current release list can be checked', () => {
    const refetch = vi.fn()
    mocks.releases.mockReturnValue({ data: undefined, isLoading: false, error: new Error('offline'), refetch })
    setup('/repositories/demo?tab=releases')
    expect(screen.getByRole('button', { name: 'releases.create' })).toBeDisabled()
    expect(screen.getByText('repositoryBrowser.releasesLoadError')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(refetch).toHaveBeenCalledOnce()
    expect(mocks.create).not.toHaveBeenCalled()
  })
})
