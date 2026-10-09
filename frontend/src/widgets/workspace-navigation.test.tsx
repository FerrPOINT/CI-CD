import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter, useLocation } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { api } from '@/api/client'
import { WorkspaceNavigation, useProjectDisclosures } from './workspace-navigation'
vi.mock('@/api/client', () => ({ api: vi.fn() }))
const registry = '11111111-1111-4111-8111-111111111111'
const first = '22222222-2222-4222-8222-222222222222'
const second = '33333333-3333-4333-8333-333333333333'
const projects = [first, second].map((id, index) => ({
  registry_instance_id: registry,
  namespace_id: id,
  tracker_instance_id: registry,
  tracker_project_id: id,
  name: 'Same name',
  project_key: index ? 'TWO' : 'ONE',
}))
function View() {
  const disclosures = useProjectDisclosures()
  const location = useLocation()
  return (
    <>
      <WorkspaceNavigation {...disclosures} />
      <output>{location.pathname + location.search}</output>
    </>
  )
}
function mount(path: string) {
  return render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <MemoryRouter initialEntries={[path]}>
        <View />
      </MemoryRouter>
    </QueryClientProvider>,
  )
}
beforeEach(() => {
  localStorage.clear()
  vi.mocked(api).mockResolvedValue({ items: projects, total: 2 })
})
afterEach(() => {
  cleanup()
  vi.resetAllMocks()
})
describe('project navigation scope and persisted disclosures', () => {
  it('keeps every section after navigating inside a project in all-project mode', async () => {
    mount('/projects?project_scope=all')
    await screen.findByRole('button', { name: 'Same name · ONE' })
    fireEvent.click(screen.getAllByRole('link', { name: 'Пайплайны' })[0])
    expect(screen.getByRole('button', { name: 'Same name · TWO' })).toBeInTheDocument()
    expect(screen.getByRole('status')).toHaveTextContent('project_scope=all')
    expect(screen.getByRole('status')).toHaveTextContent(first)
  })
  it('persists folds by ResourceRef after refresh while selection filters sections', async () => {
    const view = mount('/projects?project_scope=all')
    const toggle = await screen.findByRole('button', { name: 'Same name · TWO' })
    fireEvent.click(toggle)
    expect(toggle).toHaveAttribute('aria-expanded', 'false')
    await waitFor(() =>
      expect(localStorage.getItem('forge:collapsed-projects:v1')).toContain(second),
    )
    view.unmount()
    mount(
      `/workspaces/${registry}/${second}?registry_instance_id=${registry}&namespace_id=${second}`,
    )
    expect(await screen.findByRole('button', { name: 'Same name · TWO' })).toHaveAttribute(
      'aria-expanded',
      'false',
    )
    expect(screen.queryByRole('button', { name: 'Same name · ONE' })).toBeNull()
  })
})
