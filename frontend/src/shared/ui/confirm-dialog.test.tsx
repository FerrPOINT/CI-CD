import { describe, expect, it, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

import { ConfirmDialog } from './confirm-dialog'

describe('ConfirmDialog', () => {
  it('is hidden until open', () => {
    render(<ConfirmDialog open={false} onConfirm={() => {}} onCancel={() => {}} title="t" description="d" />)
    expect(screen.queryByRole('alertdialog')).toBeNull()
  })

  it('shows title and triggers confirm', () => {
    const onConfirm = vi.fn()
    render(<ConfirmDialog open onConfirm={onConfirm} onCancel={() => {}} title="Delete runner" confirmLabel="Delete" description="Are you sure?" />)

    expect(screen.getByRole('alertdialog')).toBeDefined()
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }))
    expect(onConfirm).toHaveBeenCalledOnce()
  })

  it('cancel does not confirm', () => {
    const onConfirm = vi.fn()
    const onCancel = vi.fn()
    render(<ConfirmDialog open onConfirm={onConfirm} onCancel={onCancel} title="t" description="d" confirmLabel="Delete" cancelLabel="Cancel" />)

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(onConfirm).not.toHaveBeenCalled()
    expect(onCancel).toHaveBeenCalled()
  })
})
