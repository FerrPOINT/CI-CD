import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { AuditLogPage } from './index'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    i18n: { language: 'ru' },
    t: (key: string, options?: { defaultValue?: string; count?: number }) =>
      options?.defaultValue ?? (options?.count === undefined ? key : `${key}: ${options.count}`),
  }),
}))

vi.mock('@/api/hooks', () => ({
  useAuditLog: () => ({
    data: Array.from({ length: 25 }, (_, index) => ({
      id: index,
      action: index < 22 ? 'auth.login_success' : 'auth.login_failed',
      resource_type: 'user',
      resource_id: `user-${index}`,
      actor: `qa-${index}`,
      created_at: '2026-09-19T10:00:00Z',
    })),
    isLoading: false,
    error: null,
  }),
}))

afterEach(cleanup)

describe('AuditLogPage', () => {
  it('paginates the latest events and filters by action', () => {
    render(<AuditLogPage />)
    expect(screen.getByText('1 / 2')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'auditLog.next' }))
    expect(screen.getByText('2 / 2')).toBeInTheDocument()

    fireEvent.change(screen.getByRole('combobox', { name: 'auditLog.actionFilter' }), {
      target: { value: 'auth.login_failed' },
    })
    expect(screen.queryByText('2 / 2')).not.toBeInTheDocument()
    expect(screen.getByText('auditLog.resultCount: 3')).toBeInTheDocument()
    expect(screen.queryByText('qa-0')).not.toBeInTheDocument()
  })

  it('searches actors and shows a no-results state', () => {
    render(<AuditLogPage />)
    const search = screen.getByRole('searchbox', { name: 'auditLog.search' })
    fireEvent.change(search, { target: { value: 'qa-24' } })
    expect(screen.queryAllByText('qa-24')).toHaveLength(2)
    expect(screen.queryByText('qa-0')).not.toBeInTheDocument()

    fireEvent.change(search, { target: { value: 'missing' } })
    expect(screen.getByRole('status')).toHaveTextContent('auditLog.noMatches')
  })
})
