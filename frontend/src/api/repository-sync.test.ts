import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { acceptSso, clearSession } from './auth'
import { ApiError, apiRetry } from './client'
import {
  repositorySync,
  type Page,
  type SaveSync,
  type SyncConnection,
  type SyncRef,
  type SyncRun,
  type SyncSettings,
} from './repository-sync'

type Fetch = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>

const settings: SyncSettings = {
  provider: 'github', instance_url: 'https://github.com',
  repository_path: 'fixture-team/service', direction: 'both', automatic: true,
}
const connection: SyncConnection = {
  repository_id: '40000000-0000-4000-8000-000000000001',
  repository_name: 'fixture-platform/service', version: 7, settings,
  has_credential: true, writable: true, worker_enabled: true,
  active_run: null, last_run: null, paused_reason: null, next_check_at: null,
}
const run: SyncRun = {
  id: '50000000-0000-4000-8000-000000000001',
  repository_id: connection.repository_id, connection_version: 7, settings,
  source: 'manual', state: 'queued', cancel_requested: false,
  created_at: '2026-10-18T10:00:00Z', started_at: null, finished_at: null, message: null,
}
const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), {
  status, headers: { 'Content-Type': 'application/json' },
})
const storedValues = (storage: Storage) => Array.from({ length: storage.length }, (_, index) => {
  const key = storage.key(index)!
  return [key, storage.getItem(key)]
})

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  acceptSso('fixture-central-access', Date.now() + 3_600_000, 'fixture-reader')
})
afterEach(() => {
  clearSession()
  vi.unstubAllGlobals()
})

