import { fireEvent, render, screen, within } from '@testing-library/react'
import { Link, MemoryRouter, Route, Routes } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { PipelinesPage } from './index'

const mocks = vi.hoisted(() => ({
  usePipelines: vi.fn(),
  refetch: vi.fn(),
  trigger: vi.fn(),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: { page?: number }) =>
      options?.page ? `${key} ${options.page}` : key,
  }),
}))
vi.mock('@/api/hooks', () => ({
  useProjects: () => ({ data: [{ id: 'project-1', name: 'Project 01', default_branch: 'main' }] }),
  usePipelines: mocks.usePipelines,
  useTriggerPipeline: () => ({ mutate: mocks.trigger, isPending: false }),
}))
vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }))

function pipeline(index: number) {
  return {
    id: `${String(index).padStart(8, '0')}-0000-4000-8000-000000000000`,
    project_id: 'project-1',
    git_ref: index % 2 ? 'main' : 'release/2026-09',
    status: index === 1 ? 'running' : 'success',
    created_at: '2026-09-19T12:00:00Z',
    started_at: null,
    finished_at: null,
  }
}

function setup() {
  render(
    <MemoryRouter initialEntries={['/projects/project-1/pipelines']}>
      <Routes>
        <Route
          path="/projects/:projectId/pipelines"
          element={
            <>
              <Link to="/projects/project-2/pipelines">Switch project</Link>
              <PipelinesPage />
            </>
          }
        />
        <Route path="/pipelines/:pipelineId" element={<div>Created pipeline</div>} />
      </Routes>
    </MemoryRouter>,
  )
}

afterEach(() => vi.clearAllMocks())

describe('PipelinesPage', () => {
  it('shows 20 runs and fetches the next server page', () => {
    mocks.usePipelines.mockImplementation((_projectId: string, page: number) => ({
      data:
        page === 0 ? Array.from({ length: 21 }, (_, index) => pipeline(index + 1)) : [pipeline(21)],
      isLoading: false,
      isFetching: false,
      error: null,
      refetch: mocks.refetch,
    }))
    setup()

    expect(screen.getAllByRole('link', { name: /#000000/ })).toHaveLength(20)
    expect(screen.getByText('pipelines.page 1')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'projects.next' }))
    expect(mocks.usePipelines).toHaveBeenLastCalledWith('project-1', 1, 20)
    expect(screen.getAllByRole('link', { name: /#000000/ })).toHaveLength(1)
    expect(screen.getByRole('button', { name: 'projects.next' })).toBeDisabled()
  })

  it('starts from the first page when the project changes', () => {
    mocks.usePipelines.mockImplementation(() => ({
      data: Array.from({ length: 21 }, (_, index) => pipeline(index + 1)),
      isLoading: false,
      isFetching: false,
      error: null,
      refetch: mocks.refetch,
    }))
    setup()
    fireEvent.click(screen.getByRole('button', { name: 'projects.next' }))
    fireEvent.click(screen.getByRole('link', { name: 'Switch project' }))
    expect(mocks.usePipelines).toHaveBeenLastCalledWith('project-2', 0, 20)
  })

  it('opens a newly triggered pipeline instead of leaving the user in the list', () => {
    mocks.usePipelines.mockReturnValue({
      data: [],
      isLoading: false,
      isFetching: false,
      error: null,
      refetch: mocks.refetch,
    })
    mocks.trigger.mockImplementation(
      (_gitRef: string, callbacks: { onSuccess: (value: unknown) => void }) =>
        callbacks.onSuccess({ pipeline: { id: 'new-pipeline' } }),
    )
    setup()

    fireEvent.click(screen.getByRole('button', { name: 'pipelines.run' }))
    const form = screen.getByRole('form', { name: 'pipelines.run' })
    expect(within(form).getByLabelText('pipelines.gitRef')).toHaveValue('main')
    fireEvent.change(within(form).getByLabelText('pipelines.gitRef'), {
      target: { value: 'feature/ui' },
    })
    fireEvent.click(within(form).getByRole('button', { name: 'pipelines.run' }))
    expect(mocks.trigger).toHaveBeenCalledWith('feature/ui', expect.any(Object))
    expect(screen.getByText('Created pipeline')).toBeInTheDocument()
  })

  it('offers retry when the run list fails to load', () => {
    mocks.usePipelines.mockReturnValue({
      data: undefined,
      isLoading: false,
      isFetching: false,
      error: new Error('offline'),
      refetch: mocks.refetch,
    })
    setup()
    expect(screen.getByRole('alert')).toHaveTextContent('offline')
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(mocks.refetch).toHaveBeenCalledOnce()
  })
})
