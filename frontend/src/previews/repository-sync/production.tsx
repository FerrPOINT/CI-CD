// Isolated visual QA entrypoint. Fixtures and transport replacement never enter production.
import { useEffect, useState } from 'react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { RepositorySyncEditor } from '@/pages/repository-syncs'
import { repositorySync, type SyncConnection } from '@/api/repository-sync'
const cache = new QueryClient()
const initial: SyncConnection = {
  repository_id: 'visual-qa',
  repository_name: 'platform/service-api',
  namespace: null,
  version: 1,
  settings: {
    provider: 'github',
    instance_url: 'https://github.com',
    repository_path: 'example/service-api',
    direction: 'both',
    automatic: true,
  },
  has_credential: true,
  writable: true,
  paused_reason: null,
  worker_enabled: true,
  next_check_at: null,
  active_run: null,
  last_run: null,
}
let savedConnection = initial
let publish: ((connection: SyncConnection) => void) | undefined
repositorySync.save = async (_id, payload) => {
  savedConnection = {
    ...savedConnection,
    settings: payload.settings,
    version: savedConnection.version + 1,
  }
  publish?.(savedConnection)
  return savedConnection
}
repositorySync.get = async () => savedConnection
repositorySync.check = async () => ({
  read: true,
  write: 'preliminary',
  message: 'Предпросмотр: доступ не проверялся.',
})
repositorySync.start = async () => {
  throw new Error('Предпросмотр: реальный запуск недоступен.')
}
repositorySync.runs = async () => ({ items: [], total: 0 })
repositorySync.refs = async () => ({ items: [], total: 0 })
export function ProductionSyncPreview() {
  const [connection, setConnection] = useState(savedConnection)
  useEffect(() => {
    publish = setConnection
    return () => {
      publish = undefined
    }
  }, [])
  return (
    <QueryClientProvider client={cache}>
      <div className="border-b border-border px-4 py-3 text-sm">
        Предпросмотр production-компонента · вымышленные данные и токены
      </div>
      <main className="mx-auto max-w-6xl p-4 sm:p-8">
        <RepositorySyncEditor
          backLocation="/sync-preview.html"
          connection={connection}
          reload={async () => undefined}
        />
      </main>
    </QueryClientProvider>
  )
}
