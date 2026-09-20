import { act, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { toast } from 'sonner'
import { RunnersPage } from './index'

const mocks = vi.hoisted(() => ({
  useRunners: vi.fn(),
  register: vi.fn(),
  remove: vi.fn(),
  refetch: vi.fn(),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: { name?: string; count?: number; total?: number }) =>
      options?.name ? `${key} ${options.name}` :
        options?.count !== undefined ? `${key} ${options.count}/${options.total}` : key,
  }),
}))
vi.mock('@/api/hooks', () => ({
  useRunners: mocks.useRunners,
  useRegisterRunner: () => ({ mutate: mocks.register, isPending: false }),
  useDeleteRunner: () => ({ mutate: mocks.remove, isPending: false }),
}))
vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }))

function runner(index: number) {
  return {
    id: `runner-${index}`,
    name: `runner-${String(index).padStart(2, '0')}`,
    tags: index % 2 === 0 ? ['linux', 'docker'] : ['shell'],
    status: index % 2 === 0 ? 'online' : 'offline',
    last_seen_at: index % 2 === 0 ? '2026-09-20T00:00:00Z' : null,
    created_at: '2026-09-19T00:00:00Z',
  }
}

function setup(count: number) {
  mocks.useRunners.mockReturnValue({
    data: Array.from({ length: count }, (_, index) => runner(index + 1)),
    isLoading: false,
    error: null,
    refetch: mocks.refetch,
  })
  render(<RunnersPage />)
}

afterEach(() => vi.clearAllMocks())

describe('RunnersPage', () => {
  it('shows one compact list with search, status filtering and pagination', () => {
    setup(24)
    expect(screen.getAllByRole('button', { name: /runners.deleteRunner/ })).toHaveLength(20)
    expect(screen.queryByText('runners.heartbeat')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'runners.next' }))
    expect(screen.getAllByRole('button', { name: /runners.deleteRunner/ })).toHaveLength(4)
    fireEvent.change(screen.getByRole('searchbox', { name: 'runners.search' }), { target: { value: 'runner-07' } })
    expect(screen.getByRole('button', { name: 'runners.deleteRunner runner-07' })).toBeInTheDocument()
    expect(screen.queryByRole('navigation', { name: 'runners.pages' })).not.toBeInTheDocument()

    fireEvent.change(screen.getByRole('searchbox', { name: 'runners.search' }), { target: { value: '' } })
    fireEvent.change(screen.getByLabelText('runners.status'), { target: { value: 'online' } })
    expect(screen.getAllByRole('button', { name: /runners.deleteRunner/ })).toHaveLength(12)
    expect(screen.queryByRole('button', { name: 'runners.deleteRunner runner-07' })).not.toBeInTheDocument()
  })

  it('distinguishes empty inventory from no filter matches', () => {
    setup(1)
    fireEvent.change(screen.getByRole('searchbox', { name: 'runners.search' }), { target: { value: 'missing' } })
    expect(screen.getByRole('status')).toHaveTextContent('runners.noMatches')
    expect(screen.queryByText('runners.empty')).not.toBeInTheDocument()
  })

  it('hides stale rows on list error and exposes retry', () => {
    mocks.useRunners.mockReturnValue({ data: [runner(1)], isLoading: false, error: new Error('Internal Server Error'), refetch: mocks.refetch })
    render(<RunnersPage />)
    expect(screen.getByRole('alert')).toHaveTextContent('runners.loadError')
    expect(screen.queryByRole('button', { name: 'runners.deleteRunner runner-01' })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(mocks.refetch).toHaveBeenCalledOnce()
  })

  it('trims a manual inventory record and waits for registration success', () => {
    setup(0)
    fireEvent.click(screen.getByRole('button', { name: 'runners.register' }))
    const form = screen.getByRole('form', { name: 'runners.register' })
    expect(form).toHaveTextContent('runners.manualNotice')
    fireEvent.change(screen.getByLabelText('runners.name'), { target: { value: '  qa-runner  ' } })
    fireEvent.change(screen.getByLabelText('runners.tags'), { target: { value: ' linux, docker, ' } })
    fireEvent.submit(form)
    expect(mocks.register).toHaveBeenCalledWith(
      { name: 'qa-runner', tags: ['linux', 'docker'] },
      { onSuccess: expect.any(Function), onError: expect.any(Function) },
    )
    expect(form).toBeInTheDocument()
    act(() => mocks.register.mock.calls[0][1].onSuccess())
    expect(screen.queryByRole('form', { name: 'runners.register' })).not.toBeInTheDocument()
    expect(toast.success).toHaveBeenCalledWith('runners.registered')
  })

  it('keeps the delete confirmation on API failure and closes it after success', () => {
    setup(1)
    fireEvent.click(screen.getByRole('button', { name: 'runners.deleteRunner runner-01' }))
    expect(screen.getByRole('alertdialog')).toHaveTextContent('runners.deleteWarning')
    fireEvent.click(screen.getByRole('button', { name: 'common.delete' }))
    expect(mocks.remove).toHaveBeenCalledWith('runner-1', { onSuccess: expect.any(Function), onError: expect.any(Function) })
    act(() => mocks.remove.mock.calls[0][1].onError(new Error('Unavailable')))
    expect(screen.getByRole('alertdialog')).toBeInTheDocument()
    expect(toast.error).toHaveBeenCalledWith('runners.deleteError')
    act(() => mocks.remove.mock.calls[0][1].onSuccess())
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument()
  })
})
