import { MemoryRouter } from 'react-router'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/api/client'
import { QueryState } from './query-state'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

describe('QueryState', () => {
  it('[REQ-UI-002] renders the accessible loading contract', () => {
    render(
      <QueryState data={undefined} isLoading error={null}>
        {() => <div>data</div>}
      </QueryState>,
    )

    expect(screen.getByLabelText('common.loading')).toHaveAttribute('aria-busy', 'true')
  })

  it('[REQ-UI-002] renders an explicit empty state instead of children', () => {
    render(
      <QueryState data={[]} isLoading={false} error={null} isEmpty={(items) => items.length === 0} empty={{ title: 'Nothing here' }}>
        {() => <div>data</div>}
      </QueryState>,
    )

    expect(screen.getByText('Nothing here')).toBeInTheDocument()
    expect(screen.queryByText('data')).toBeNull()
  })

  it('[REQ-UI-002] turns authorization failures into the forbidden page', () => {
    render(
      <MemoryRouter>
        <QueryState
          data={undefined}
          isLoading={false}
          error={new ApiError({ kind: 'api', status: 403, message: 'forbidden' })}
        >
          {() => <div>data</div>}
        </QueryState>
      </MemoryRouter>,
    )

    expect(screen.getByText('errors.forbidden.title')).toBeInTheDocument()
  })
})
