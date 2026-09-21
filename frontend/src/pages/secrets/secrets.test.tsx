import { act, fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { toast } from 'sonner'
import { SecretsPage } from './index'

const mocks = vi.hoisted(() => ({
  useSecrets: vi.fn(),
  upsert: vi.fn(),
  remove: vi.fn(),
  refetch: vi.fn(),
  upsertPending: false,
  removePending: false,
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: { name?: string; count?: number; total?: number }) =>
      options?.name ? `${key} ${options.name}` :
        options?.count !== undefined ? `${key} ${options.count}/${options.total}` : key,
  }),
}))
vi.mock('@/api/hooks', () => ({
  useSecrets: mocks.useSecrets,
  useUpsertSecret: () => ({ mutate: mocks.upsert, isPending: mocks.upsertPending }),
  useDeleteSecret: () => ({ mutate: mocks.remove, isPending: mocks.removePending }),
}))
vi.mock('sonner', () => ({ toast: { success: vi.fn() } }))

function secret(index: number) {
  return {
    id: `secret-${index}`,
    project_id: 'project-1',
    key: `SECRET_${String(index).padStart(2, '0')}`,
    created_at: '2026-09-19T00:00:00Z',
    updated_at: '2026-09-20T00:00:00Z',
  }
}

function page() {
  return (
    <MemoryRouter initialEntries={['/projects/project-1/secrets']}>
      <Routes>
        <Route path="/projects/:projectId/secrets" element={<SecretsPage />} />
      </Routes>
    </MemoryRouter>
  )
}

function setup(count: number) {
  mocks.useSecrets.mockReturnValue({
    data: Array.from({ length: count }, (_, index) => secret(index + 1)),
    isLoading: false,
    error: null,
    refetch: mocks.refetch,
  })
  return render(page())
}

afterEach(() => {
  vi.clearAllMocks()
  mocks.upsertPending = false
  mocks.removePending = false
})

