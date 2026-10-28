// Source-only component fixture, excluded from the production index entrypoint.
import { createRoot } from 'react-dom/client'
import { BrowserRouter } from 'react-router'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { ThemeProvider } from '@pdlc/ui/lib'
import { RepositorySyncEditor } from '../../src/pages/repository-syncs'
import type { SyncConnection } from '../../src/api/repository-sync'
import '../../src/index.css'

const connection: SyncConnection = {
  id: 'repo',
  repository_id: '00000000-0000-4000-8000-000000000001',
  repository_name: 'fixture/service', namespace: null, version: 1,
  settings: { provider: 'github', instance_url: 'https://github.com',
    repository_path: 'fixture/service', direction: 'disabled', automatic: false },
  has_credential: true, writable: true, paused_reason: null, worker_enabled: false,
  next_check_at: null, active_run: null, last_run: null,
}
const cache = new QueryClient({ defaultOptions: { queries: { retry: false } } })
createRoot(document.getElementById('root')!).render(
  <ThemeProvider theme="dark">
    <QueryClientProvider client={cache}>
      <BrowserRouter>
        <main className="mx-auto max-w-6xl p-4">
          <RepositorySyncEditor connection={connection} reload={async () => undefined} />
        </main>
      </BrowserRouter>
    </QueryClientProvider>
  </ThemeProvider>,
)
