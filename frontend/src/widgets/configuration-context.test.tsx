import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { acceptSso, clearSession } from '@/api/auth'
import { SecretsPage } from '@/pages/secrets'
import { SchedulesPage } from '@/pages/schedules'
import { ConfigurationContext } from './configuration-context'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: { name?: string }) =>
      options?.name ? `${key} ${options.name}` : key,
  }),
}))
vi.mock('@pdlc/ui/ui', async (importOriginal) => ({...(await importOriginal<typeof import('@pdlc/ui/ui')>()),...await (() => ({ toast: { success: vi.fn() } }))()}))

const first = '11111111-1111-4111-8111-111111111111'
const second = '22222222-2222-4222-8222-222222222222'
const now = '2026-10-15T00:00:00Z'

function mount(section: 'secrets' | 'schedules') {
  acceptSso('synthetic-test-access', Date.now() + 3600_000, 'fixture')
  const writes: Array<{ path: string; method: string; body: unknown }> = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = new URL(String(input), 'http://localhost').pathname
      const method = init?.method ?? 'GET'
      if (method !== 'GET')
        writes.push({
          path,
          method,
          body: init?.body ? JSON.parse(String(init.body)) : null,
        })
      const id = path.includes(first) ? first : second
      const body =
        section === 'secrets'
          ? [
              {
                id: `secret-${id}`,
                project_id: id,
                key: `SECRET_${id === first ? 'A' : 'B'}`,
                created_at: now,
                updated_at: now,
              },
            ]
          : [
              {
                id: `schedule-${id}`,
                project_id: id,
                cron: id === first ? '0 4 * * *' : '0 5 * * *',
                git_ref: 'main',
                enabled: true,
                created_at: now,
              },
            ]
      return new Response(JSON.stringify(method === 'GET' ? body : body[0]), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    }),
  )
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, staleTime: Infinity },
      mutations: { retry: false },
    },
  })
  for (const id of [first, second]) {
    client.setQueryData(['delivery-config', undefined, undefined, id], {
      id,
      name: id === first ? 'Configuration A' : 'Configuration B',
      repository_id: null,
    })
  }
  const router = createMemoryRouter(
    [
      {
        path: `/delivery-configs/:projectId/${section}`,
        element: (
          <ConfigurationContext>
            {section === 'secrets' ? <SecretsPage /> : <SchedulesPage />}
          </ConfigurationContext>
        ),
      },
    ],
    { initialEntries: [`/delivery-configs/${first}/${section}`] },
  )
  render(
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  )
  return { router, writes, client }
}

afterEach(() => {
  clearSession()
  vi.unstubAllGlobals()
})

describe('configuration-owned page state', () => {
  it('preserves the current draft when metadata of the same configuration refreshes', async () => {
    const { client, writes } = mount('secrets')
    await screen.findByText('SECRET_A')
    fireEvent.click(screen.getByRole('button', { name: 'secrets.add' }))
    fireEvent.change(screen.getByLabelText('secrets.key'), {
      target: { value: 'A_KEY' },
    })
    fireEvent.change(screen.getByLabelText('secrets.value'), {
      target: { value: 'synthetic-a-value' },
    })
    act(() =>
      client.setQueryData(['delivery-config', undefined, undefined, first], {
        id: first,
        name: 'Renamed A',
        repository_id: null,
      }),
    )
    await screen.findByText('CI: Renamed A')
    expect(screen.getByLabelText('secrets.key')).toHaveValue('A_KEY')
    expect(screen.getByLabelText('secrets.value')).toHaveValue(
      'synthetic-a-value',
    )
    expect(writes).toEqual([])
  })

  it('clears an unsaved secret on a cached configuration change and saves only new input to the new scope', async () => {
    const { router, writes } = mount('secrets')
    await screen.findByText('SECRET_A')
    fireEvent.click(screen.getByRole('button', { name: 'secrets.add' }))
    fireEvent.change(screen.getByLabelText('secrets.key'), {
      target: { value: 'A_PRIVATE_KEY' },
    })
    fireEvent.change(screen.getByLabelText('secrets.value'), {
      target: { value: 'synthetic-a-value' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'secrets.showValue' }))
    await act(async () =>
      router.navigate(`/delivery-configs/${second}/secrets`),
    )
    await screen.findByText('SECRET_B')
    expect(
      screen.queryByRole('form', { name: 'secrets.add' }),
    ).not.toBeInTheDocument()
    expect(writes).toEqual([])
    fireEvent.click(screen.getByRole('button', { name: 'secrets.add' }))
    expect(screen.getByLabelText('secrets.key')).toHaveValue('')
    expect(screen.getByLabelText('secrets.value')).toHaveValue('')
    expect(screen.getByLabelText('secrets.value')).toHaveAttribute(
      'type',
      'password',
    )
    fireEvent.change(screen.getByLabelText('secrets.key'), {
      target: { value: 'B_KEY' },
    })
    fireEvent.change(screen.getByLabelText('secrets.value'), {
      target: { value: 'synthetic-b-value' },
    })
    fireEvent.submit(screen.getByRole('form', { name: 'secrets.add' }))
    await waitFor(() =>
      expect(writes).toEqual([
        {
          path: `/api/v1/projects/${second}/secrets`,
          method: 'POST',
          body: { key: 'B_KEY', value: 'synthetic-b-value' },
        },
      ]),
    )
  })

  it('dismisses a pending deletion from the previous configuration without sending a delete', async () => {
    const { router, writes } = mount('secrets')
    await screen.findByText('SECRET_A')
    fireEvent.click(
      screen.getByRole('button', { name: 'secrets.deleteFor SECRET_A' }),
    )
    expect(screen.getByRole('alertdialog')).toBeInTheDocument()
    await act(async () =>
      router.navigate(`/delivery-configs/${second}/secrets`),
    )
    await screen.findByText('SECRET_B')
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument()
    expect(writes).toEqual([])
  })

  it('clears a schedule edit on a cached configuration change before creating in the new scope', async () => {
    const { router, writes } = mount('schedules')
    await screen.findByText('0 4 * * *')
    fireEvent.click(
      screen.getByRole('button', { name: 'schedules.editFor 0 4 * * *' }),
    )
    fireEvent.change(screen.getByLabelText('schedules.cron'), {
      target: { value: '0 1 * * *' },
    })
    await act(async () =>
      router.navigate(`/delivery-configs/${second}/schedules`),
    )
    await screen.findByText('0 5 * * *')
    expect(
      screen.queryByRole('form', { name: 'schedules.edit' }),
    ).not.toBeInTheDocument()
    expect(writes).toEqual([])
    fireEvent.click(screen.getByRole('button', { name: 'schedules.create' }))
    expect(screen.getByLabelText('schedules.cron')).toHaveValue('')
    fireEvent.change(screen.getByLabelText('schedules.cron'), {
      target: { value: '0 2 * * *' },
    })
    fireEvent.submit(screen.getByRole('form', { name: 'schedules.create' }))
    await waitFor(() =>
      expect(writes).toEqual([
        {
          path: `/api/v1/projects/${second}/schedules`,
          method: 'POST',
          body: { cron: '0 2 * * *', git_ref: 'main', enabled: true },
        },
      ]),
    )
  })
})
