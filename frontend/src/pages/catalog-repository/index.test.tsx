import { cleanup, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter, Route, Routes } from 'react-router'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { api } from '@/api/client'
import { CatalogRepositoryPage } from './index'

vi.mock('@/api/client', () => ({ api: vi.fn() }))

const registry = '11111111-1111-4111-8111-111111111111'
const namespace = '22222222-2222-4222-8222-222222222222'
const repository = '33333333-3333-4333-8333-333333333333'

beforeEach(() => {
  vi.mocked(api).mockImplementation(async (path) => {
    if (path === `/catalog/repositories/${repository}`)
      return {
        id: repository,
        namespace: { registry_instance_id: registry, namespace_id: namespace },
        public_name: 'project/api',
        kind: 'external',
        ready: true,
        state: 'active',
        external_url: 'https://example.test/api.git',
      }
    if (path.startsWith('/workspace-projects/'))
      return { name: 'Same name', project_key: 'ONE', group_id: repository }
    if (path.startsWith('/namespace-contexts/')) return {}
    return { items: [], total: 0 }
  })
})

afterEach(() => {
  cleanup()
  vi.resetAllMocks()
})

function mount(selectedNamespace: string, tab: string) {
  const path = `/catalog/repositories/${repository}?tab=${tab}&registry_instance_id=${registry}&namespace_id=${selectedNamespace}`
  render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="/catalog/repositories/:id" element={<CatalogRepositoryPage />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

it.each(['pipelines', 'deployments'])(
  'keeps repository %s configuration choices in the verified namespace',
  async (tab) => {
    mount(namespace, tab)
    await screen.findByRole('heading', { name: 'project/api' })
    await waitFor(() =>
      expect(api).toHaveBeenCalledWith(
        `/catalog/repositories/${repository}/delivery-configs?limit=100&search=&registry_instance_id=${registry}&namespace_id=${namespace}`,
        expect.objectContaining({ signal: expect.any(AbortSignal) }),
      ),
    )
    expect(api).toHaveBeenCalledWith(
      `/catalog/repositories/${repository}/${tab}?limit=50&offset=0`,
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    )
  },
)

it('rejects a foreign namespace before loading repository executions', async () => {
  mount('44444444-4444-4444-8444-444444444444', 'pipelines')
  expect(await screen.findByRole('alert')).toHaveTextContent(
    'Репозиторий принадлежит другому проекту',
  )
  expect(
    vi.mocked(api).mock.calls.some(([path]) =>
      path.startsWith(`/catalog/repositories/${repository}/pipelines`),
    ),
  ).toBe(false)
})
