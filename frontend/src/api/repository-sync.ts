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
export const repositorySync = {
  list: (query: URLSearchParams, signal?: AbortSignal) =>
    api<Page<SyncConnection>>(`/repository-syncs?${query}`, { signal }),
  get: (id: string, signal?: AbortSignal) =>
    api<SyncConnection>(path(id), { signal }),
  save: (id: string, payload: SaveSync, key: string) =>
    api<SyncConnection>(path(id), {
      method: 'PUT',
      headers: { 'Idempotency-Key': key },
      body: JSON.stringify(payload),
    }),
  check: (id: string, payload: Omit<SaveSync, 'expected_version'>) =>
    api<components['schemas']['RepositorySyncCheckResult']>(
      `${path(id)}/check`,
      { method: 'POST', body: JSON.stringify(payload) },
    ),
  start: (id: string, key: string) =>
    api<SyncRun>(`${path(id)}/runs`, {
      method: 'POST',
      headers: { 'Idempotency-Key': key },
    }),
  runs: (id: string, offset: number, signal?: AbortSignal) =>
    api<Page<SyncRun>>(`${path(id)}/runs?limit=20&offset=${offset}`, {
      signal,
    }),
  refs: (id: string, run: string, offset: number, signal?: AbortSignal) =>
    api<Page<SyncRef>>(
      `${path(id)}/runs/${encodeURIComponent(run)}/refs?limit=50&offset=${offset}`,
      { signal },
    ),
}
