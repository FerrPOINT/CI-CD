import { Link } from 'react-router'
import { useTranslation } from 'react-i18next'
import { useProjects, useRunners } from '@/api/hooks'
import { useProjectPipelines } from '@/shared/lib/use-project-pipelines'
import { formatDate } from '@/shared/lib/format'
import { QueryState } from '@/shared/ui/query-state'
import { Button } from '@sdlc/ui/ui'
import { ArrowRight, Clock3, FolderGit2, Play, Server } from 'lucide-react'
import { summarizePipelines } from './model'

function RetryNotice({ message, onRetry }: { message: string; onRetry: () => void }) {
  const { t } = useTranslation()

  return (
    <div role="alert" className="flex flex-wrap items-center gap-3 border-l-2 border-danger py-2 pl-3 text-sm text-text-secondary">
      <span className="min-w-0 flex-1">{message}</span>
      <Button type="button" size="sm" variant="outline" className="min-h-10 sm:min-h-10" onClick={onRetry}>
        {t('common.retry')}
      </Button>
    </div>
  )
}

export function DashboardPage() {
  const { t, i18n } = useTranslation()
  const {
    data: projectsData,
    isLoading: projectsLoading,
    error: projectsError,
    refetch: refetchProjects,
  } = useProjects()
  const {
    data: runnersData,
    isLoading: runnersLoading,
    error: runnersError,
    refetch: refetchRunners,
  } = useRunners()
  const projects = projectsData ?? []
  const runners = runnersData ?? []
  const hasProjectData = projectsData !== undefined
  const hasRunnerData = runnersData !== undefined
  const pipelineQuery = useProjectPipelines(projects)
  const summary = summarizePipelines(pipelineQuery.runs)
  const projectNames = new Map(projects.map(project => [project.id, project.name]))
  const unavailableRunners = runners.filter(runner => runner.status !== 'online').length
  const runsReady = !projectsLoading
    && hasProjectData
    && !pipelineQuery.isLoading
    && (projects.length === 0 || pipelineQuery.hasData)

  const stats = [
    { label: t('dashboard.projects'), value: projectsLoading && !hasProjectData ? '…' : hasProjectData ? projects.length : '—', tone: 'text-text-primary' },
    { label: t('dashboard.queued'), value: runsReady ? summary.queued : pipelineQuery.isLoading ? '…' : '—', tone: 'text-text-primary' },
    { label: t('dashboard.running'), value: runsReady ? summary.running : pipelineQuery.isLoading ? '…' : '—', tone: 'text-warning' },
    { label: t('dashboard.failed'), value: runsReady ? summary.failed : pipelineQuery.isLoading ? '…' : '—', tone: 'text-danger' },
  ]

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-xl font-bold sm:text-2xl">{t('navigation.dashboard')}</h1>
        <Button asChild size="sm" className="min-h-10 sm:min-h-10">
          <Link to="/projects"><FolderGit2 className="h-4 w-4" />{t('navigation.projects')}</Link>
        </Button>
      </div>

      <div className="grid grid-cols-2 border-y border-border sm:grid-cols-4" aria-label={t('dashboard.summary')}>
        {stats.map((stat, index) => (
          <div key={stat.label} className={`min-w-0 px-3 py-3 first:pl-0 sm:px-4 ${index > 0 ? 'border-l border-border' : ''} ${index === 2 ? 'border-l-0 sm:border-l' : ''} ${index >= 2 ? 'border-t border-border sm:border-t-0' : ''}`}>
            <p className="text-xs text-text-muted">{stat.label}</p>
            <p className={`mt-1 text-xl font-semibold tabular-nums ${stat.tone}`}>{stat.value}</p>
          </div>
        ))}
      </div>

      {projectsError && hasProjectData && (
        <RetryNotice
          message={t('dashboard.projectsRefreshError')}
          onRetry={() => { void refetchProjects() }}
        />
      )}

      <div className="grid gap-6 xl:grid-cols-[minmax(0,1.7fr)_minmax(18rem,1fr)]">
        <section className="min-w-0" aria-labelledby="recent-runs-heading">
          <div className="mb-2 flex items-center justify-between gap-3">
            <h2 id="recent-runs-heading" className="text-base font-semibold">{t('dashboard.recentRuns')}</h2>
            {projects.length > 0 && <span className="text-xs text-text-muted">{t('dashboard.latestEight')}</span>}
          </div>
          <QueryState
            data={projectsData}
            isLoading={projectsLoading}
            error={hasProjectData ? null : projectsError}
            errorMessage={t('dashboard.projectsError')}
            onRetry={() => { void refetchProjects() }}
            isEmpty={list => list.length === 0}
            empty={{ title: t('dashboard.noProjects'), description: t('dashboard.noProjectsHint') }}
          >
            {() => pipelineQuery.isLoading ? (
              <div role="status" className="py-5 text-sm text-text-muted">{t('common.loading')}</div>
            ) : (
              <div className="space-y-3">
                {pipelineQuery.error && (
                  <RetryNotice
                    message={pipelineQuery.hasData
                      ? t('dashboard.runsPartialError', { count: pipelineQuery.failedCount })
                      : t('dashboard.runsError')}
                    onRetry={() => { void pipelineQuery.refetch() }}
                  />
                )}
                {pipelineQuery.error && !pipelineQuery.hasData ? null : summary.recent.length === 0 ? (
                  <div className="border-y border-border py-5 text-sm text-text-muted">{t('dashboard.noRuns')}</div>
                ) : (
                  <ul className="divide-y divide-border border-y border-border">
                    {summary.recent.map(run => (
                      <li key={run.id}>
                        <Link to={`/pipelines/${run.id}`} className="flex min-h-14 items-center gap-3 py-2 transition-colors hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-accent">
                          <span aria-hidden className={`h-2 w-2 shrink-0 rounded-full ${run.status === 'success' ? 'bg-success' : run.status === 'failed' ? 'bg-danger' : run.status === 'running' ? 'bg-warning' : 'bg-text-muted'}`} />
                          <span className="min-w-0 flex-1">
                            <span className="block truncate text-sm font-medium">{projectNames.get(run.project_id) ?? run.project_id}</span>
                            <span className="block truncate text-xs text-text-muted">{run.git_ref}</span>
                          </span>
                          <span className="hidden shrink-0 text-xs text-text-muted sm:block">{formatDate(run.created_at, i18n.language)}</span>
                          <span className="w-24 shrink-0 text-right text-xs">{t(`pipelines.${run.status}`)}</span>
                          <ArrowRight className="h-4 w-4 shrink-0 text-text-muted" aria-hidden />
                        </Link>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )}
          </QueryState>
          {projects.length > 0 && <p className="mt-2 text-xs text-text-muted">{t('dashboard.runsScope')}</p>}
        </section>

        <div className="space-y-5">
          {(projects.length > 0 || projectsLoading) && <section aria-labelledby="projects-heading">
            <div className="mb-2 flex items-center justify-between gap-3">
              <h2 id="projects-heading" className="text-base font-semibold">{t('navigation.projects')}</h2>
              <Link to="/projects" className="inline-flex min-h-10 items-center gap-1 text-sm text-accent hover:underline">{t('dashboard.allProjects')} <ArrowRight className="h-4 w-4" /></Link>
            </div>
            <QueryState data={projectsData} isLoading={projectsLoading} error={null} isEmpty={list => list.length === 0} empty={{ title: t('projects.empty') }}>
              {() => <ul className="divide-y divide-border border-y border-border">
                {projects.slice(0, 6).map(project => (
                  <li key={project.id}>
                    <Link to={`/projects/${project.id}/pipelines`} className="flex min-h-12 items-center gap-2 py-2 text-sm hover:text-accent focus-visible:outline-2 focus-visible:outline-accent">
                      <FolderGit2 className="h-4 w-4 shrink-0 text-text-muted" aria-hidden />
                      <span className="min-w-0 flex-1 truncate font-medium">{project.name}</span>
                      <span className="max-w-24 truncate text-xs text-text-muted">{project.default_branch}</span>
                      <ArrowRight className="h-4 w-4 shrink-0 text-text-muted" aria-hidden />
                    </Link>
                  </li>
                ))}
              </ul>}
            </QueryState>
          </section>}

          <section aria-labelledby="runners-heading">
            <div className="mb-2 flex items-center justify-between gap-3">
              <h2 id="runners-heading" className="text-base font-semibold">{t('navigation.runners')}</h2>
              <Link to="/runners" className="inline-flex min-h-10 items-center gap-1 text-sm text-accent hover:underline">{t('dashboard.manageRunners')} <ArrowRight className="h-4 w-4" /></Link>
            </div>
            <div className="space-y-2">
              <div className="flex items-center gap-3 border-y border-border py-3 text-sm">
                <Server className="h-4 w-4 shrink-0 text-text-muted" aria-hidden />
                {runnersLoading && !hasRunnerData ? <span>{t('common.loading')}</span> : runnersError && !hasRunnerData ? (
                  <RetryNotice message={t('dashboard.runnersError')} onRetry={() => { void refetchRunners() }} />
                ) : runners.length === 0 ? <span className="text-text-muted">{t('dashboard.noRunners')}</span> : (
                  <span>{t('dashboard.runnerHealth', { online: runners.length - unavailableRunners, total: runners.length })}{unavailableRunners > 0 && <span className="ml-2 text-warning">{t('dashboard.unavailableRunners', { count: unavailableRunners })}</span>}</span>
                )}
              </div>
              {runnersError && hasRunnerData && (
                <RetryNotice message={t('dashboard.runnersRefreshError')} onRetry={() => { void refetchRunners() }} />
              )}
            </div>
          </section>

          <div className="flex flex-wrap gap-x-4 gap-y-1 text-sm">
            <Link to="/repositories" className="inline-flex min-h-10 items-center gap-1 text-accent hover:underline"><FolderGit2 className="h-4 w-4" />{t('navigation.repositories')}</Link>
            <Link to="/projects" className="inline-flex min-h-10 items-center gap-1 text-accent hover:underline"><Play className="h-4 w-4" />{t('dashboard.chooseProject')}</Link>
            <Link to="/audit-log" className="inline-flex min-h-10 items-center gap-1 text-accent hover:underline"><Clock3 className="h-4 w-4" />{t('navigation.auditLog')}</Link>
          </div>
        </div>
      </div>
    </div>
  )
}
