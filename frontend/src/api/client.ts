import { currentSession } from './auth'

const BASE = '/api/v1'

export type ApiErrorKind = 'api' | 'network' | 'cancelled'

export interface ApiErrorDetail {
  field?: string
  code: string
  message: string
}

export class ApiError extends Error {
  readonly kind: ApiErrorKind
  readonly status: number
  readonly code?: string
  readonly requestId?: string
  readonly details?: ApiErrorDetail[]
  readonly retryAfterSeconds?: number

  constructor(input: {
    kind: ApiErrorKind
    message: string
    status?: number
    code?: string
    requestId?: string
    details?: ApiErrorDetail[]
    retryAfterSeconds?: number
  }) {
    const status = input.status ?? 0
    const message = input.message
    super(message)
    this.name = 'ApiError'
    this.kind = input.kind
    this.status = status
    this.code = input.code
    this.requestId = input.requestId
    this.details = input.details
    this.retryAfterSeconds = input.retryAfterSeconds
  }
}

export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  let response = await request(path, init)
  if (response.status === 401) {
    // One transparent refresh + retry before surfacing the 401.
    const refreshed = await import('./auth').then((m) => m.refresh()).catch(() => null)
    if (refreshed) response = await request(path, init)
  }
  if (!response.ok) {
    throw await apiErrorFromResponse(response)
  }
  return response.json() as Promise<T>
}

async function request(path: string, init?: RequestInit): Promise<Response> {
  try {
    return await fetch(`${BASE}${path}`, withAuth(init))
  } catch (error) {
    throw apiErrorFromFetch(error)
  }
}

function withAuth(init?: RequestInit): RequestInit {
  const session = currentSession()
  const headers = new Headers(init?.headers)
  headers.set('Accept', 'application/json')
  if (init?.body !== undefined && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json')
  }
  if (session && session.expires_at * 1000 > Date.now() + 30_000) {
    headers.set('Authorization', `Bearer ${session.access_token}`)
  }
  return { ...init, headers }
}

/** Retry transport/5xx failures but surface client errors (4xx) immediately. */
export function apiRetry(failureCount: number, error: unknown): boolean {
  if (error instanceof ApiError && error.kind === 'cancelled') return false
  if (error instanceof ApiError && error.status >= 400 && error.status < 500) return false
  return failureCount < 2
}

async function apiErrorFromResponse(response: Response): Promise<ApiError> {
  const retryAfterSeconds = parseRetryAfter(response.headers.get('Retry-After'))
  const requestId = response.headers.get('x-request-id') ?? undefined
  const body = await response.json().catch(() => null)
  const envelope = parseErrorEnvelope(body)
  return new ApiError({
    kind: 'api',
    status: response.status,
    code: envelope?.code,
    message: envelope?.message ?? (response.statusText || `HTTP ${response.status}`),
    requestId: envelope?.requestId ?? requestId,
    details: envelope?.details,
    retryAfterSeconds,
  })
}

function apiErrorFromFetch(error: unknown): ApiError {
  if (error instanceof DOMException && error.name === 'AbortError') {
    return new ApiError({ kind: 'cancelled', message: 'request cancelled' })
  }
  return new ApiError({
    kind: 'network',
    message: error instanceof Error && error.message ? error.message : 'network request failed',
  })
}

function parseRetryAfter(value: string | null): number | undefined {
  if (!value) return undefined
  const seconds = Number(value)
  if (Number.isFinite(seconds) && seconds >= 0) return Math.ceil(seconds)
  const date = Date.parse(value)
  if (Number.isNaN(date)) return undefined
  return Math.max(0, Math.ceil((date - Date.now()) / 1000))
}

function parseErrorEnvelope(body: unknown): {
  code?: string
  message?: string
  requestId?: string
  details?: ApiErrorDetail[]
} | undefined {
  if (!isRecord(body) || !isRecord(body.error)) return undefined
  const error = body.error
  return {
    code: typeof error.code === 'string' ? error.code : undefined,
    message: typeof error.message === 'string' ? error.message : undefined,
    requestId: typeof error.request_id === 'string' ? error.request_id : undefined,
    details: parseDetails(error.details),
  }
}

function parseDetails(value: unknown): ApiErrorDetail[] | undefined {
  if (!Array.isArray(value)) return undefined
  const details = value.flatMap((item): ApiErrorDetail[] => {
    if (!isRecord(item) || typeof item.code !== 'string' || typeof item.message !== 'string') {
      return []
    }
    return [{
      code: item.code,
      message: item.message,
      field: typeof item.field === 'string' ? item.field : undefined,
    }]
  })
  return details.length > 0 ? details : undefined
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}