describe('SecretsPage', () => {
  it('shows compact searchable pages with touch-sized actions', () => {
    setup(25)

    expect(screen.getAllByRole('button', { name: /secrets.deleteFor/ })).toHaveLength(20)
    expect(screen.getByRole('button', { name: 'secrets.deleteFor SECRET_01' })).toHaveClass('h-10', 'w-10')
    expect(screen.getAllByText(/secrets.updated:/)[0]!.closest('time')).toHaveAttribute('datetime', '2026-09-20T00:00:00Z')
    fireEvent.click(screen.getByRole('button', { name: 'secrets.next' }))
    expect(screen.getAllByRole('button', { name: /secrets.deleteFor/ })).toHaveLength(5)
    fireEvent.change(screen.getByRole('searchbox', { name: 'secrets.search' }), { target: { value: 'secret_07' } })
    expect(screen.getByRole('button', { name: 'secrets.replaceFor SECRET_07' })).toBeInTheDocument()
    expect(screen.queryByRole('navigation', { name: 'secrets.pages' })).not.toBeInTheDocument()
    fireEvent.change(screen.getByRole('searchbox', { name: 'secrets.search' }), { target: { value: 'missing' } })
    expect(screen.getByRole('status')).toHaveTextContent('secrets.noMatches')
    expect(screen.queryByText('secrets.empty')).not.toBeInTheDocument()
  })

  it('distinguishes empty inventory from a list error and retries without stale rows', () => {
    const view = setup(0)
    expect(screen.getByText('secrets.empty')).toBeInTheDocument()

    mocks.useSecrets.mockReturnValue({ data: [secret(1)], isLoading: false, error: null, refetch: mocks.refetch })
    view.rerender(page())
    expect(screen.getByRole('button', { name: 'secrets.deleteFor SECRET_01' })).toBeInTheDocument()
    mocks.useSecrets.mockReturnValue({ data: [secret(1)], isLoading: false, error: new Error('raw 500'), refetch: mocks.refetch })
    view.rerender(page())
    expect(screen.getByRole('alert')).toHaveTextContent('secrets.loadError')
    expect(screen.queryByRole('button', { name: 'secrets.deleteFor SECRET_01' })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(mocks.refetch).toHaveBeenCalledOnce()
  })

  it('clears an unsaved value on cancel and only closes after a successful save', () => {
    setup(0)
    fireEvent.click(screen.getByRole('button', { name: 'secrets.add' }))
    fireEvent.change(screen.getByLabelText('secrets.key'), { target: { value: '  DEPLOY_TOKEN  ' } })
    fireEvent.change(screen.getByLabelText('secrets.value'), { target: { value: 'private-value' } })
    fireEvent.click(screen.getByRole('button', { name: 'secrets.showValue' }))
    expect(screen.getByLabelText('secrets.value')).toHaveAttribute('type', 'text')
    fireEvent.click(screen.getByRole('button', { name: 'common.cancel' }))
    fireEvent.click(screen.getByRole('button', { name: 'secrets.add' }))
    expect(screen.getByLabelText('secrets.value')).toHaveValue('')
    expect(screen.getByLabelText('secrets.key')).toHaveFocus()
    expect(screen.getByLabelText('secrets.value')).toHaveAttribute('type', 'password')

    fireEvent.change(screen.getByLabelText('secrets.key'), { target: { value: '  DEPLOY_TOKEN  ' } })
    fireEvent.change(screen.getByLabelText('secrets.value'), { target: { value: 'private-value' } })
    fireEvent.submit(screen.getByRole('form', { name: 'secrets.add' }))
    expect(mocks.upsert).toHaveBeenCalledWith(
      { key: 'DEPLOY_TOKEN', value: 'private-value' },
      { onSuccess: expect.any(Function), onError: expect.any(Function) },
    )
    act(() => mocks.upsert.mock.calls[0]![1].onError(new Error('failure')))
    expect(screen.getByRole('alert')).toHaveTextContent('secrets.saveError')
    expect(screen.getByLabelText('secrets.value')).toHaveValue('private-value')
    act(() => mocks.upsert.mock.calls[0]![1].onSuccess())
    expect(screen.queryByRole('form', { name: 'secrets.add' })).not.toBeInTheDocument()
    expect(toast.success).toHaveBeenCalledWith('secrets.saved')
  })

  it('makes replacement explicit and preserves the key during rotation', () => {
    setup(1)
    fireEvent.click(screen.getByRole('button', { name: 'secrets.replaceFor SECRET_01' }))
    expect(screen.getByRole('form', { name: 'secrets.replace' })).toHaveTextContent('secrets.replaceWarning')
    expect(screen.getByRole('form', { name: 'secrets.replace' })).toHaveTextContent('SECRET_01')
    expect(screen.queryByLabelText('secrets.key')).not.toBeInTheDocument()
    expect(screen.getByLabelText('secrets.value')).toHaveValue('')
    expect(screen.getByLabelText('secrets.value')).toHaveFocus()
    fireEvent.change(screen.getByLabelText('secrets.value'), { target: { value: 'rotated' } })
    fireEvent.submit(screen.getByRole('form', { name: 'secrets.replace' }))
    expect(mocks.upsert).toHaveBeenCalledWith(
      { key: 'SECRET_01', value: 'rotated' },
      expect.objectContaining({ onSuccess: expect.any(Function) }),
    )
    act(() => mocks.upsert.mock.calls[0]![1].onSuccess())
    expect(toast.success).toHaveBeenCalledWith('secrets.replaced')
  })

  it('labels a duplicate key as replacement and locks the draft while saving', () => {
    const view = setup(1)
    fireEvent.click(screen.getByRole('button', { name: 'secrets.add' }))
    fireEvent.change(screen.getByLabelText('secrets.key'), { target: { value: 'SECRET_01' } })
    expect(screen.getByText('secrets.replaceWarning')).toBeInTheDocument()
    fireEvent.change(screen.getByLabelText('secrets.value'), { target: { value: 'new-value' } })
    fireEvent.click(screen.getByRole('button', { name: 'secrets.replace' }))
    expect(mocks.upsert).toHaveBeenCalledTimes(1)

    mocks.upsertPending = true
    view.rerender(page())
    expect(screen.getByRole('form', { name: 'secrets.add' })).toHaveAttribute('aria-busy', 'true')
    expect(screen.getByLabelText('secrets.key')).toBeDisabled()
    expect(screen.getByLabelText('secrets.value')).toBeDisabled()
    expect(screen.getByRole('button', { name: 'secrets.replace' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'common.cancel' })).toBeDisabled()
  })

  it('keeps deletion open through failure and pending, then closes after success', () => {
    const view = setup(1)
    fireEvent.click(screen.getByRole('button', { name: 'secrets.deleteFor SECRET_01' }))
    expect(screen.getByRole('alertdialog')).toHaveTextContent('secrets.deleteWarning')
    fireEvent.click(screen.getByRole('button', { name: 'common.cancel' }))
    expect(mocks.remove).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: 'secrets.deleteFor SECRET_01' }))
    fireEvent.click(screen.getByRole('button', { name: 'common.delete' }))
    expect(mocks.remove).toHaveBeenCalledTimes(1)
    expect(mocks.remove.mock.calls[0]![0]).toBe('secret-1')
    mocks.removePending = true
    view.rerender(page())
    expect(screen.getByRole('button', { name: 'common.delete' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'common.cancel' })).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: 'common.delete' }))
    expect(mocks.remove).toHaveBeenCalledTimes(1)

    mocks.removePending = false
    view.rerender(page())
    act(() => mocks.remove.mock.calls[0]![1].onError(new Error('failure')))
    expect(screen.getByRole('alertdialog')).toBeInTheDocument()
    expect(screen.getByRole('alert')).toHaveTextContent('secrets.deleteError')
    fireEvent.click(screen.getByRole('button', { name: 'common.delete' }))
    expect(mocks.remove).toHaveBeenCalledTimes(2)
    act(() => mocks.remove.mock.calls[1]![1].onSuccess())
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument()
    expect(toast.success).toHaveBeenCalledWith('secrets.deleted')
  })
})
