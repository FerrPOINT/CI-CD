import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { AuditLogPage } from './index'

const mocks = vi.hoisted(() => ({
  refetch: vi.fn(),
  useAuditLog: vi.fn(),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    i18n: { language: 'ru' },
    t: (key: string, options?: { defaultValue?: string }) => options?.defaultValue ?? key,
  }),
}))

vi.mock('@/api/hooks', () => ({
  useAuditLog: mocks.useAuditLog,
}))

type AuditFilters = {
  action?: string
  q?: string
  limit?: number
  offset?: number
}

function auditResult(filters: AuditFilters, total = filters.q === 'missing' ? 0 : 45) {
  const offset = filters.offset ?? 0
  const itemCount = Math.min(20, Math.max(0, total - offset))
  return {
    data: {
      items: Array.from({ length: itemCount }, (_, index) => ({
        id: offset + index + 1,
        action: filters.action ?? (index % 2 === 0 ? 'auth.login_success' : 'auth.login_failed'),
        resource_type: 'user',
        resource_id: `00000000-0000-4000-8000-${String(offset + index + 1).padStart(12, '0')}`,
        actor: `qa-${offset + index + 1}`,
        created_at: '2026-09-19T10:00:00Z',
      })),
      total,
      limit: 20,
      offset,
      actions: ['auth.login_failed', 'auth.login_success'],
    },
    isLoading: false,
    isFetching: false,
    isPlaceholderData: false,
    error: null,
    refetch: mocks.refetch,
  }
}

function renderPage(entry = '/audit-log') {
  return render(
    <MemoryRouter initialEntries={[entry]}>
      <Routes>
        <Route path="/audit-log" element={<AuditLogPage />} />
      </Routes>
    </MemoryRouter>,
  )
}

beforeEach(() => {
  mocks.refetch.mockReset()
  mocks.useAuditLog.mockReset()
  mocks.useAuditLog.mockImplementation((filters: AuditFilters) => auditResult(filters))
})

afterEach(cleanup)

describe('AuditLogPage', () => {
  it('restores server filters from the URL and resets the page when they change', async () => {
    renderPage('/audit-log?page=2&action=auth.login_failed&q=qa')

    expect(mocks.useAuditLog).toHaveBeenLastCalledWith({
      action: 'auth.login_failed',
      limit: 20,
      offset: 20,
      q: 'qa',
    })
    expect(screen.getByText('2 / 3')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'auditLog.next' }))
    await waitFor(() => expect(mocks.useAuditLog).toHaveBeenLastCalledWith({
      action: 'auth.login_failed',
      limit: 20,
      offset: 40,
      q: 'qa',
    }))

    fireEvent.change(screen.getByRole('combobox', { name: 'auditLog.actionFilter' }), {
      target: { value: 'auth.login_success' },
    })
    await waitFor(() => expect(mocks.useAuditLog).toHaveBeenLastCalledWith({
      action: 'auth.login_success',
      limit: 20,
      offset: 0,
      q: 'qa',
    }))
  })

  it('submits and clears server search while distinguishing no matches', async () => {
    renderPage()
    const search = screen.getByRole('searchbox', { name: 'auditLog.search' })

    fireEvent.change(search, { target: { value: ' missing ' } })
    fireEvent.click(screen.getByRole('button', { name: 'auditLog.searchButton' }))
    await waitFor(() => expect(mocks.useAuditLog).toHaveBeenLastCalledWith({
      action: undefined,
      limit: 20,
      offset: 0,
      q: 'missing',
    }))
    expect(screen.getByRole('status')).toHaveTextContent('auditLog.noMatches')

    fireEvent.click(screen.getByRole('button', { name: 'auditLog.clearSearch' }))
    await waitFor(() => expect(mocks.useAuditLog).toHaveBeenLastCalledWith({
      action: undefined,
      limit: 20,
      offset: 0,
      q: undefined,
    }))
  })

  it('clamps an empty page to the last page returned by the server', async () => {
    mocks.useAuditLog.mockImplementation((filters: AuditFilters) => auditResult(filters, 21))
    renderPage('/audit-log?page=5')

    await waitFor(() => expect(mocks.useAuditLog).toHaveBeenLastCalledWith({
      action: undefined,
      limit: 20,
      offset: 20,
      q: undefined,
    }))
    expect(screen.getByText('2 / 2')).toBeInTheDocument()
  })

  it('guards unsafe pages and exposes terminal retry', () => {
    mocks.useAuditLog.mockReturnValue({
      data: undefined,
      isLoading: false,
      isFetching: false,
      isPlaceholderData: false,
      error: new Error('offline'),
      refetch: mocks.refetch,
    })
    renderPage('/audit-log?page=999999999999')

    expect(mocks.useAuditLog).toHaveBeenLastCalledWith({
      action: undefined,
      limit: 20,
      offset: 0,
      q: undefined,
    })
    expect(screen.getByRole('alert')).toHaveTextContent('auditLog.loadFailed')
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }))
    expect(mocks.refetch).toHaveBeenCalledOnce()
  })
})
