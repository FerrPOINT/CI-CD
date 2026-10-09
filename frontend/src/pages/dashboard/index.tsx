import { useQuery } from '@tanstack/react-query'
import { api } from '@/api/client'
import type { components } from '@/api/schema'
import { ExecutionList } from '@/widgets/execution-list'
import { NamespaceLink as Link } from '@sdlc/ui/ui'
import type { Runner } from '@/api/types'
export function DashboardPage() {
  const runners = useQuery({
    queryKey: ['runners'],
    queryFn: ({ signal }) => api<Runner[]>('/runners', { signal }),
  })
  const summary = useQuery({
    queryKey: ['workspace-summary', 'all'],
    queryFn: ({ signal }) =>
      api<components['schemas']['WorkspaceSummary']>('/workspace-summary', { signal }),
  })
  return (
    <div className="space-y-5">
      <h1 className="text-2xl font-semibold">CI/CD</h1>
      {summary.isPending ? (
        <p role="status">Загружаем сводку…</p>
      ) : summary.isError ? (
        <p role="alert" className="text-danger">
          Сводка недоступна.
        </p>
      ) : (
        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
          {[
            ['Проекты', summary.data.projects],
            ['Репозитории', summary.data.repositories],
            ['CI-конфигурации', summary.data.configurations],
            ['В очереди', summary.data.queued],
            ['Выполняются', summary.data.running],
            ['С ошибкой', summary.data.failed],
          ].map(([label, count]) => (
            <div key={label} className="rounded-lg border border-border bg-surface p-4">
              <p className="text-sm text-text-muted">{label}</p>
              <p className="mt-2 text-2xl font-semibold">{count}</p>
            </div>
          ))}
        </div>
      )}
      <section className="rounded-lg border border-border bg-surface p-4">
        <Link to="/runners" className="font-semibold text-accent">
          Исполнители CI
        </Link>
        {runners.isError ? (
          <p className="text-danger">Состояние исполнителей недоступно.</p>
        ) : runners.isPending ? (
          <p role="status">Загружаем исполнителей…</p>
        ) : (
          <p className="text-sm text-text-muted">
            Онлайн: {runners.data.filter((runner) => runner.status === 'online').length} /{' '}
            {runners.data.length}
          </p>
        )}
      </section>
      <h2 className="font-semibold">Пайплайны проектов</h2>
      <ExecutionList />
    </div>
  )
}
