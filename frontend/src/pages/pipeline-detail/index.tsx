import { useEffect, useState } from 'react'
import { useParams, Link } from 'react-router'
import { useTranslation } from 'react-i18next'
import { usePipeline, useUpdateJobStatus, useJobLogPages, useAppendLog, useCancelPipeline, useRetryPipeline, useTestReport, useJobAttempts } from '@/api/hooks'
import { Card } from '@/shared/ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/ui/table'
import type { Job, TestReport } from '@/api/types'
import { Button } from '@/shared/ui/button'
import { Input } from '@/shared/ui/input'
import { ChevronRight, Terminal, ClipboardCheck, Play, CheckCircle2, XCircle, Square, Ban, RotateCcw, Package, FileCode2 } from 'lucide-react'
import { toast } from 'sonner'
import type { PipelinePlan, Status } from '@/api/types'

const statusColors: Record<string, string> = {
  queued: 'bg-text-muted',
  running: 'bg-warning',
  success: 'bg-success',
  failed: 'bg-danger',
  canceled: 'bg-text-muted',
}

export function PipelineDetailPage() {
  const { t } = useTranslation()
  const { pipelineId } = useParams<{ pipelineId: string }>()
  const { data, isLoading } = usePipeline(pipelineId)
  const updateStatus = useUpdateJobStatus()
  const cancelPipeline = useCancelPipeline()
  const retryPipeline = useRetryPipeline()
  const [selectedJobId, setSelectedJobId] = useState<string | null>(null)
  const [logMessage, setLogMessage] = useState('')

  if (isLoading || !data) return <p className="text-sm text-text-muted">{t('common.loading')}</p>

  const { pipeline, stages, plan } = data
  const selectedJob = stages.flatMap(s => s.jobs).find(j => j.id === selectedJobId)

  function handleStatus(jobId: string, status: Status) {
    updateStatus.mutate({ jobId, status }, {
      onError: e => toast.error(e.message),
    })
  }

  return (
    <div className="min-w-0 space-y-6">
      <div>
        <div className="flex min-w-0 flex-wrap items-center gap-2 text-sm text-text-muted">
          <Link to="/projects" className="hover:text-text-primary">{t('navigation.projects')}</Link>
          <ChevronRight className="h-3 w-3" />
          <Link to={`/projects/${pipeline.project_id}/pipelines`} className="hover:text-text-primary">{t('navigation.pipelines')}</Link>
          <ChevronRight className="h-3 w-3" />
          <span>#{pipeline.id.slice(0, 8)}</span>
        </div>
        <div className="mt-2 flex min-w-0 flex-wrap items-center gap-3">
          <h1 className="text-2xl font-bold">#{pipeline.id.slice(0, 8)}</h1>
          {(pipeline.status === 'queued' || pipeline.status === 'running') && (
            <Button size="sm" variant="ghost" className="h-8 gap-1 text-danger hover:text-danger" disabled={cancelPipeline.isPending} onClick={() => cancelPipeline.mutate(pipeline.id)}>
              <Ban className="h-4 w-4" /> {t('pipelines.cancel')}
            </Button>
          )}
          {(pipeline.status === 'failed' || pipeline.status === 'canceled') && (
            <Button size="sm" variant="ghost" className="h-8 gap-1" disabled={retryPipeline.isPending} onClick={() => retryPipeline.mutate(pipeline.id)}>
              <RotateCcw className="h-4 w-4" /> {t('pipelines.retry')}
            </Button>
          )}
          <code className="rounded bg-surface-raised px-2 py-1 text-sm">{pipeline.git_ref}</code>
          <span className={`h-2.5 w-2.5 rounded-full ${statusColors[pipeline.status]}`} />
        </div>
      </div>

      {plan && <PipelinePlanCard plan={plan} />}

      <div className="grid min-w-0 gap-4 lg:grid-cols-3">
        {stages.map(stage => (
          <Card key={stage.id} className="min-w-0 p-4">
            <div className="flex items-center justify-between gap-3 border-b border-border pb-3">
              <h3 className="text-sm font-semibold uppercase tracking-wide">{stage.name}</h3>
              <span className={`h-2.5 w-2.5 rounded-full ${statusColors[stage.status]}`} />
            </div>
            <div className="mt-3 space-y-3">
              {stage.jobs.map(job => (
                <JobCard
                  key={job.id}
                  job={job}
                  onStatus={handleStatus}
                  onShowLogs={() => setSelectedJobId(job.id)}
                />
              ))}
            </div>
          </Card>
        ))}
      </div>

      {selectedJob && (
        <Card className="p-4">
          <div className="flex items-center justify-between border-b border-border pb-3">
            <div className="flex items-center gap-2">
              <Terminal className="h-4 w-4 text-accent" />
              <h3 className="text-sm font-semibold">{t('jobs.logs')} — {selectedJob.name}</h3>
            </div>
            <Button size="sm" variant="ghost" onClick={() => setSelectedJobId(null)}>{t('common.close')}</Button>
          </div>
          <JobLogPanel jobId={selectedJob.id} logMessage={logMessage} setLogMessage={setLogMessage} />
        </Card>
      )}
      {selectedJob && <JobTestReportPanel jobId={selectedJob.id} />}
    </div>
  )
}

