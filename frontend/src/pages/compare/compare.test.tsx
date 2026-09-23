import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { Link, MemoryRouter, Route, Routes } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ComparePage } from './index'

const mocks = vi.hoisted(() => ({ comparison: vi.fn(), refs: vi.fn(), refetch: vi.fn(), refetchRefs: vi.fn() }))

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }))
vi.mock('@/api/hooks', () => ({ useRepositoryComparison: mocks.comparison, useRepositoryRefs: mocks.refs }))

function setup(path = '/repositories/platform/compare', options: { comparison?: unknown; error?: Error; refsError?: Error } = {}) {
  mocks.comparison.mockReturnValue({ data: options.comparison, isLoading: false, error: options.error ?? null, refetch: mocks.refetch })
  mocks.refs.mockReturnValue({ data: [{ name: 'main', sha: 'a', target: '' }, { name: 'feature/cache', sha: 'b', target: '' }], isLoading: false, error: options.refsError ?? null, refetch: mocks.refetchRefs })
  render(
    <MemoryRouter initialEntries={[path]}>
      <Link to="/repositories/platform/compare?from=release%2F1&to=feature%2Fcache">Switch comparison</Link>
      <Routes><Route path="/repositories/:repo/compare" element={<ComparePage />} /></Routes>
    </MemoryRouter>,
  )
}

afterEach(() => vi.clearAllMocks())

describe('ComparePage', () => {
  it('starts without fabricated refs and waits for an explicit comparison', () => {
    setup()
    expect(screen.getByLabelText('compare.baseRef')).toHaveValue('')
    expect(screen.getByLabelText('compare.headRef')).toHaveValue('')
    expect(screen.getByRole('status')).toHaveTextContent('compare.chooseRefs')
    expect(mocks.comparison).toHaveBeenCalledWith('platform', '', '')
  })

  it('prevents comparing the same ref and synchronizes inputs after URL navigation', async () => {
    setup()
    const form = screen.getByRole('form', { name: 'compare.title' })
    fireEvent.change(screen.getByLabelText('compare.baseRef'), { target: { value: 'main' } })
    fireEvent.change(screen.getByLabelText('compare.headRef'), { target: { value: 'main' } })
    expect(within(form).getByRole('button', { name: 'compare.compareAction' })).toBeDisabled()
    expect(screen.getByRole('alert')).toHaveTextContent('compare.refsMustDiffer')
    fireEvent.click(screen.getByRole('link', { name: 'Switch comparison' }))
    await waitFor(() => expect(screen.getByLabelText('compare.baseRef')).toHaveValue('release/1'))
    expect(screen.getByLabelText('compare.headRef')).toHaveValue('feature/cache')
    expect(mocks.comparison).toHaveBeenLastCalledWith('platform', 'release/1', 'feature/cache')
  })

  it('shows accurate status and a binary marker instead of invented line counts', () => {
    setup('/repositories/platform/compare?from=main&to=feature%2Fcache', {
      comparison: {
        from: 'main', to: 'feature/cache', merge_base: 'abc123', patch: '+new line', patch_truncated: true,
        files: [
          { path: 'new.txt', status: 'added', additions: 1, deletions: 0, binary: false },
          { path: 'old.txt', status: 'deleted', additions: 0, deletions: 3, binary: false },
          { path: 'image.png', status: 'modified', additions: 0, deletions: 0, binary: true },
        ],
      },
    })
    const files = screen.getByRole('list')
    expect(within(files).getByText('compare.status_added')).toBeInTheDocument()
    expect(within(files).getByText('compare.status_deleted')).toBeInTheDocument()
    expect(within(files).getByText('compare.binaryFile')).toBeInTheDocument()
    expect(within(files).getByText('image.png').closest('li')).not.toHaveTextContent('+0')
    expect(screen.getByRole('status')).toHaveTextContent('compare.patchTruncated')
    expect(screen.getByLabelText('compare.patch')).toHaveAttribute('tabindex', '0')
  })

  it('offers retry for comparison and ref-suggestion failures without blocking manual entry', () => {
    setup('/repositories/platform/compare?from=main&to=feature%2Fcache', { error: new Error('Unavailable'), refsError: new Error('Refs unavailable') })
    expect(screen.getAllByRole('alert')).toHaveLength(2)
    expect(screen.getByLabelText('compare.baseRef')).not.toBeDisabled()
    const retries = screen.getAllByRole('button', { name: 'common.retry' })
    fireEvent.click(retries[0])
    fireEvent.click(retries[1])
    expect(mocks.refetchRefs).toHaveBeenCalled()
    expect(mocks.refetch).toHaveBeenCalled()
  })
})
