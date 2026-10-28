import { cleanup, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { useApplicationAccess } from '@pdlc/ui/auth'
import { CurrentApplicationAccess } from './current-application-access'

const current = vi.hoisted(() => ({ token: null as string | null, retire: vi.fn() }))
vi.mock('./auth-provider', () => ({
  ssoConfig: { issuer: 'https://auth.fixture.test' },
  useAuth: () => ({ session: current.token ? { access_token: current.token } : null, invalidate: current.retire }),
}))
vi.mock('@/api/auth', () => ({ clearSession: vi.fn() }))

function View() {
  const access = useApplicationAccess()!
  return <output data-testid="access">{access.status}:{access.access?.decision.subject ?? 'closed'}:{access.permits(['recruiter']) ? 'yes' : 'no'}</output>
}
function tree() { return <CurrentApplicationAccess><View /></CurrentApplicationAccess> }
function token(subject: string) {
  return `header.${btoa(JSON.stringify({ sub: subject, exp: Math.floor(Date.now() / 1000) + 300 }))}.signature`
}
function response(subject: string, role: 'admin' | 'user') {
  return new Response(JSON.stringify({ schema_version: 1, issuer: 'https://auth.fixture.test', subject, role,
    applications: role === 'admin' ? ['wiki', 'recruiter'] : ['wiki'], revision: 1, enforced: true,
    observed_at: new Date().toISOString(), expires_at: new Date(Date.now() + 30000).toISOString(),
  }), { headers: { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' } })
}
beforeEach(() => {
  current.token = token('administrator')
  current.retire.mockClear()
  window.addEventListener('base-admin:unauthorized', current.retire)
})
afterEach(() => {
  cleanup()
  window.removeEventListener('base-admin:unauthorized', current.retire)
  vi.unstubAllGlobals()
})
it('confirms admin access and retires it immediately on account change and logout', async () => {
  const fetch = vi.fn().mockResolvedValueOnce(response('administrator', 'admin')).mockResolvedValueOnce(response('ordinary', 'user'))
  vi.stubGlobal('fetch', fetch)
  const view = render(tree())
  await waitFor(() => expect(screen.getByTestId('access').textContent).toBe('active:administrator:yes'))
  expect(fetch.mock.calls[0]![0]).toBe('https://auth.fixture.test/auth/access')
  expect(fetch.mock.calls[0]![1].headers.Authorization).toBe(`Bearer ${current.token}`)
  current.token = token('ordinary')
  view.rerender(tree())
  expect(screen.getByTestId('access').textContent).not.toContain('administrator:yes')
  await waitFor(() => expect(screen.getByTestId('access').textContent).toBe('active:ordinary:no'))
  current.token = null
  view.rerender(tree())
  expect(screen.getByTestId('access').textContent).toBe('signed-out:closed:no')
})
it('keeps navigation closed on an owner outage', async () => {
  vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('offline')))
  render(tree())
  await waitFor(() => expect(screen.getByTestId('access').textContent).toBe('unavailable:closed:no'))
})
it('passes revoked sessions to the existing local retirement path', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('', { status: 401 })))
  render(tree())
  await waitFor(() => expect(current.retire).toHaveBeenCalled())
  expect(screen.getByTestId('access').textContent).not.toContain(':yes')
})
