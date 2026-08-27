import { Link } from 'react-router'
import { useTranslation } from 'react-i18next'
import { useProjects } from '@/api/hooks'
import { useProjectPipelines } from '@/shared/lib/use-project-pipelines'
import { Card } from '@/shared/ui/card'
import { Button } from '@/shared/ui/button'
import { FolderGit2, Play, CheckCircle2, XCircle, Clock } from 'lucide-react'

export function DashboardPage() {
  const { t } = useTranslation()
  const { data: projects = [], isLoading } = useProjects()

  // Aggregate CI metrics from recent pipelines across all visible projects.
  // Page is a summary, not an exhaustive report (see /reports for per-project stats).
  const pipelineLists = useProjectPipelines(projects)

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">{t('navigation.dashboard')}</h1>
        <Button asChild size="sm">
          <Link to="/projects">
            <FolderGit2 className="h-4 w-4" />
            {t('navigation.projects')}
          </Link>
        </Button>
      </div>

      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <Card className="p-4">
          <div className="flex items-center gap-2 text-text-muted"><FolderGit2 className="h-4 w-4" /><span className="text-sm">{t('dashboard.projects')}</span></div>
          <p className="mt-2 text-2xl font-bold">{projects.length}</p>
        </Card>
        <Card className="p-4">
          <div className="flex items-center gap-2 text-text-muted"><Play className="h-4 w-4" /><span className="text-sm">{t('dashboard.totalRuns')}</span></div>
          <p className="mt-2 text-2xl font-bold">{pipelineLists.flat().length}</p>
        </Card>
        <Card className="p-4">
          <div className="flex items-center gap-2 text-success"><CheckCircle2 className="h-4 w-4" /><span className="text-sm">{t('dashboard.success')}</span></div>
          <p className="mt-2 text-2xl font-bold">{pipelineLists.flat().filter(p => p.status === 'success').length}</p>
        </Card>
        <Card className="p-4">
          <div className="flex items-center gap-2 text-danger"><XCircle className="h-4 w-4" /><span className="text-sm">{t('dashboard.failed')}</span></div>
          <p className="mt-2 text-2xl font-bold">{pipelineLists.flat().filter(p => p.status === 'failed').length}</p>
        </Card>
      </div>

      <div>
        <h2 className="mb-3 text-lg font-semibold">{t('navigation.projects')}</h2>
        {isLoading ? (
          <p className="text-sm text-text-muted">{t('common.loading')}</p>
        ) : projects.length === 0 ? (
          <Card className="p-8 text-center">
            <p className="text-text-muted">{t('projects.empty')}</p>
          </Card>
        ) : (
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {projects.map((p) => (
              <Link key={p.id} to={`/projects/${p.id}/pipelines`}>
                <Card className="cursor-pointer p-4 transition-colors hover:border-accent">
                  <div className="flex items-center gap-2">
                    <FolderGit2 className="h-4 w-4 text-accent" />
                    <span className="font-medium">{p.name}</span>
                  </div>
                  <p className="mt-2 truncate text-xs text-text-muted">{p.repository_url}</p>
                  <div className="mt-2 flex items-center gap-2 text-xs text-text-muted">
                    <Clock className="h-3 w-3" />
                    <code>{p.default_branch}</code>
                  </div>
                </Card>
              </Link>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
