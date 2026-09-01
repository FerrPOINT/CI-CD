import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const authMocks = vi.hoisted(() => ({
  currentSession: vi.fn(),
  refresh: vi.fn(),
}))

vi.mock('./auth', () => authMocks)

import { api, ApiError, apiRetry } from './client'

beforeEach(() => {
  authMocks.currentSession.mockReset()
  authMocks.refresh.mockReset()
  authMocks.currentSession.mockReturnValue(null)
  authMocks.refresh.mockResolvedValue(null)
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('api client errors', () => {
  it('[REQ-UI-001] preserves structured API error fields from the envelope', async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({
      error: {
        code: 'permission_denied',
        message: 'forbidden',
        request_id: '11111111-1111-4111-8111-111111111111',
        details: [
          { field: 'role', code: 'requires_admin', message: 'admin role required' },
          { code: 404, message: 'ignored invalid detail' },
        ],
      },
    }), {
      status: 403,
      headers: {
        'Content-Type': 'application/json',
        'Retry-After': '12',
      },
    }))
    vi.stubGlobal('fetch', fetchMock)

    const error = await captureApiError(() => api('/users'))

    expect(error.kind).toBe('api')
    expect(error.status).toBe(403)
    expect(error.code).toBe('permission_denied')
    expect(error.message).toBe('forbidden')
    expect(error.requestId).toBe('11111111-1111-4111-8111-111111111111')
    expect(error.retryAfterSeconds).toBe(12)
    expect(error.details).toEqual([
      { field: 'role', code: 'requires_admin', message: 'admin role required' },
    ])
    expect(apiRetry(0, error)).toBe(false)
    expect((fetchMock.mock.calls[0][1]?.headers as Headers).get('Accept')).toBe('application/json')
    expect((fetchMock.mock.calls[0][1]?.headers as Headers).has('Content-Type')).toBe(false)
  })

  it('[REQ-UI-001] uses a safe fallback for non-JSON HTTP errors', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('html error', {
      status: 503,
      statusText: 'Service Unavailable',
      headers: { 'x-request-id': '22222222-2222-4222-8222-222222222222' },
    })))

    const error = await captureApiError(() => api('/projects'))

    expect(error.kind).toBe('api')
    expect(error.status).toBe(503)
    expect(error.message).toBe('Service Unavailable')
    expect(error.requestId).toBe('22222222-2222-4222-8222-222222222222')
    expect(apiRetry(0, error)).toBe(true)
    expect(apiRetry(2, error)).toBe(false)
  })

  it('[REQ-UI-001] maps network failures separately from server responses', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new TypeError('Failed to fetch')))

    const error = await captureApiError(() => api('/projects'))

    expect(error.kind).toBe('network')
    expect(error.status).toBe(0)
    expect(error.message).toBe('Failed to fetch')
    expect(apiRetry(1, error)).toBe(true)
  })

  it('[REQ-UI-001] maps aborted requests as non-retryable cancellation', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new DOMException('stop', 'AbortError')))

    const error = await captureApiError(() => api('/projects'))

    expect(error.kind).toBe('cancelled')
    expect(error.status).toBe(0)
    expect(apiRetry(0, error)).toBe(false)
  })
})

async function captureApiError(action: () => Promise<unknown>): Promise<ApiError> {
  try {
    await action()
  } catch (error) {
    expect(error).toBeInstanceOf(ApiError)
    return error as ApiError
  }
  throw new Error('expected ApiError')
}