function JobCard({
  job,
  onStatus,
  onShowLogs,
}: {
  job: Job
  onStatus: (jobId: string, status: Status) => void
  onShowLogs: () => void
}) {
  const { t } = useTranslation()
  const terminalJobId = job.status === 'failed' || job.status === 'canceled' ? job.id : undefined
  const { data: attempts = [] } = useJobAttempts(terminalJobId)
  const latestDiagnostic = attempts.find(attempt => attempt.error_tail)?.error_tail

  return (
    <div className="min-w-0 rounded-md border border-border p-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <span className={`h-2 w-2 rounded-full ${statusColors[job.status]}`} />
          <span className="break-words text-sm font-medium">{job.name}</span>
        </div>
        <span className="text-xs text-text-muted">{t(`pipelines.${job.status}`)}</span>
      </div>
      <p className="mt-2 min-w-0 break-words text-xs text-text-muted">
        <code className="rounded bg-surface-raised px-1 py-0.5">{job.image}</code>
        <span className="mx-1">·</span>
        <code className="break-all">{job.command}</code>
      </p>
      {job.required_tags.length > 0 && (
        <div className="mt-2 flex flex-wrap items-center gap-1 text-xs text-text-muted">
          <span>{t('jobs.runnerTags')}:</span>
          {job.required_tags.map(tag => (
            <code key={tag} className="rounded bg-surface-raised px-1.5 py-0.5 text-text-primary">
              {tag}
            </code>
          ))}
        </div>
      )}
      {job.required_secrets.length > 0 && (
        <div className="mt-2 flex flex-wrap items-center gap-1 text-xs text-text-muted">
          <span>{t('jobs.secrets')}:</span>
          {job.required_secrets.map(secret => (
            <code key={secret} className="rounded bg-surface-raised px-1.5 py-0.5 text-text-primary">
              {secret}
            </code>
          ))}
        </div>
      )}
      {job.artifact_paths.length > 0 && (
        <div className="mt-2 flex flex-wrap items-center gap-1 text-xs text-text-muted">
          <span>{t('jobs.artifacts')}:</span>
          {job.artifact_paths.map(path => (
            <code key={path} className="rounded bg-surface-raised px-1.5 py-0.5 text-text-primary">
              {path}
            </code>
          ))}
        </div>
      )}
      {latestDiagnostic && (
        <p className="mt-2 min-w-0 break-words rounded border border-danger/30 bg-danger/10 px-2 py-1 text-xs text-danger">
          {latestDiagnostic}
        </p>
      )}
      <div className="mt-3 flex flex-wrap items-center gap-1.5">
        {job.status === 'queued' && (
          <Button size="sm" variant="outline" onClick={() => onStatus(job.id, 'running')}>
            <Play className="h-3 w-3" /> {t('jobs.start')}
          </Button>
        )}
        {job.status === 'running' && (
          <>
            <Button size="sm" variant="outline" onClick={() => onStatus(job.id, 'success')}>
              <CheckCircle2 className="h-3 w-3" /> {t('jobs.pass')}
            </Button>
            <Button size="sm" variant="outline" onClick={() => onStatus(job.id, 'failed')}>
              <XCircle className="h-3 w-3" /> {t('jobs.fail')}
            </Button>
            <Button size="sm" variant="ghost" onClick={() => onStatus(job.id, 'canceled')}>
              <Square className="h-3 w-3" /> {t('jobs.cancel')}
            </Button>
          </>
        )}
        <Button size="sm" variant="ghost" onClick={onShowLogs}>
          <Terminal className="h-3 w-3" /> {t('jobs.logs')}
        </Button>
        <Button asChild size="sm" variant="ghost">
          <Link to={`/jobs/${job.id}/artifacts`}>
            <Package className="h-3 w-3" /> {t('artifacts.title')}
          </Link>
        </Button>
      </div>
    </div>
  )
}

