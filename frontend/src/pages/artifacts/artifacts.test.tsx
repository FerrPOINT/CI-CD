import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ArtifactsPage } from './index'

const clientMocks = vi.hoisted(() => ({
  downloadArtifact: vi.fn(),
  saveDownloadedArtifact: vi.fn(),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))
vi.mock('@/api/hooks', () => ({
  useArtifacts: () => ({
    data: [{
      id: 'artifact-id',
      name: 'build.txt',
      size_bytes: 14,
      content_type: 'text/plain',
      sha256: null,
      created_at: '2026-09-08T12:00:00Z',
      expires_at: '2099-09-08T12:00:00Z',
      purged_at: null,
    }],
    isLoading: false,
    error: null,
  }),
}))
vi.mock('@/api/client', () => clientMocks)
vi.mock('sonner', () => ({ toast: { error: vi.fn() } }))

afterEach(() => {
  clientMocks.downloadArtifact.mockReset()
  clientMocks.saveDownloadedArtifact.mockReset()
})

describe('ArtifactsPage', () => {
  it('[REQ-UI-001] downloads through the authenticated transport instead of a plain link', async () => {
    const artifact = { blob: new Blob(['artifact']), filename: 'build.txt' }
    clientMocks.downloadArtifact.mockResolvedValue(artifact)

    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <MemoryRouter initialEntries={['/jobs/job-id/artifacts']}>
          <Routes>
            <Route path="/jobs/:jobId/artifacts" element={<ArtifactsPage />} />
          </Routes>
        </MemoryRouter>
      </QueryClientProvider>,
    )

    fireEvent.click(await screen.findByRole('button', { name: 'artifacts.download' }))

    await waitFor(() => {
      expect(clientMocks.downloadArtifact).toHaveBeenCalledWith('artifact-id')
      expect(clientMocks.saveDownloadedArtifact).toHaveBeenCalledWith(artifact)
    })
    expect(screen.queryByRole('link', { name: 'artifacts.download' })).toBeNull()
  })
})
