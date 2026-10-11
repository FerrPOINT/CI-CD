import { cleanup, render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { api } from '@/api/client'
import { RepositoriesPage } from './index'
vi.mock('@/api/client', () => ({ api: vi.fn() }))
afterEach(() => {
  cleanup()
  vi.resetAllMocks()
})
describe('repository catalog identity', () => {
  it('keeps identical short slugs, external Git and missing storage separate', async () => {
    vi.mocked(api).mockResolvedValue({
      items: [
        {
          id: 'hosted',
          public_name: 'one/api',
          kind: 'hosted',
          ready: true,
          availability: 'missing',
          namespace: null,
        },
        {
          id: 'external',
          public_name: 'two/api',
          kind: 'external',
          ready: true,
          namespace: { registry_instance_id: 'registry', namespace_id: 'namespace' },
          state: 'active',
        },
      ],
    })
    render(
      <QueryClientProvider client={new QueryClient()}>
        <MemoryRouter>
          <RepositoriesPage />
        </MemoryRouter>
      </QueryClientProvider>,
    )
    expect((await screen.findByText('one/api')).closest('a')).toHaveAttribute(
      'href',
      '/catalog/repositories/hosted?project_scope=all',
    )
    expect(screen.getByText('two/api').closest('a')?.getAttribute('href')).toContain(
      '/catalog/repositories/external',
    )
    expect(screen.getByText(/Хранилище недоступно/)).toBeInTheDocument()
    expect(screen.getByText(/Внешний Git/)).toBeInTheDocument()
    expect(vi.mocked(api).mock.calls).toHaveLength(1)
  })
})
