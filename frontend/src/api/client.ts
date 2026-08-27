import { currentSession } from './auth'

const BASE = '/api/v1'

export class ApiError extends Error {
  readonly status: number

  constructor(message: string, status: number) {
    super(message)
    this.name = 'ApiError'
    this.status = status
  }
}

export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  let response = await fetch(`${BASE}${path}`, withAuth(init))
  if (response.status === 401) {
    // One transparent refresh + retry before surfacing the 401.
    const refreshed = await import('./auth').then((m) => m.refresh())
    if (refreshed) response = await fetch(`${BASE}${path}`, withAuth(init))
  }
  if (!response.ok) {
    const body = await response.json().catch(() => ({ error: response.statusText }))
    throw new ApiError(body.error || response.statusText, response.status)
  }
  return response.json() as Promise<T>
}

function withAuth(init?: RequestInit): RequestInit {
  const session = currentSession()
  const headers = new Headers(init?.headers)
  headers.set('Content-Type', 'application/json')
  if (session && session.expires_at * 1000 > Date.now() + 30_000) {
    headers.set('Authorization', `Bearer ${session.access_token}`)
  }
  return { ...init, headers }
}

/** Retry transport/5xx failures but surface client errors (4xx) immediately. */
export function apiRetry(failureCount: number, error: unknown): boolean {
  if (error instanceof ApiError && error.status >= 400 && error.status < 500) return false
  return failureCount < 2
}
