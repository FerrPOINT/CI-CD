import { fireEvent, render, screen, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { DashboardPage } from './index'

const mocks = vi.hoisted(() => ({
  useProjects: vi.fn(),
  useRunners: vi.fn(),
  useProjectPipelines: vi.fn(),
  refetchProjects: vi.fn(),
  refetchRunners: vi.fn(),
  refetchPipelines: vi.fn(),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    i18n: { language: 'ru' },
    t: (key: string, options?: { count?: number }) => options?.count === undefined ? key : `${key} ${options.count}`,
  }),
}))

vi.mock('@/api/hooks', () => ({
  useProjects: mocks.useProjects,
  useRunners: mocks.useRunners,
}))

vi.mock('@/shared/lib/use-project-pipelines', () => ({
  useProjectPipelines: mocks.useProjectPipelines,
}))

const projects = [
  {
    id: 'project-1',
    name: 'Platform API',
    repository_url: 'https://example.test/platform-api.git',
    default_branch: 'main',
    created_at: '2026-09-23T00:00:00Z',
  },
  {
    id: 'project-2',
    name: 'Web Console',
    repository_url: 'https://example.test/web-console.git',
    default_branch: 'main',
    created_at: '2026-09-23T00:00:00Z',
  },
]

function renderPage() {
  render(
    <MemoryRouter>
      <DashboardPage />
    </MemoryRouter>,
  )
}

function defaultQueries() {
  mocks.useProjects.mockReturnValue({
    data: projects,
    isLoading: false,
    error: null,
    refetch: mocks.refetchProjects,
  })
  mocks.useRunners.mockReturnValue({
    data: [],
    isLoading: false,
    error: null,
    refetch: mocks.refetchRunners,
  })
  mocks.useProjectPipelines.mockReturnValue({
    runs: [],
    isLoading: false,
    error: null,
    failedCount: 0,
    hasData: true,
    refetch: mocks.refetchPipelines,
  })
}

afterEach(() => {
  vi.clearAllMocks()
})

describe('DashboardPage recovery states', () => {
  it('keeps successful runs visible when another project request fails', () => {
    defaultQueries()
    mocks.useProjectPipelines.mockReturnValue({
      runs: [{
        id: 'run-1',
        project_id: 'project-1',
        git_ref: 'main',
        commit_sha: 'a'.repeat(40),
        status: 'failed',
        created_at: '2026-09-23T01:00:00Z',
        updated_at: '2026-09-23T01:01:00Z',
      }],
      isLoading: false,
      error: new Error('Service Unavailable'),
      failedCount: 1,
      hasData: true,
      refetch: mocks.refetchPipelines,
    })
    renderPage()

    const recentRuns = screen.getByRole('heading', { name: 'dashboard.recentRuns' }).closest('section')
    expect(recentRuns).not.toBeNull()
    expect(within(recentRuns!).getByText('Platform API')).toBeInTheDocument()
    const alert = within(recentRuns!).getByRole('alert')
    expect(alert).toHaveTextContent('dashboard.runsPartialError 1')
    fireEvent.click(within(alert).getByRole('button', { name: 'common.retry' }))
    expect(mocks.refetchPipelines).toHaveBeenCalledOnce()
  })

  it('offers a local retry when the project catalog is unavailable', () => {
    defaultQueries()
    mocks.useProjects.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: new Error('Service Unavailable'),
      refetch: mocks.refetchProjects,
    })
    renderPage()

    const alert = screen.getByRole('alert')
    expect(alert).toHaveTextContent('dashboard.projectsError')
    fireEvent.click(within(alert).getByRole('button', { name: 'common.retry' }))
    expect(mocks.refetchProjects).toHaveBeenCalledOnce()
  })

  it('retries runner status without hiding the rest of the dashboard', () => {
    defaultQueries()
    mocks.useRunners.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: new Error('Service Unavailable'),
      refetch: mocks.refetchRunners,
    })
    renderPage()

    expect(screen.getByText('Platform API')).toBeInTheDocument()
    const runners = screen.getByRole('heading', { name: 'navigation.runners' }).closest('section')
    expect(runners).not.toBeNull()
    const alert = within(runners!).getByRole('alert')
    expect(alert).toHaveTextContent('dashboard.runnersError')
    fireEvent.click(within(alert).getByRole('button', { name: 'common.retry' }))
    expect(mocks.refetchRunners).toHaveBeenCalledOnce()
  })
})
