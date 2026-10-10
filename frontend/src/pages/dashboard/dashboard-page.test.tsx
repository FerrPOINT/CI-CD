import { cleanup, render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { api } from '@/api/client'
import { DashboardPage } from './index'
vi.mock('@/api/client', () => ({ api: vi.fn() }))
function mount() {
  return render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <MemoryRouter>
        <DashboardPage />
      </MemoryRouter>
    </QueryClientProvider>,
  )
}
afterEach(() => {
  cleanup()
  vi.resetAllMocks()
})
describe('workspace dashboard', () => {
  it('separately counts projects, repositories and configurations without per-config requests', async () => {
    vi.mocked(api).mockImplementation(async (path) =>
      path === '/runners'
        ? []
        : path === '/workspace-summary'
          ? { projects: 2, repositories: 3, configurations: 4, queued: 5, running: 6, failed: 7 }
          : path.startsWith('/catalog')
            ? { items: [] }
            : { items: [], total: 0 },
    )
    mount()
    await screen.findByText('CI-конфигурации')
    expect(screen.getByText('Проекты').parentElement).toHaveTextContent('2')
    expect(screen.getByText('Репозитории').parentElement).toHaveTextContent('3')
    expect(screen.getByText('CI-конфигурации').parentElement).toHaveTextContent('4')
    expect(vi.mocked(api).mock.calls.some(([path]) => path.startsWith('/projects'))).toBe(false)
  })
  it('reports unavailable counters while keeping independently available execution history', async () => {
    vi.mocked(api).mockImplementation(async (path) => {
      if (path === '/runners') return []
      if (path === '/workspace-summary') throw new Error('unavailable')
      if (path.startsWith('/workspace-pipelines'))
        return {
          total: 1,
          items: [
            {
              id: 'run',
              repository_id: 'repo',
              repository_name: 'group/api',
              configuration_id: 'config',
              configuration_name: 'Build',
              git_ref: 'main',
              status: 'success',
              created_at: '2026-10-09T00:00:00Z',
              pipeline_id: 'run',
            },
          ],
        }
      return { items: [], total: 0 }
    })
    mount()
    expect(await screen.findByRole('alert')).toHaveTextContent('Сводка недоступна')
    expect(await screen.findByText('group/api')).toBeInTheDocument()
    expect(screen.queryByText('Проекты')).toBeNull()
  })
})
