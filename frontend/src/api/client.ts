const BASE = '/api/v1'

export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${BASE}${path}`, {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
  if (!response.ok) {
    const body = await response.json().catch(() => ({ error: response.statusText }))
    throw new Error(body.error || response.statusText)
  }
  return response.json() as Promise<T>
}
