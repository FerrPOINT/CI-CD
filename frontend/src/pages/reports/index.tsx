import { Link, useParams } from 'react-router'
import { useTranslation } from 'react-i18next'
import { useProjectReport } from '@/api/hooks'
import { QueryState } from '@/shared/ui/query-state'
import { ArrowRight, BarChart3 } from 'lucide-react'
import type { ProjectReport } from '@/api/types'

export function ReportsPage() {
  const { t, i18n } = useTranslation()
  const { projectId } = useParams()
  const reportQuery = useProjectReport(projectId)
  const pipelinesPath = `/projects/${projectId}/pipelines`

  return (
    <div className="max-w-5xl space-y-5">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <BarChart3 className="h-5 w-5 text-accent" aria-hidden />
          <h1 className="text-xl font-bold sm:text-2xl">{t('reports.title')}</h1>
        </div>
        {reportQuery.data?.total_pipelines !== 0 && <Link to={pipelinesPath} className="inline-flex min-h-10 items-center gap-2 text-sm text-accent hover:underline focus-visible:outline-2 focus-visible:outline-accent">
          {t('reports.openPipelines')} <ArrowRight className="h-4 w-4" aria-hidden />
        </Link>}
      </header>

      <QueryState
        data={reportQuery.data}
        isLoading={reportQuery.isLoading}
        error={reportQuery.error}
        errorMessage={t('reports.loadFailed')}
        onRetry={() => void reportQuery.refetch()}
      >
        {report => report.total_pipelines === 0
          ? <div className="border-y border-border py-6">
              <p className="text-sm font-medium">{t('reports.noData')}</p>
              <p className="mt-1 text-sm text-text-secondary">{t('reports.emptyHint')}</p>
              <Link to={pipelinesPath} className="mt-3 inline-flex min-h-10 items-center gap-2 text-sm text-accent hover:underline focus-visible:outline-2 focus-visible:outline-accent">
                {t('reports.openPipelines')} <ArrowRight className="h-4 w-4" aria-hidden />
              </Link>
            </div>
          : <ReportSummary report={report} locale={i18n.language} />}
      </QueryState>
    </div>
  )
}

function ReportSummary({ report, locale }: { report: ProjectReport; locale: string }) {
  const { t } = useTranslation()
  const other = Math.max(0, report.total_pipelines - report.successful_pipelines - report.failed_pipelines)
  const percent = new Intl.NumberFormat(locale, { style: 'percent', minimumFractionDigits: 1, maximumFractionDigits: 1 })
  const number = new Intl.NumberFormat(locale, { maximumFractionDigits: 1 })
  const share = (count: number) => `${Math.min(100, Math.max(0, count / report.total_pipelines * 100))}%`

  const counts = [
    { label: t('reports.totalPipelines'), value: report.total_pipelines, color: null },
    { label: t('reports.successful'), value: report.successful_pipelines, color: 'bg-success' },
    { label: t('reports.failed'), value: report.failed_pipelines, color: 'bg-danger' },
    { label: t('reports.other'), value: other, color: 'bg-text-muted' },
  ]

  return (
    <div className="space-y-5">
      <p className="text-xs text-text-secondary">{t('reports.allTime')}</p>
      <div className="grid gap-5 sm:grid-cols-2">
        <section aria-labelledby="success-rate-heading">
          <h2 id="success-rate-heading" className="text-sm text-text-secondary">{t('reports.successRate')}</h2>
          <p className="mt-1 text-3xl font-semibold tabular-nums">{percent.format(report.success_rate)}</p>
          <p className="mt-1 text-xs text-text-secondary">{t('reports.rateScope')}</p>
        </section>
        <section aria-labelledby="duration-heading">
          <h2 id="duration-heading" className="text-sm text-text-secondary">{t('reports.avgDuration')}</h2>
          <p className="mt-1 text-3xl font-semibold tabular-nums">{formatDuration(report.average_duration_seconds, number, t)}</p>
          <p className="mt-1 text-xs text-text-secondary">{t('reports.durationScope')}</p>
        </section>
      </div>

      <div className="space-y-2">
        <p className="text-xs text-text-secondary">{t('reports.statusDistribution')}</p>
        <div className="flex h-2 overflow-hidden rounded-sm bg-surface-raised" role="img" aria-label={t('reports.distribution', { successful: report.successful_pipelines, failed: report.failed_pipelines, other })}>
          <span className="bg-success" style={{ width: share(report.successful_pipelines) }} />
          <span className="bg-danger" style={{ width: share(report.failed_pipelines) }} />
          <span className="bg-text-muted" style={{ width: share(other) }} />
        </div>
      </div>

      <dl className="grid grid-cols-2 border-y border-border sm:grid-cols-4">
        {counts.map((count, index) => <div key={count.label} className={`min-w-0 px-3 py-3 first:pl-0 sm:px-4 ${index % 2 === 1 ? 'border-l border-border' : ''} ${index >= 2 ? 'border-t border-border sm:border-t-0' : ''} ${index === 2 ? 'sm:border-l' : ''}`}>
          <dt className="flex items-center gap-1.5 text-xs text-text-secondary">
            {count.color && <span className={`h-2 w-2 shrink-0 rounded-full ${count.color}`} aria-hidden />}
            {count.label}
          </dt>
          <dd className="mt-1 text-xl font-semibold tabular-nums">{number.format(count.value)}</dd>
        </div>)}
      </dl>
    </div>
  )
}

function formatDuration(seconds: number, number: Intl.NumberFormat, t: (key: string) => string): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return '—'
  if (seconds < 60) return `${number.format(seconds)} ${t('reports.seconds')}`
  if (seconds < 3600) return `${number.format(seconds / 60)} ${t('reports.minutes')}`
  return `${number.format(seconds / 3600)} ${t('reports.hours')}`
}
