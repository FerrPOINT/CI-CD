import { fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
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
  useCreateRelease: () => ({ mutate: vi.fn(), isPending: false }),
  useDeleteRelease: () => ({ mutate: vi.fn(), isPending: false }),
}))

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

beforeEach(() => {
  mocks.refs.mockReturnValue(result([
    { name: 'main', kind: 'branch', sha: 'abc123456', target: '' },
    { name: 'v1', kind: 'tag', sha: 'def123456', target: '' },
  ], mocks.refetchRefs))
  mocks.tree.mockReturnValue(result([], mocks.refetchTree))
  mocks.blob.mockReturnValue(result({ path: 'empty.txt', sha: 'abc123456', size: 0, content: '', binary: false, truncated: false }, mocks.refetchBlob))
  mocks.commits.mockReturnValue(result([], mocks.refetchCommits))
  mocks.tags.mockReturnValue(result([], mocks.refetchTags))
  mocks.releases.mockReturnValue(result([]))
})

afterEach(() => vi.clearAllMocks())

describe('RepositoryBrowserPage', () => {
  it('opens code by default and keeps nested directories and files in the URL', () => {
    mocks.tree.mockImplementation((_repo: string, _ref: string, dir?: string) => result(dir === 'src'
      ? [{ path: 'src/index.tsx', name: 'index.tsx', kind: 'blob', sha: 'def123456', size: 12 }]
      : [{ path: 'src', name: 'src', kind: 'tree', sha: 'abc123456', size: null }]))
    setup()

    expect(screen.getByRole('tab', { name: 'repositoryBrowser.code' })).toHaveAttribute('data-state', 'active')
    fireEvent.click(screen.getByRole('button', { name: 'src' }))
    expect(screen.getByTestId('location')).toHaveTextContent('dir=src')
    expect(mocks.tree).toHaveBeenLastCalledWith('demo', 'HEAD', 'src')
    fireEvent.click(screen.getByRole('button', { name: 'index.tsx' }))
    expect(screen.getByTestId('location')).toHaveTextContent('file=src%2Findex.tsx')
    expect(mocks.blob).toHaveBeenCalledWith('demo', 'HEAD', 'src/index.tsx')
    expect(screen.getByRole('button', { name: 'repositoryBrowser.backToTree' })).toBeInTheDocument()
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
    expect(screen.getByText('repositoryBrowser.truncatedFile')).toBeInTheDocument()
    expect(screen.getByText('partial text')).toBeInTheDocument()
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
    expect(mocks.tree).toHaveBeenLastCalledWith('demo', 'refs/heads/main', undefined)
  })

  it('isolates commits and tags errors from the code tab', async () => {
    mocks.commits.mockReturnValue({ ...result(undefined, mocks.refetchCommits), error: new Error('offline') })
    mocks.tags.mockReturnValue({ ...result(undefined, mocks.refetchTags), error: new Error('offline') })
    setup()

    fireEvent.mouseDown(screen.getByRole('tab', { name: 'repositoryBrowser.commits' }), { button: 0 })
    expect(screen.getByTestId('location')).toHaveTextContent('tab=commits')
    expect(screen.getByRole('tab', { name: 'repositoryBrowser.commits' })).toHaveAttribute('data-state', 'active')
    expect(await screen.findByText('repositoryBrowser.commitsLoadError')).toBeInTheDocument()
    expect(mocks.commits).toHaveBeenCalledWith('demo', 'HEAD')
    fireEvent.mouseDown(screen.getByRole('tab', { name: 'repositoryBrowser.tags' }), { button: 0 })
    expect(await screen.findByText('repositoryBrowser.tagsLoadError')).toBeInTheDocument()
    fireEvent.mouseDown(screen.getByRole('tab', { name: 'repositoryBrowser.code' }), { button: 0 })
    expect(screen.getByText('repositoryBrowser.emptyTree')).toBeInTheDocument()
  })
})
