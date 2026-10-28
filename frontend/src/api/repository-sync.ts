import { api } from './client'

import type { components } from './schema'
export type SyncDirection = components['schemas']['RepositorySyncDirection']
export type SyncSettings = components['schemas']['RepositorySyncSettings']
export type SyncRun = components['schemas']['RepositorySyncSyncRun']
export type SyncConnection = components['schemas']['RepositorySyncConnection']
export type SyncRef = components['schemas']['RepositorySyncRefResult']
export interface Page<T> {
  items: T[]
  total: number
}
export type SaveSync = components['schemas']['RepositorySyncSaveConnection']
const path = (id: string) =>
  `/catalog/repositories/${encodeURIComponent(id)}/sync`
const selected = (url: string, connection?: string) =>
  connection !== undefined ? `${url}${url.includes('?') ? '&' : '?'}connection_id=${encodeURIComponent(connection)}` : url
export const repositorySync = {
  list: (query: URLSearchParams, signal?: AbortSignal) =>
    api<Page<SyncConnection>>(`/repository-syncs?${query}`, { signal }),
  get: (id: string, signal?: AbortSignal, connection?: string) =>
    api<SyncConnection>(selected(path(id), connection), { signal }),
  save: (id: string, payload: SaveSync, key: string, connection?: string) =>
    api<SyncConnection>(selected(path(id), connection), {
      method: 'PUT',
      headers: { 'Idempotency-Key': key },
      body: JSON.stringify(payload),
    }),
  check: (id: string, payload: Omit<SaveSync, 'expected_version'>, connection?: string) =>
    api<components['schemas']['RepositorySyncCheckResult']>(
      selected(`${path(id)}/check`, connection),
      { method: 'POST', body: JSON.stringify(payload) },
    ),
  start: (id: string, key: string, connection?: string) =>
    api<SyncRun>(selected(`${path(id)}/runs`, connection), {
      method: 'POST',
      headers: { 'Idempotency-Key': key },
    }),
  runs: (id: string, offset: number, signal?: AbortSignal, connection?: string) =>
    api<Page<SyncRun>>(selected(`${path(id)}/runs?limit=20&offset=${offset}`, connection), {
      signal,
    }),
  refs: (id: string, run: string, offset: number, signal?: AbortSignal, connection?: string) =>
    api<Page<SyncRef>>(
      selected(`${path(id)}/runs/${encodeURIComponent(run)}/refs?limit=50&offset=${offset}`, connection),
      { signal },
    ),
}