describe('repository synchronization HTTP boundary', () => {
  it('preserves server filters, page totals and cancellation through the authenticated list request', async () => {
    const page: Page<SyncConnection> = { items: [connection], total: 41 }
    const fetchMock = vi.fn<Fetch>().mockResolvedValueOnce(json(page))
    vi.stubGlobal('fetch', fetchMock)
    const controller = new AbortController()
    const query = new URLSearchParams({
      provider: 'github', state: 'conflict', search: 'service & tools',
      group_id: '30000000-0000-4000-8000-000000000001', limit: '20', offset: '20',
    })

    expect(await repositorySync.list(query, controller.signal)).toEqual(page)
    const [input, init] = fetchMock.mock.calls[0]
    const url = new URL(String(input), 'https://fixture-forge.invalid')
    expect(url.pathname).toBe('/api/v1/repository-syncs')
    expect(Object.fromEntries(url.searchParams)).toEqual({
      provider: 'github', state: 'conflict', search: 'service & tools',
      group_id: '30000000-0000-4000-8000-000000000001', limit: '20', offset: '20',
    })
    expect(init?.signal).toBe(controller.signal)
    expect(init?.body).toBeUndefined()
    const headers = new Headers(init?.headers)
    expect(headers.get('Authorization')).toBe('Bearer fixture-central-access')
    expect(headers.get('Accept')).toBe('application/json')
    expect(headers.has('Content-Type')).toBe(false)

    fetchMock.mockImplementationOnce((_input, request) => new Promise((_resolve, reject) => {
      request?.signal?.addEventListener('abort', () => reject(new DOMException('fixture abort', 'AbortError')), { once: true })
    }))
    const cancelled = repositorySync.list(query, controller.signal)
    const rejection = expect(cancelled).rejects.toMatchObject({ kind: 'cancelled', status: 0 })
    controller.abort()
    await rejection
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })

  it('keeps repository and run identities in single URL segments and returns the selected history and ref pages', async () => {
    const history: Page<SyncRun> = { items: [run], total: 21 }
    const references: Page<SyncRef> = { items: [{
      ref_name: 'refs/heads/fixture-feature', direction: 'pull', state: 'applied',
      old_sha: null, new_sha: 'a'.repeat(40), event_delivered: true,
    }], total: 51 }
    const fetchMock = vi.fn<Fetch>()
      .mockResolvedValueOnce(json(connection))
      .mockResolvedValueOnce(json(history))
      .mockResolvedValueOnce(json(references))
    vi.stubGlobal('fetch', fetchMock)
    const signal = new AbortController().signal
    const repository = 'fixture/repo?# %'
    const runId = 'fixture/run?# %'

    expect(await repositorySync.get(repository, signal)).toEqual(connection)
    expect(await repositorySync.runs(repository, 20, signal)).toEqual(history)
    expect(await repositorySync.refs(repository, runId, 50, signal)).toEqual(references)
    expect(fetchMock.mock.calls.map(([input]) => input)).toEqual([
      '/api/v1/catalog/repositories/fixture%2Frepo%3F%23%20%25/sync',
      '/api/v1/catalog/repositories/fixture%2Frepo%3F%23%20%25/sync/runs?limit=20&offset=20',
      '/api/v1/catalog/repositories/fixture%2Frepo%3F%23%20%25/sync/runs/fixture%2Frun%3F%23%20%25/refs?limit=50&offset=50',
    ])
    for (const [, init] of fetchMock.mock.calls) {
      expect(init?.signal).toBe(signal)
      expect(init?.body).toBeUndefined()
      expect(new Headers(init?.headers).get('Authorization')).toBe('Bearer fixture-central-access')
    }
  })

  it('sends stable save receipts and distinguishes an omitted credential from a write-only replacement', async () => {
    const saved = { ...connection, version: 8 }
    const rotated = { ...connection, version: 9 }
    const fetchMock = vi.fn<Fetch>()
      .mockResolvedValueOnce(json(saved)).mockResolvedValueOnce(json(saved))
      .mockResolvedValueOnce(json(rotated))
    vi.stubGlobal('fetch', fetchMock)
    const payload: SaveSync = { expected_version: 7, settings }
    const operation = '60000000-0000-4000-8000-000000000001'
    const replacement: SaveSync = { expected_version: 8, settings, token: 'fixture-provider-replacement' }
    const storageBefore = [storedValues(localStorage), storedValues(sessionStorage)]

    expect(await repositorySync.save(connection.repository_id, payload, operation)).toEqual(saved)
    expect(await repositorySync.save(connection.repository_id, payload, operation)).toEqual(saved)
    expect(await repositorySync.save(connection.repository_id, replacement,
      '60000000-0000-4000-8000-000000000002')).toEqual(rotated)

    expect(fetchMock.mock.calls.map(([, init]) => JSON.parse(String(init?.body)))).toEqual([
      { expected_version: 7, settings }, { expected_version: 7, settings },
      { expected_version: 8, settings, token: 'fixture-provider-replacement' },
    ])
    expect(fetchMock.mock.calls.map(([, init]) => new Headers(init?.headers).get('Idempotency-Key'))).toEqual([
      operation, operation, '60000000-0000-4000-8000-000000000002',
    ])
    for (const [input, init] of fetchMock.mock.calls) {
      expect(input).toBe(`/api/v1/catalog/repositories/${connection.repository_id}/sync`)
      expect(init?.method).toBe('PUT')
      const headers = new Headers(init?.headers)
      expect(headers.get('Content-Type')).toBe('application/json')
      expect(headers.get('Authorization')).toBe('Bearer fixture-central-access')
      expect(String(input)).not.toContain('fixture-provider-replacement')
      expect(Array.from(headers.values()).join(' ')).not.toContain('fixture-provider-replacement')
    }
    expect([storedValues(localStorage), storedValues(sessionStorage)]).toEqual(storageBefore)
  })

  it('checks an unsaved draft without saving it and enqueues a manual run with an empty POST body', async () => {
    const checkResult = { read: true, write: 'dry_run_passed', message: 'fixture preliminary write check' }
    const fetchMock = vi.fn<Fetch>()
      .mockResolvedValueOnce(json(checkResult)).mockResolvedValueOnce(json(run, 202))
    vi.stubGlobal('fetch', fetchMock)
    const draft: SyncSettings = { ...settings, provider: 'gitlab', instance_url: 'https://fixture-gitlab.invalid',
      repository_path: 'fixture-team/unsaved', direction: 'push', automatic: false }
    const checkPayload = { settings: draft, token: 'fixture-unsaved-provider' }

    expect(await repositorySync.check(connection.repository_id, checkPayload)).toEqual(checkResult)
    expect(fetchMock).toHaveBeenCalledOnce()
    const [checkUrl, checkInit] = fetchMock.mock.calls[0]
    expect(checkUrl).toBe(`/api/v1/catalog/repositories/${connection.repository_id}/sync/check`)
    expect(checkInit?.method).toBe('POST')
    expect(JSON.parse(String(checkInit?.body))).toEqual(checkPayload)
    expect(new Headers(checkInit?.headers).has('Idempotency-Key')).toBe(false)
    expect(new Headers(checkInit?.headers).get('Content-Type')).toBe('application/json')

    const key = '60000000-0000-4000-8000-000000000003'
    expect(await repositorySync.start(connection.repository_id, key)).toEqual(run)
    expect(fetchMock).toHaveBeenCalledTimes(2)
    const [startUrl, startInit] = fetchMock.mock.calls[1]
    expect(startUrl).toBe(`/api/v1/catalog/repositories/${connection.repository_id}/sync/runs`)
    expect(startInit?.method).toBe('POST')
    expect(startInit?.body).toBeUndefined()
    const startHeaders = new Headers(startInit?.headers)
    expect(startHeaders.get('Idempotency-Key')).toBe(key)
    expect(startHeaders.get('Authorization')).toBe('Bearer fixture-central-access')
    expect(startHeaders.has('Content-Type')).toBe(false)
    expect(storedValues(localStorage)).toEqual([])
    expect(storedValues(sessionStorage)).toEqual([])
    expect(`${checkUrl} ${startUrl}`).not.toContain('fixture-unsaved-provider')
  })

  it('propagates a structured version conflict without silently retrying or changing the save command', async () => {
    const requestId = '70000000-0000-4000-8000-000000000001'
    const details = [{ field: 'expected_version', code: 'stale_version', message: 'fixture saved version is 8' }]
    const fetchMock = vi.fn<Fetch>().mockResolvedValue(json({ error: {
      code: 'sync_version_conflict', message: 'sync_version_conflict', request_id: requestId, details,
    } }, 409))
    vi.stubGlobal('fetch', fetchMock)
    const payload: SaveSync = { expected_version: 7, settings }
    const key = '60000000-0000-4000-8000-000000000004'

    const error: unknown = await repositorySync.save(connection.repository_id, payload, key)
      .catch((failure: unknown) => failure)
    expect(error).toBeInstanceOf(ApiError)
    expect(error).toMatchObject({
      kind: 'api', status: 409, code: 'sync_version_conflict', message: 'sync_version_conflict', requestId, details,
    })
    expect(apiRetry(0, error)).toBe(false)
    expect(fetchMock).toHaveBeenCalledOnce()
    expect(JSON.parse(String(fetchMock.mock.calls[0][1]?.body))).toEqual(payload)
    expect(new Headers(fetchMock.mock.calls[0][1]?.headers).get('Idempotency-Key')).toBe(key)
  })
})
