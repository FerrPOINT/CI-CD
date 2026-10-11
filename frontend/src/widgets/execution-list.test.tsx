import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter, useLocation, useNavigate } from 'react-router'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { api, ApiError } from '@/api/client'
import { ExecutionList } from './execution-list'

vi.mock('@/api/client', async (importOriginal) => ({
  ...(await importOriginal<object>()),
  api: vi.fn(),
}))
const namespace = {
  registry_instance_id: '11111111-1111-4111-8111-111111111111',
  namespace_id: '22222222-2222-4222-8222-222222222222',
}
const initial = `/workspaces/${namespace.registry_instance_id}/${namespace.namespace_id}/pipelines?project_scope=all&registry_instance_id=${namespace.registry_instance_id}&namespace_id=${namespace.namespace_id}`
function HistoryProbe() {
  const location = useLocation(),
    navigate = useNavigate()
  return (
    <>
      <output data-testid="location">{location.search}</output>
      <button onClick={() => navigate(-1)}>Browser back</button>
    </>
  )
}
function mount(deployment = false, suffix = '', preview = false) {
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <MemoryRouter initialEntries={[initial + suffix]}>
        <HistoryProbe />
        <ExecutionList
          namespace={namespace}
          deployment={deployment}
          preview={preview}
        />
      </MemoryRouter>
    </QueryClientProvider>,
  )
}
beforeEach(() => {
  vi.mocked(api).mockImplementation(async (path) => {
    if (path.endsWith('/summary')) return { configurations: 1 }
    if (
      path.startsWith('/workspace-projects/') &&
      !path.includes('/pipelines') &&
      !path.includes('/deployments')
    )
      return { group_id: 'group' }
    return { items: [], total: 0 }
  })
})
afterEach(() => {
  cleanup()
  vi.resetAllMocks()
})

it('applies the draft once and browser Back restores the whole filter state', async () => {
  mount()
  await screen.findByText('Нет CI-конфигураций.')
  fireEvent.click(screen.getByRole('button', { name: 'Фильтры' }))
  const input = screen.getByRole('textbox', { name: 'Ветка / тег / коммит' })
  for (const value of ['m', 'ma', 'mai', 'main'])
    fireEvent.change(input, { target: { value } })
  expect(screen.getByTestId('location')).not.toHaveTextContent('git_ref')
  expect(
    vi
      .mocked(api)
      .mock.calls.some(
        ([path]) => path.includes('/pipelines?') && path.includes('git_ref'),
      ),
  ).toBe(false)
  fireEvent.click(screen.getByRole('button', { name: 'Применить' }))
  await waitFor(() =>
    expect(screen.getByTestId('location')).toHaveTextContent('git_ref=main'),
  )
  expect(screen.getByTestId('location')).toHaveTextContent('project_scope=all')
  fireEvent.click(screen.getByRole('button', { name: 'Browser back' }))
  await waitFor(() =>
    expect(screen.getByTestId('location')).not.toHaveTextContent('git_ref'),
  )
})

it('keeps an invalid period in the form without replacing results or issuing a list request', async () => {
  mount()
  fireEvent.click(screen.getByRole('button', { name: 'Фильтры' }))
  fireEvent.change(screen.getByLabelText('Дата начала'), {
    target: { value: '2026-10-12' },
  })
  fireEvent.change(screen.getByLabelText('Дата окончания'), {
    target: { value: '2026-10-10' },
  })
  fireEvent.click(screen.getByRole('button', { name: 'Применить' }))
  expect(await screen.findByRole('alert')).toHaveTextContent(
    'Дата начала позже даты окончания',
  )
  expect(screen.getByRole('dialog')).toBeInTheDocument()
  expect(screen.getByTestId('location')).not.toHaveTextContent('since=')
})

it('distinguishes no matches and resets only filter parameters', async () => {
  mount(false, '&status=queued')
  expect(
    await screen.findByText('По выбранным фильтрам ничего не найдено.'),
  ).toBeInTheDocument()
  fireEvent.click(screen.getByRole('button', { name: 'Сбросить фильтры' }))
  await waitFor(() =>
    expect(screen.getByTestId('location')).not.toHaveTextContent('status='),
  )
  expect(screen.getByTestId('location')).toHaveTextContent(
    namespace.namespace_id,
  )
  expect(screen.getByTestId('location')).toHaveTextContent('project_scope=all')
})

it('links a deployment to its exact environment and keeps the pipeline as a separate action', async () => {
  vi.mocked(api).mockImplementation(async (path) =>
    path.includes('/deployments?')
      ? {
          total: 1,
          items: [
            {
              namespace,
              id: 'deployment',
              repository_id: 'repo',
              repository_name: 'project/api',
              configuration_id: 'config',
              configuration_name: 'Build',
              status: 'success',
              git_ref: 'main',
              created_at: '2026-10-10T00:00:00Z',
              environment_id: 'environment',
              environment_name: 'Production',
              approval_state: 'approved',
              pipeline_id: 'pipeline',
            },
          ],
        }
      : { items: [], total: 0 },
  )
  mount(true)
  const target = await screen.findByRole('link', { name: 'Деплой #deployme' })
  expect(target.getAttribute('href')).toContain(
    '/delivery-configs/config/environments?environment_id=environment&deployment_id=deployment',
  )
  expect(
    screen.getByRole('link', { name: 'Пайплайн' }).getAttribute('href'),
  ).toContain('/pipelines/pipeline')
})

it('overview preview requests five rows and omits filters and pagination', async () => {
  mount(false, '&status=failed&offset=50', true)
  await waitFor(() =>
    expect(
      vi
        .mocked(api)
        .mock.calls.some(([path]) =>
          path.endsWith('/pipelines?limit=5&offset=0'),
        ),
    ).toBe(true),
  )
  expect(
    screen.queryByRole('button', { name: /Фильтры/ }),
  ).not.toBeInTheDocument()
  expect(
    screen.queryByRole('button', { name: 'Далее' }),
  ).not.toBeInTheDocument()
})

it.each([
  { status: 403, retained: false },
  { status: 503, retained: true },
])(
  'handles refresh failure $status without exposing denied cached rows',
  async ({ status, retained }) => {
    let fail = false
    vi.mocked(api).mockImplementation(async (path) => {
      if (path.includes('/pipelines?')) {
        if (fail)
          throw new ApiError({ kind: 'api', status, message: 'unavailable' })
        return {
          total: 1,
          items: [
            {
              namespace,
              id: 'run',
              repository_id: 'repo',
              repository_name: 'project/api',
              configuration_id: 'config',
              configuration_name: 'Build',
              status: 'success',
              git_ref: 'main',
              created_at: '2026-10-10T00:00:00Z',
              pipeline_id: 'run',
            },
          ],
        }
      }
      return { items: [], total: 0 }
    })
    mount()
    await screen.findByRole('link', { name: 'Пайплайн #run' })
    fail = true
    fireEvent.click(screen.getByRole('button', { name: 'Обновить' }))
    await screen.findByRole('alert')
    expect(!!screen.queryByRole('link', { name: 'Пайплайн #run' })).toBe(
      retained,
    )
  },
)