function PipelinePlanCard({ plan }: { plan: PipelinePlan }) {
  const { t } = useTranslation()
  return (
    <Card className="min-w-0 p-4">
      <div className="flex min-w-0 flex-wrap items-center justify-between gap-3 border-b border-border pb-3">
        <div className="flex min-w-0 items-center gap-2">
          <FileCode2 className="h-4 w-4 shrink-0 text-accent" />
          <h2 className="text-sm font-semibold">{t('pipelines.planTitle')}</h2>
        </div>
        <span className="rounded bg-surface-raised px-2 py-1 text-xs font-medium text-text-muted">
          {plan.config_source}
        </span>
      </div>
      <dl className="mt-3 grid min-w-0 gap-3 text-xs sm:grid-cols-2 lg:grid-cols-3">
        <PlanFact label={t('pipelines.planParser')} value={plan.parser_version} />
        <PlanFact label={t('pipelines.planCommit')} value={plan.resolved_commit_sha ?? '-'} mono />
        <PlanFact label={t('pipelines.planEdges')} value={String(planDependencyCount(plan.plan))} />
        <PlanFact label={t('pipelines.planConfigHash')} value={plan.config_sha256} mono />
        <PlanFact label={t('pipelines.planHash')} value={plan.plan_sha256} mono />
      </dl>
    </Card>
  )
}

function PlanFact({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="min-w-0">
      <dt className="text-text-muted">{label}</dt>
      <dd className={`mt-1 break-all ${mono ? 'font-mono text-[11px]' : 'font-medium'}`}>{value}</dd>
    </div>
  )
}

function planDependencyCount(plan: unknown): number {
  if (!plan || typeof plan !== 'object') return 0
  const dependencies = (plan as { dependencies?: unknown }).dependencies
  return Array.isArray(dependencies) ? dependencies.length : 0
}

function JobLogPanel({ jobId, logMessage, setLogMessage }: { jobId: string; logMessage: string; setLogMessage: (v: string) => void }) {
  const { t } = useTranslation()
  const { data: attempts = [] } = useJobAttempts(jobId)
  const [selectedAttemptId, setSelectedAttemptId] = useState<string | null>(null)
  const [logSearch, setLogSearch] = useState('')
  const selectedAttempt = attempts.find(a => a.id === selectedAttemptId) ?? attempts[0]
  const logPages = useJobLogPages(jobId, selectedAttempt?.id, logSearch)
  const logs = logPages.data?.pages.flatMap(page => page.items) ?? []
  const appendLog = useAppendLog()
  const activeAttemptId = attempts[0]?.id
  const canAppend = !selectedAttempt || selectedAttempt.id === activeAttemptId

  useEffect(() => {
    if (attempts.length === 0) {
      setSelectedAttemptId(null)
      return
    }
    if (!selectedAttemptId || !attempts.some(a => a.id === selectedAttemptId)) {
      setSelectedAttemptId(attempts[0].id)
    }
  }, [attempts, selectedAttemptId])

  return (
    <div className="mt-3 space-y-3">
      {attempts.length > 0 && (
        <div className="space-y-2">
          <div className="flex flex-wrap gap-2">
            {attempts.map(attempt => (
              <Button
                key={attempt.id}
                type="button"
                size="sm"
                variant={attempt.id === selectedAttempt?.id ? 'default' : 'outline'}
                onClick={() => setSelectedAttemptId(attempt.id)}
              >
                {t('jobs.attempt')} #{attempt.attempt_no} · {t(`pipelines.${attempt.status}`)}
              </Button>
            ))}
          </div>
          {selectedAttempt && (
            <div className="grid gap-2 text-xs text-text-muted sm:grid-cols-2 lg:grid-cols-4">
              <span>{t('jobs.trigger')}: {selectedAttempt.trigger}</span>
              <span>{t('jobs.exitCode')}: {selectedAttempt.exit_code ?? '-'}</span>
              <span>{t('jobs.startedAt')}: {formatAttemptTime(selectedAttempt.started_at)}</span>
              <span>{t('jobs.finishedAt')}: {formatAttemptTime(selectedAttempt.finished_at)}</span>
            </div>
          )}
          {selectedAttempt?.error_tail && (
            <p className="rounded-md border border-danger/40 bg-danger/10 p-2 text-xs text-danger">
              {selectedAttempt.error_tail}
            </p>
          )}
        </div>
      )}
      <div className="flex flex-col gap-2 sm:flex-row">
        <Input
          value={logSearch}
          onChange={e => setLogSearch(e.target.value)}
          placeholder={t('jobs.searchLogs')}
          className="font-mono text-sm"
        />
        {logSearch && (
          <Button type="button" size="sm" variant="outline" onClick={() => setLogSearch('')}>
            {t('common.clear')}
          </Button>
        )}
      </div>
      <pre className="max-h-80 overflow-auto rounded-md bg-zinc-950 p-3 text-xs text-green-400">
        {logPages.isLoading ? t('common.loading') : logs.length === 0 ? '<no logs>' : logs.map(l => `${String(l.sequence).padStart(3, '0')}  ${l.message}`).join('\n')}
      </pre>
      {logPages.hasNextPage && (
        <Button
          type="button"
          size="sm"
          variant="outline"
          disabled={logPages.isFetchingNextPage}
          onClick={() => logPages.fetchNextPage()}
        >
          {logPages.isFetchingNextPage ? t('common.loading') : t('jobs.loadMoreLogs')}
        </Button>
      )}
      {canAppend && (
        <form
          className="flex gap-2"
          onSubmit={(e) => {
            e.preventDefault()
            if (!logMessage.trim()) return
            appendLog.mutate({ jobId, message: logMessage.trim() }, { onSuccess: () => setLogMessage(''), onError: err => toast.error(err.message) })
          }}
        >
          <Input value={logMessage} onChange={e => setLogMessage(e.target.value)} placeholder={t('jobs.logMessage')} className="font-mono text-sm" />
          <Button type="submit" size="sm">{t('jobs.append')}</Button>
        </form>
      )}
    </div>
  )
}

