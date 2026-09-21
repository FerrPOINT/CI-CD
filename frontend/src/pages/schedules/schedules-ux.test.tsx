import { act, fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { toast } from 'sonner'
import { SchedulesPage } from './index'

const mocks = vi.hoisted(() => ({
  useSchedules: vi.fn(),
  create: vi.fn(),
  update: vi.fn(),
  remove: vi.fn(),
  refetch: vi.fn(),
  createPending: false,
  updatePending: false,
  deletePending: false,
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: { name?: string; count?: number; total?: number }) =>
      options?.name ? key + ' ' + options.name :
        options?.count !== undefined ? key + ' ' + options.count + '/' + options.total : key,
  }),
}))
vi.mock('@/api/hooks', () => ({
  useSchedules: mocks.useSchedules,
  useCreateSchedule: () => ({ mutate: mocks.create, isPending: mocks.createPending }),
  useUpdateSchedule: () => ({ mutate: mocks.update, isPending: mocks.updatePending }),
  useDeleteSchedule: () => ({ mutate: mocks.remove, isPending: mocks.deletePending }),
}))
vi.mock('sonner', () => ({ toast: { success: vi.fn() } }))

function schedule(index: number) {
  return {
    id: 'schedule-' + index,
    project_id: 'project-1',
    cron: '0 4 ' + index + ' * *',
    git_ref: 'release/' + String(index).padStart(2, '0'),
    enabled: index !== 2,
    next_fire_at: '2026-09-21T04:00:00Z',
    last_fired_at: null,
    last_fire_error: index === 2 ? 'cron must have five fields' : null,
    created_at: '2026-09-20T00:00:00Z',
  }
}

function page() {
  return (
    <MemoryRouter initialEntries={['/projects/project-1/schedules']}>
      <Routes>
        <Route path="/projects/:projectId/schedules" element={<SchedulesPage />} />
      </Routes>
    </MemoryRouter>
  )
}

function setup(count: number) {
  mocks.useSchedules.mockReturnValue({
    data: Array.from({ length: count }, (_, index) => schedule(index + 1)),
    isLoading: false,
    error: null,
    refetch: mocks.refetch,
  })
  return render(page())
}

afterEach(() => {
  vi.clearAllMocks()
  mocks.createPending = false
  mocks.updatePending = false
  mocks.deletePending = false
})

