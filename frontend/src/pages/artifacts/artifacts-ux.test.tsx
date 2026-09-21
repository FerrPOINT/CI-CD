import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { toast } from 'sonner'
import { ArtifactsPage } from './index'

const mocks = vi.hoisted(() => ({
  useArtifacts: vi.fn(),
  refetch: vi.fn(),
  download: vi.fn(),
  save: vi.fn(),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: { name?: string; count?: number; total?: number }) =>
      options?.name ? key + ' ' + options.name :
        options?.count !== undefined ? key + ' ' + options.count + '/' + options.total : key,
  }),
}))
vi.mock('@/api/hooks', () => ({ useArtifacts: mocks.useArtifacts }))
vi.mock('@/api/client', () => ({
  ApiError: class ApiError extends Error {},
  downloadArtifact: mocks.download,
  saveDownloadedArtifact: mocks.save,
}))
vi.mock('sonner', () => ({ toast: { success: vi.fn() } }))

const originalClipboard = navigator.clipboard

function artifact(index: number) {
  return {
    id: 'artifact-' + index,
    job_id: 'job-1',
    attempt_id: null,
    name: 'report-' + String(index).padStart(2, '0') + '.txt',
    content_type: 'text/plain',
    sha256: index === 4 ? null : String(index).padStart(2, '0').repeat(32),
    size_bytes: index * 1024,
    created_at: '2026-09-20T00:00:00Z',
    expires_at: index === 2 ? '2020-09-20T00:00:00Z' : '2099-09-20T00:00:00Z',
    purged_at: index === 3 ? '2026-09-20T00:00:00Z' : null,
  }
}

function page() {
  return (
    <MemoryRouter initialEntries={['/jobs/job-1/artifacts']}>
      <Routes>
        <Route path="/jobs/:jobId/artifacts" element={<ArtifactsPage />} />
      </Routes>
    </MemoryRouter>
  )
}

function setup(count: number) {
  mocks.useArtifacts.mockReturnValue({
    data: Array.from({ length: count }, (_, index) => artifact(index + 1)),
    isLoading: false,
    error: null,
    refetch: mocks.refetch,
  })
  return render(page())
}

afterEach(() => {
  vi.resetAllMocks()
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: originalClipboard })
})

describe('ArtifactsPage UX', () => {
  it('shows searchable pages, status filters and touch-sized actions', () => {
    setup(25)
    expect(screen.getAllByRole('listitem')).toHaveLength(20)
    expect(screen.getByRole('button', { name: 'artifacts.downloadFor report-01.txt' })).toHaveClass('h-10', 'w-10')
    expect(screen.getByRole('button', { name: 'artifacts.copyDigestFor report-01.txt' })).toHaveClass('h-10', 'w-10')
    expect(screen.getAllByText('SHA-256:', { exact: false })).toHaveLength(19)
    fireEvent.click(screen.getByRole('button', { name: 'artifacts.next' }))
    expect(screen.getAllByRole('listitem')).toHaveLength(5)
    fireEvent.change(screen.getByRole('searchbox', { name: 'artifacts.search' }), { target: { value: 'report-07' } })
    expect(screen.getByRole('button', { name: 'artifacts.downloadFor report-07.txt' })).toBeInTheDocument()
    expect(screen.queryByRole('navigation', { name: 'artifacts.pages' })).not.toBeInTheDocument()

    fireEvent.change(screen.getByRole('searchbox', { name: 'artifacts.search' }), { target: { value: '' } })
    fireEvent.change(screen.getByRole('combobox', { name: 'artifacts.status' }), { target: { value: 'expired' } })
    expect(screen.getAllByRole('listitem')).toHaveLength(1)
    expect(screen.getAllByText('artifacts.expired')).toHaveLength(2)
    expect(screen.queryByRole('button', { name: 'artifacts.downloadFor report-02.txt' })).not.toBeInTheDocument()
    fireEvent.change(screen.getByRole('combobox', { name: 'artifacts.status' }), { target: { value: 'purged' } })
    expect(screen.getAllByText('artifacts.purged')).toHaveLength(2)
    expect(screen.queryByRole('button', { name: 'artifacts.downloadFor report-03.txt' })).not.toBeInTheDocument()
  })

  it('distinguishes empty, error and retry without stale rows', () => {
    const view = setup(0)
    expect(screen.getByText('artifacts.empty')).toBeInTheDocument()
    mocks.useArtifacts.mockReturnValue({ data: [artifact(1)], isLoading: false, error: null, refetch: mocks.refetch })
    view.rerender(page())
    expect(screen.getByRole('button', { name: 'artifacts.downloadFor report-01.txt' })).toBeInTheDocument()
    mocks.useArtifacts.mockReturnValue({ data: [artifact(1)], isLoading: false, error: new Error('raw 500'), refetch: mocks.refetch })
    view.rerender(page())
    expect(screen.getByRole('alert')).toHaveTextContent('artifacts.loadError')
    expect(screen.queryByRole('button', { name: 'artifacts.downloadFor report-01.txt' })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(mocks.refetch).toHaveBeenCalledOnce()
  })

  it('blocks duplicate downloads and keeps a row-level error for retry', async () => {
    setup(1)
    let rejectDownload: (error: Error) => void = () => {}
    mocks.download.mockReturnValueOnce(new Promise((_, reject) => { rejectDownload = reject }))
    fireEvent.click(screen.getByRole('button', { name: 'artifacts.downloadFor report-01.txt' }))
    expect(screen.getByRole('button', { name: 'artifacts.downloading report-01.txt' })).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: 'artifacts.downloading report-01.txt' }))
    expect(mocks.download).toHaveBeenCalledOnce()

    await act(async () => rejectDownload(new Error('network failed')))
    expect(screen.getByRole('alert')).toHaveTextContent('artifacts.downloadFailed')
    expect(mocks.refetch).toHaveBeenCalledOnce()
    const downloaded = { blob: new Blob(['ok']), filename: 'report-01.txt' }
    mocks.download.mockResolvedValueOnce(downloaded)
    fireEvent.click(screen.getByRole('button', { name: 'artifacts.downloadFor report-01.txt' }))
    await waitFor(() => expect(mocks.save).toHaveBeenCalledWith(downloaded))
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('copies the full digest and reports clipboard failure in the same row', async () => {
    const writeText = vi.fn().mockResolvedValueOnce(undefined).mockRejectedValueOnce(new Error('blocked'))
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } })
    setup(1)
    const copy = screen.getByRole('button', { name: 'artifacts.copyDigestFor report-01.txt' })
    fireEvent.click(copy)
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('01'.repeat(32)))
    expect(toast.success).toHaveBeenCalledWith('artifacts.copied')
    fireEvent.click(copy)
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('artifacts.copyFailed'))
  })
})