function formatAttemptTime(value: string | null): string {
  return value ? new Date(value).toLocaleString() : '-'
}


function JobTestReportPanel({ jobId }: { jobId: string }) {
  const { t } = useTranslation()
  const { data: reports = [], isLoading } = useTestReport(jobId)
  if (isLoading) return null
  if (reports.length === 0) return null
  const total = reports.reduce<{ total: number; passed: number; failed: number; skipped: number }>((acc, r) => ({ total: acc.total + r.tests_total, passed: acc.passed + r.tests_passed, failed: acc.failed + r.tests_failed, skipped: acc.skipped + r.tests_skipped }), { total: 0, passed: 0, failed: 0, skipped: 0 })
  return (
    <Card className="p-4">
      <div className="flex items-center gap-2 border-b border-border pb-3">
        <ClipboardCheck className="h-4 w-4 text-accent" />
        <h3 className="text-sm font-semibold">{t('jobs.testReport', 'Тест-отчёты')} — {selectedLabel(total)}</h3>
      </div>
      <div className="mt-3 grid gap-3 md:hidden">
        {reports.map((r: TestReport) => (
          <Card key={r.id} className="p-3 text-sm">
            <p className="font-medium">{r.suite_name}</p>
            <p className="mt-1 text-xs text-text-muted">
              ✓{r.tests_passed} ✗{r.tests_failed} ⇒{r.tests_skipped} / {r.tests_total}
              {r.duration_ms != null && ` · ${r.duration_ms}ms`}
            </p>
          </Card>
        ))}
      </div>
      <div className="mt-3 hidden overflow-hidden md:block">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t('jobs.suite', 'Набор')}</TableHead>
              <TableHead>✓</TableHead>
              <TableHead>✗</TableHead>
              <TableHead>⇢</TableHead>
              <TableHead>{t('jobs.total', 'Всего')}</TableHead>
              <TableHead>{t('jobs.duration', 'Время')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {reports.map((r: TestReport) => (
              <TableRow key={r.id}>
                <TableCell className="font-medium">{r.suite_name}</TableCell>
                <TableCell className="text-emerald-500">{r.tests_passed}</TableCell>
                <TableCell className={r.tests_failed > 0 ? 'text-destructive' : ''}>{r.tests_failed}</TableCell>
                <TableCell className="text-text-muted">{r.tests_skipped}</TableCell>
                <TableCell>{r.tests_total}</TableCell>
                <TableCell className="text-text-muted">{r.duration_ms != null ? `${r.duration_ms}ms` : '-'}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </Card>
  )

  function selectedLabel(sum: { total: number; passed: number; failed: number; skipped: number }): string {
    return `${sum.passed}/${sum.total}`
  }
}
