import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { toast } from 'sonner'
import { RepositoriesPage } from './index'

const mocks = vi.hoisted(() => ({
  useRepositories: vi.fn(),
  useProjects: vi.fn(),
  create: vi.fn(),
  remove: vi.fn(),
  refetch: vi.fn(),
  refetchProjects: vi.fn(),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: { name?: string; count?: number; total?: number }) =>
      options?.name ? `${key} ${options.name}` :
        options?.count !== undefined ? `${key} ${options.count}/${options.total}` : key,
  }),
}))
vi.mock('@/api/hooks', () => ({
  useRepositories: mocks.useRepositories,
  useProjects: mocks.useProjects,
  useCreateRepository: () => ({ mutate: mocks.create, isPending: false }),
  useDeleteRepository: () => ({ mutate: mocks.remove, isPending: false }),
}))
vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }))

function repository(index: number) {
  return {
    id: `repository-${index}`,
    name: `repo-${String(index).padStart(2, '0')}`,
    created_at: '2026-09-19T00:00:00Z',
  }
}

function setup(count: number, initialEntry = '/repositories') {
  mocks.useRepositories.mockReturnValue({
    data: Array.from({ length: count }, (_, index) => repository(index + 1)),
    isLoading: false,
    error: null,
    refetch: mocks.refetch,
  })
  mocks.useProjects.mockReturnValue({
    data: [{ id: 'project-1', name: 'Project 1', repository_url: 'git@example.test:repo-01.git' }],
    isLoading: false,
    error: null,
    refetch: mocks.refetchProjects,
  })
  render(<MemoryRouter initialEntries={[initialEntry]}><RepositoriesPage /></MemoryRouter>)
}

afterEach(() => {
  vi.clearAllMocks()
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: undefined })
})

describe('RepositoriesPage', () => {
  it('shows linked rows with visible actions, search, project filtering and pagination', () => {
    setup(25)

    expect(screen.getAllByRole('link', { name: /repositories.openRepository/ })).toHaveLength(20)
    expect(screen.getByRole('link', { name: 'repositories.openRepository repo-01' })).toHaveAttribute('href', '/repositories/repo-01')
    expect(screen.getByRole('button', { name: 'repositories.copyUrlFor repo-01' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'repositories.deleteRepository repo-01' })).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'repositories.next' }))
    expect(screen.getAllByRole('link', { name: /repositories.openRepository/ })).toHaveLength(5)
    fireEvent.change(screen.getByRole('searchbox', { name: 'repositories.search' }), { target: { value: 'repo-07' } })
    expect(screen.getAllByRole('link', { name: /repositories.openRepository/ })).toHaveLength(1)
    expect(screen.queryByRole('navigation', { name: 'repositories.pages' })).not.toBeInTheDocument()

    fireEvent.change(screen.getByRole('searchbox', { name: 'repositories.search' }), { target: { value: '' } })
    fireEvent.change(screen.getByLabelText('repositories.projectFilter'), { target: { value: 'Project 1' } })
    expect(screen.getAllByRole('link', { name: /repositories.openRepository/ })).toHaveLength(1)
  })

  it('keeps a project link filter and distinguishes no matches from an empty catalog', () => {
    setup(2, '/repositories?project=Project%201')
    expect(screen.getByLabelText('repositories.projectFilter')).toHaveValue('Project 1')
    expect(screen.getAllByRole('link', { name: /repositories.openRepository/ })).toHaveLength(1)
    fireEvent.change(screen.getByRole('searchbox', { name: 'repositories.search' }), { target: { value: 'missing' } })
    expect(screen.getByRole('status')).toHaveTextContent('repositories.noMatches')
    expect(screen.queryByText('repositories.empty')).not.toBeInTheDocument()
  })

  it('exposes a list error with retry', () => {
    mocks.useRepositories.mockReturnValue({ data: [], isLoading: false, error: new Error('Unavailable'), refetch: mocks.refetch })
    mocks.useProjects.mockReturnValue({ data: [], isLoading: false, error: null, refetch: mocks.refetchProjects })
    render(<MemoryRouter><RepositoriesPage /></MemoryRouter>)
    expect(screen.getByRole('alert')).toHaveTextContent('repositories.listLoadError')
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(mocks.refetch).toHaveBeenCalled()
  })

  it('lets users clear a project filter when the project catalog fails', () => {
    mocks.useRepositories.mockReturnValue({ data: [repository(1)], isLoading: false, error: null, refetch: mocks.refetch })
    mocks.useProjects.mockReturnValue({ data: [], isLoading: false, error: new Error('Unavailable'), refetch: mocks.refetchProjects })
    render(<MemoryRouter initialEntries={['/repositories?project=Project%201']}><RepositoriesPage /></MemoryRouter>)
    expect(screen.getByRole('alert')).toHaveTextContent('repositories.projectLoadError')
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(mocks.refetchProjects).toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'repositories.allProjects' }))
    expect(screen.getByRole('link', { name: 'repositories.openRepository repo-01' })).toBeInTheDocument()
  })

  it('trims the create name and closes the form only after success', () => {
    setup(0)
    fireEvent.click(screen.getByRole('button', { name: 'repositories.create' }))
    const form = screen.getByRole('form', { name: 'repositories.create' })
    fireEvent.change(screen.getByLabelText('repositories.name'), { target: { value: '  new-repo  ' } })
    fireEvent.submit(form)
    expect(mocks.create).toHaveBeenCalledWith({ name: 'new-repo' }, { onSuccess: expect.any(Function), onError: expect.any(Function) })
    expect(form).toBeInTheDocument()
    act(() => mocks.create.mock.calls[0][1].onSuccess())
    expect(screen.queryByRole('form', { name: 'repositories.create' })).not.toBeInTheDocument()
    expect(toast.success).toHaveBeenCalledWith('repositories.created')
  })

  it('reports clipboard failure and keeps delete confirmation open on an API error', async () => {
    setup(1)
    const writeText = vi.fn().mockRejectedValue(new Error('Denied'))
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } })
    fireEvent.click(screen.getByRole('button', { name: 'repositories.copyUrlFor repo-01' }))
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith('repositories.copyError'))
    expect(writeText).toHaveBeenCalledWith(expect.stringContaining('/git/repo-01.git'))

    fireEvent.click(screen.getByRole('button', { name: 'repositories.deleteRepository repo-01' }))
    fireEvent.click(screen.getByRole('button', { name: 'common.delete' }))
    expect(mocks.remove).toHaveBeenCalledWith('repo-01', { onSuccess: expect.any(Function), onError: expect.any(Function) })
    act(() => mocks.remove.mock.calls[0][1].onError(new Error('Unavailable')))
    expect(screen.getByRole('alertdialog')).toBeInTheDocument()
    act(() => mocks.remove.mock.calls[0][1].onSuccess())
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument()
  })

  it('confirms a successful copy without changing repository data', async () => {
    setup(1)
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } })
    fireEvent.click(screen.getByRole('button', { name: 'repositories.copyUrlFor repo-01' }))
    await waitFor(() => expect(toast.success).toHaveBeenCalledWith('repositories.copySuccess'))
    expect(mocks.create).not.toHaveBeenCalled()
    expect(mocks.remove).not.toHaveBeenCalled()
  })
})
