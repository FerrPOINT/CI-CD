import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'
import { useProjectReport } from '@/api/hooks'
import { Card } from '@sdlc/ui/ui'
import { BarChart3, CheckCircle2, XCircle, Clock, TrendingUp } from 'lucide-react'

export function ReportsPage() {
  const { t } = useTranslation()
  const { projectId } = useParams()
  const { data: report, isLoading } = useProjectReport(projectId)

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-2">
        <BarChart3 className="h-5 w-5 text-accent" />
        <h1 className="text-2xl font-bold">{t('reports.title')}</h1>
      </div>

      {isLoading ? (
        <p className="text-sm text-text-muted">{t('common.loading')}</p>
      ) : report ? (
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          <Card className="p-4">
            <div className="flex items-center gap-2 text-text-muted">
              <TrendingUp className="h-4 w-4" />
              <span className="text-sm">{t('reports.totalPipelines')}</span>
            </div>
            <p className="mt-2 text-2xl font-bold">{report.total_pipelines}</p>
          </Card>
          <Card className="p-4">
            <div className="flex items-center gap-2 text-emerald-500">
              <CheckCircle2 className="h-4 w-4" />
              <span className="text-sm">{t('reports.successful')}</span>
            </div>
            <p className="mt-2 text-2xl font-bold text-emerald-500">{report.successful_pipelines}</p>
          </Card>
          <Card className="p-4">
            <div className="flex items-center gap-2 text-danger">
              <XCircle className="h-4 w-4" />
              <span className="text-sm">{t('reports.failed')}</span>
            </div>
            <p className="mt-2 text-2xl font-bold text-danger">{report.failed_pipelines}</p>
          </Card>
          <Card className="p-4">
            <div className="flex items-center gap-2 text-text-muted">
              <CheckCircle2 className="h-4 w-4" />
              <span className="text-sm">{t('reports.successRate')}</span>
            </div>
            <p className="mt-2 text-2xl font-bold">{(report.success_rate * 100).toFixed(1)}%</p>
          </Card>
          <Card className="p-4">
            <div className="flex items-center gap-2 text-text-muted">
              <Clock className="h-4 w-4" />
              <span className="text-sm">{t('reports.avgDuration')}</span>
            </div>
            <p className="mt-2 text-2xl font-bold">{formatDuration(report.average_duration_seconds)}</p>
          </Card>
        </div>
      ) : (
        <Card className="p-8 text-center"><p className="text-text-muted">{t('reports.noData')}</p></Card>
      )}
    </div>
  )
}

function formatDuration(seconds: number): string {
  if (seconds < 1) return '—'
  if (seconds < 60) return `${seconds.toFixed(1)}s`
  if (seconds < 3600) return `${(seconds / 60).toFixed(1)}m`
  return `${(seconds / 3600).toFixed(1)}h`
}