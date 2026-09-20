import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ReportsPage } from './index'
import type { ProjectReport } from '@/api/types'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: 'ru' } }),
}))

const projectId = '22222222-2222-4222-8222-222222222222'
const report: ProjectReport = {
  total_pipelines: 6,
  successful_pipelines: 3,
  failed_pipelines: 1,
  success_rate: 0.6,
  average_duration_seconds: 90,
}

function renderReport(value: ProjectReport, failOnce = false) {
  let reads = 0
  vi.stubGlobal('fetch', vi.fn(() => {
    reads++
    if (failOnce && reads === 1) return Promise.resolve(new Response('unavailable', { status: 503 }))
    return Promise.resolve(new Response(JSON.stringify(value), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    }))
  }))

  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(<QueryClientProvider client={client}>
    <MemoryRouter initialEntries={[`/projects/${projectId}/reports`]}>
      <Routes><Route path="/projects/:projectId/reports" element={<ReportsPage />} /></Routes>
    </MemoryRouter>
  </QueryClientProvider>)
  return () => reads
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('ReportsPage', () => {
  it('shows a compact, scoped summary of all statuses', async () => {
    renderReport(report)

    expect(await screen.findByText(/60,0/)).toBeInTheDocument()
    expect(screen.getByText('reports.allTime')).toBeInTheDocument()
    expect(screen.getByText('reports.rateScope')).toBeInTheDocument()
    expect(screen.getByText('reports.durationScope')).toBeInTheDocument()
    expect(screen.getByText('reports.statusDistribution')).toBeInTheDocument()
    expect(screen.getByText('1,5 reports.minutes')).toBeInTheDocument()
    expect(screen.getByRole('img', { name: 'reports.distribution' })).toBeInTheDocument()
    expect(screen.getByText('reports.other').nextElementSibling).toHaveTextContent('2')
    expect(screen.getByRole('link', { name: 'reports.openPipelines' })).toHaveAttribute('href', `/projects/${projectId}/pipelines`)
  })

  it('offers a route to the first pipeline when no runs exist', async () => {
    renderReport({ ...report, total_pipelines: 0, successful_pipelines: 0, failed_pipelines: 0, success_rate: 0, average_duration_seconds: 0 })

    expect(await screen.findByText('reports.noData')).toBeInTheDocument()
    expect(screen.getByText('reports.emptyHint')).toBeInTheDocument()
    expect(screen.getAllByRole('link', { name: 'reports.openPipelines' })).toHaveLength(1)
    expect(screen.queryByText('reports.successRate')).not.toBeInTheDocument()
  })

  it('keeps a failed load distinct from an empty report and retries', async () => {
    const reads = renderReport(report, true)

    expect(await screen.findByText('reports.loadFailed')).toBeInTheDocument()
    expect(screen.queryByText('reports.noData')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(await screen.findByText(/60,0/)).toBeInTheDocument()
    await waitFor(() => expect(reads()).toBe(2))
  })

  it('does not show a fictitious duration when the average is unavailable', async () => {
    renderReport({ ...report, average_duration_seconds: 0 })

    expect(await screen.findByText('reports.avgDuration')).toBeInTheDocument()
    expect(screen.getByText('—')).toBeInTheDocument()
  })
})