describe('SchedulesPage UX', () => {
  it('shows searchable pages and a recoverable scheduler error', () => {
    setup(25)
    expect(screen.getAllByRole('button', { name: /schedules.deleteFor/ })).toHaveLength(20)
    expect(screen.getByRole('button', { name: 'schedules.editFor 0 4 1 * *' })).toHaveClass('h-10', 'w-10')
    expect(screen.getByText('schedules.errorPaused')).toHaveClass('min-h-10')
    fireEvent.click(screen.getByText('schedules.errorPaused'))
    expect(screen.getByText('cron must have five fields')).toBeVisible()

    fireEvent.click(screen.getByRole('button', { name: 'schedules.next' }))
    expect(screen.getAllByRole('button', { name: /schedules.deleteFor/ })).toHaveLength(5)
    fireEvent.change(screen.getByRole('searchbox', { name: 'schedules.search' }), { target: { value: 'release/07' } })
    expect(screen.getByRole('button', { name: 'schedules.editFor 0 4 7 * *' })).toBeInTheDocument()
    expect(screen.queryByRole('navigation', { name: 'schedules.pages' })).not.toBeInTheDocument()
    fireEvent.change(screen.getByRole('searchbox', { name: 'schedules.search' }), { target: { value: 'missing' } })
    expect(screen.getByRole('status')).toHaveTextContent('schedules.noMatches')
  })

  it('distinguishes empty, error and retry without stale rows', () => {
    const view = setup(0)
    expect(screen.getByText('schedules.empty')).toBeInTheDocument()
    mocks.useSchedules.mockReturnValue({ data: [schedule(1)], isLoading: false, error: null, refetch: mocks.refetch })
    view.rerender(page())
    expect(screen.getByRole('button', { name: 'schedules.editFor 0 4 1 * *' })).toBeInTheDocument()
    mocks.useSchedules.mockReturnValue({ data: [schedule(1)], isLoading: false, error: new Error('raw 500'), refetch: mocks.refetch })
    view.rerender(page())
    expect(screen.getByRole('alert')).toHaveTextContent('schedules.loadError')
    expect(screen.queryByRole('button', { name: 'schedules.editFor 0 4 1 * *' })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(mocks.refetch).toHaveBeenCalledOnce()
  })

  it('keeps a failed create draft and clears it on cancel', () => {
    setup(0)
    fireEvent.click(screen.getByRole('button', { name: 'schedules.create' }))
    expect(screen.getByLabelText('schedules.cron')).toHaveFocus()
    fireEvent.change(screen.getByLabelText('schedules.cron'), { target: { value: '  0 5 * * 1  ' } })
    fireEvent.change(screen.getByLabelText('schedules.gitRef'), { target: { value: '  release  ' } })
    fireEvent.click(screen.getByRole('checkbox', { name: 'schedules.enabled' }))
    fireEvent.submit(screen.getByRole('form', { name: 'schedules.create' }))
    expect(mocks.create).toHaveBeenCalledWith(
      { cron: '0 5 * * 1', git_ref: 'release', enabled: false },
      expect.objectContaining({ onError: expect.any(Function), onSuccess: expect.any(Function) }),
    )
    act(() => mocks.create.mock.calls[0]![1].onError(new Error('bad request')))
    expect(screen.getByRole('alert')).toHaveTextContent('schedules.saveError')
    expect(screen.getByLabelText('schedules.cron')).toHaveValue('  0 5 * * 1  ')
    fireEvent.click(screen.getByRole('button', { name: 'common.cancel' }))
    fireEvent.click(screen.getByRole('button', { name: 'schedules.create' }))
    expect(screen.getByLabelText('schedules.cron')).toHaveValue('')
    expect(screen.getByRole('checkbox', { name: 'schedules.enabled' })).toBeChecked()
  })

  it('edits a failed schedule, locks pending controls and closes after success', () => {
    const view = setup(2)
    fireEvent.click(screen.getByRole('button', { name: 'schedules.editFor 0 4 2 * *' }))
    expect(screen.getByRole('form', { name: 'schedules.edit' })).toBeInTheDocument()
    expect(screen.getByLabelText('schedules.cron')).toHaveValue('0 4 2 * *')
    expect(screen.getByRole('checkbox', { name: 'schedules.enabled' })).not.toBeChecked()
    fireEvent.change(screen.getByLabelText('schedules.cron'), { target: { value: '0 5 * * 1' } })
    fireEvent.click(screen.getByRole('checkbox', { name: 'schedules.enabled' }))
    fireEvent.submit(screen.getByRole('form', { name: 'schedules.edit' }))
    expect(mocks.update).toHaveBeenCalledWith(
      { id: 'schedule-2', cron: '0 5 * * 1', git_ref: 'release/02', enabled: true },
      expect.objectContaining({ onSuccess: expect.any(Function) }),
    )
    mocks.updatePending = true
    view.rerender(page())
    expect(screen.getByRole('form', { name: 'schedules.edit' })).toHaveAttribute('aria-busy', 'true')
    expect(screen.getByLabelText('schedules.cron')).toBeDisabled()
    expect(screen.getByRole('button', { name: 'common.cancel' })).toBeDisabled()
    mocks.updatePending = false
    view.rerender(page())
    act(() => mocks.update.mock.calls[0]![1].onSuccess())
    expect(screen.queryByRole('form', { name: 'schedules.edit' })).not.toBeInTheDocument()
    expect(toast.success).toHaveBeenCalledWith('schedules.updated')
  })

  it('keeps confirmation open on delete failure and prevents duplicate pending requests', () => {
    const view = setup(1)
    fireEvent.click(screen.getByRole('button', { name: 'schedules.deleteFor 0 4 1 * *' }))
    fireEvent.click(screen.getByRole('button', { name: 'common.cancel' }))
    expect(mocks.remove).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'schedules.deleteFor 0 4 1 * *' }))
    fireEvent.click(screen.getByRole('button', { name: 'common.delete' }))
    expect(mocks.remove).toHaveBeenCalledTimes(1)
    mocks.deletePending = true
    view.rerender(page())
    expect(screen.getByRole('button', { name: 'common.delete' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'common.cancel' })).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: 'common.delete' }))
    expect(mocks.remove).toHaveBeenCalledTimes(1)
    mocks.deletePending = false
    view.rerender(page())
    act(() => mocks.remove.mock.calls[0]![1].onError(new Error('failure')))
    expect(screen.getByRole('alertdialog')).toHaveTextContent('schedules.deleteError')
    expect(screen.getByRole('alert')).toHaveTextContent('schedules.deleteError')
    fireEvent.click(screen.getByRole('button', { name: 'common.delete' }))
    expect(mocks.remove).toHaveBeenCalledTimes(2)
    act(() => mocks.remove.mock.calls[1]![1].onSuccess())
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument()
    expect(toast.success).toHaveBeenCalledWith('schedules.deleted')
  })
})
