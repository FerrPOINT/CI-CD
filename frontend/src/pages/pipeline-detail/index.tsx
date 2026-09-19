import { useEffect, useMemo, useRef, useState } from 'react'
import { useParams, Link } from 'react-router'
import { useTranslation } from 'react-i18next'
import { ApiError } from '@/api/client'
import { ForbiddenPage } from '@/pages/forbidden'
import {
  usePipeline,
  useUpdateJobStatus,
  useJobLogPages,
  useAppendLog,
  useCancelPipeline,
  useRetryPipeline,
  useTestReport,
  useJobAttempts,
} from '@/api/hooks'
import { Card } from '@sdlc/ui/ui'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@sdlc/ui/ui'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@sdlc/ui/ui'
import type { Job, TestReport } from '@/api/types'
import { Button } from '@sdlc/ui/ui'
import { Input } from '@sdlc/ui/ui'
import {
  ChevronRight,
  Terminal,
  ClipboardCheck,
  Play,
  CheckCircle2,
  XCircle,
  Square,
  Ban,
  RotateCcw,
  Package,
  FileCode2,
} from 'lucide-react'
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
  const { data, isLoading, error, refetch } = usePipeline(pipelineId)
  const updateStatus = useUpdateJobStatus()
  const cancelPipeline = useCancelPipeline()
  const retryPipeline = useRetryPipeline()
  const [selectedJobId, setSelectedJobId] = useState<string | null>(null)
  const [logMessage, setLogMessage] = useState('')
  const [confirmCancel, setConfirmCancel] = useState(false)
  const logPanelRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (selectedJobId) logPanelRef.current?.scrollIntoView?.({ block: 'start' })
  }, [selectedJobId])

  if (isLoading)
    return (
      <p role="status" className="text-sm text-text-muted">
        {t('common.loading')}
      </p>
    )
  if (error && !data) {
    if (error instanceof ApiError && error.status === 403) return <ForbiddenPage />
    return (
      <div role="alert" className="flex flex-wrap items-center gap-3 text-sm text-danger">
        <span>{t('pipelines.loadError')}</span>
        <Button type="button" variant="outline" className="min-h-10" onClick={() => void refetch()}>
          {t('common.retry')}
        </Button>
      </div>
    )
  }
  if (!data) return <p className="text-sm text-text-muted">{t('common.noData')}</p>

  const { pipeline, stages, plan } = data
  const selectedJob = stages.flatMap((s) => s.jobs).find((j) => j.id === selectedJobId)

  function handleStatus(jobId: string, status: Status) {
    updateStatus.mutate(
      { jobId, status },
      {
        onSuccess: () => toast.success(t('jobs.statusUpdated')),
        onError: (e) => toast.error(e.message),
      },
    )
  }

  function handleCancel() {
    cancelPipeline.mutate(pipeline.id, {
      onSuccess: () => {
        setConfirmCancel(false)
        toast.success(t('pipelines.canceledNotice'))
      },
      onError: () => toast.error(t('pipelines.actionError')),
    })
  }

  function handleRetry() {
    retryPipeline.mutate(pipeline.id, {
      onSuccess: () => toast.success(t('pipelines.retryStarted')),
      onError: () => toast.error(t('pipelines.actionError')),
    })
  }

  return (
    <div className="min-w-0 space-y-6">
      <div>
        <div className="flex min-w-0 flex-wrap items-center gap-2 text-sm text-text-muted">
          <Link
            to="/projects"
            className="inline-flex min-h-10 items-center hover:text-text-primary"
          >
            {t('navigation.projects')}
          </Link>
          <ChevronRight className="h-3 w-3" />
          <Link
            to={`/projects/${pipeline.project_id}/pipelines`}
            className="inline-flex min-h-10 items-center hover:text-text-primary"
          >
            {t('navigation.pipelines')}
          </Link>
          <ChevronRight className="h-3 w-3" />
          <span>#{pipeline.id.slice(0, 8)}</span>
        </div>
        <div className="mt-2 flex min-w-0 flex-wrap items-center gap-3">
          <h1 className="text-2xl font-bold">#{pipeline.id.slice(0, 8)}</h1>
          {(pipeline.status === 'queued' || pipeline.status === 'running') && (
            <Button
              size="sm"
              variant="outline"
              className="min-h-10 gap-1 text-danger hover:text-danger sm:min-h-10"
              disabled={cancelPipeline.isPending}
              onClick={() => setConfirmCancel(true)}
            >
              <Ban className="h-4 w-4" /> {t('pipelines.cancel')}
            </Button>
          )}
          {(pipeline.status === 'failed' || pipeline.status === 'canceled') && (
            <Button
              size="sm"
              variant="outline"
              className="min-h-10 gap-1 sm:min-h-10"
              disabled={retryPipeline.isPending}
              onClick={handleRetry}
            >
              <RotateCcw className="h-4 w-4" /> {t('pipelines.retry')}
            </Button>
          )}
          <code className="rounded bg-surface-raised px-2 py-1 text-sm">{pipeline.git_ref}</code>
          <span role="status" className="flex items-center gap-1.5 text-sm text-text-secondary">
            <span
              className={`h-2.5 w-2.5 rounded-full ${statusColors[pipeline.status]}`}
              aria-hidden
            />
            {t(`pipelines.${pipeline.status}`)}
          </span>
        </div>
      </div>

      {error && (
        <div role="alert" className="flex flex-wrap items-center gap-2 text-sm text-danger">
          <span>{t('pipelines.loadError')}</span>
          <Button
            type="button"
            variant="outline"
            className="min-h-10"
            onClick={() => void refetch()}
          >
            {t('common.retry')}
          </Button>
        </div>
      )}

      {plan && <PipelinePlanCard plan={plan} />}

      <div className="grid min-w-0 gap-4 lg:grid-cols-3">
        {stages.map((stage) => (
          <Card key={stage.id} className="min-w-0 p-4">
            <div className="flex items-center justify-between gap-3 border-b border-border pb-3">
              <h3 className="min-w-0 break-words text-sm font-semibold">{stage.name}</h3>
              <span className="flex shrink-0 items-center gap-1.5 text-xs text-text-muted">
                <span
                  className={`h-2.5 w-2.5 rounded-full ${statusColors[stage.status]}`}
                  aria-hidden
                />
                {t(`pipelines.${stage.status}`)}
              </span>
            </div>
            <div className="mt-3 space-y-3">
              {stage.jobs.map((job) => (
                <JobCard
                  key={job.id}
                  job={job}
                  onStatus={handleStatus}
                  onShowLogs={() => setSelectedJobId(job.id)}
                  pending={updateStatus.isPending}
                />
              ))}
            </div>
          </Card>
        ))}
      </div>

      {selectedJob && (
        <div ref={logPanelRef} className="scroll-mt-16">
          <Card className="p-4">
            <div className="flex items-center justify-between border-b border-border pb-3">
              <div className="flex items-center gap-2">
                <Terminal className="h-4 w-4 text-accent" />
                <h3 className="text-sm font-semibold">
                  {t('jobs.logs')} — {selectedJob.name}
                </h3>
              </div>
              <Button
                size="sm"
                variant="outline"
                className="min-h-10 sm:min-h-10"
                onClick={() => setSelectedJobId(null)}
              >
                {t('common.close')}
              </Button>
            </div>
            <JobLogPanel
              jobId={selectedJob.id}
              live={selectedJob.status === 'queued' || selectedJob.status === 'running'}
              logMessage={logMessage}
              setLogMessage={setLogMessage}
            />
          </Card>
        </div>
      )}
      {selectedJob && <JobTestReportPanel jobId={selectedJob.id} />}
      <AlertDialog open={confirmCancel} onOpenChange={setConfirmCancel}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('pipelines.cancelTitle')}</AlertDialogTitle>
            <AlertDialogDescription>{t('pipelines.cancelDescription')}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel className="min-h-10" disabled={cancelPipeline.isPending}>
              {t('pipelines.keepRunning')}
            </AlertDialogCancel>
            <AlertDialogAction
              className="min-h-10"
              disabled={cancelPipeline.isPending}
              onClick={(event) => {
                event.preventDefault()
                handleCancel()
              }}
            >
              {t('pipelines.confirmCancel')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}

function JobCard({
  job,
  onStatus,
  onShowLogs,
  pending,
}: {
  job: Job
  onStatus: (jobId: string, status: Status) => void
  onShowLogs: () => void
  pending: boolean
}) {
  const { t } = useTranslation()
  const terminalJobId = job.status === 'failed' || job.status === 'canceled' ? job.id : undefined
  const { data: attempts = [] } = useJobAttempts(terminalJobId)
  const latestDiagnostic = attempts.find((attempt) => attempt.error_tail)?.error_tail

  return (
    <div className="min-w-0 border-t border-border py-3 first:border-t-0">
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
          {job.required_tags.map((tag) => (
            <code key={tag} className="rounded bg-surface-raised px-1.5 py-0.5 text-text-primary">
              {tag}
            </code>
          ))}
        </div>
      )}
      {job.required_secrets.length > 0 && (
        <div className="mt-2 flex flex-wrap items-center gap-1 text-xs text-text-muted">
          <span>{t('jobs.secrets')}:</span>
          {job.required_secrets.map((secret) => (
            <code
              key={secret}
              className="rounded bg-surface-raised px-1.5 py-0.5 text-text-primary"
            >
              {secret}
            </code>
          ))}
        </div>
      )}
      {job.artifact_paths.length > 0 && (
        <div className="mt-2 flex flex-wrap items-center gap-1 text-xs text-text-muted">
          <span>{t('jobs.artifacts')}:</span>
          {job.artifact_paths.map((path) => (
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
        <Button size="sm" variant="outline" className="min-h-10 sm:min-h-10" onClick={onShowLogs}>
          <Terminal className="h-3 w-3" /> {t('jobs.logs')}
        </Button>
        <Button asChild size="sm" variant="outline" className="min-h-10 sm:min-h-10">
          <Link to={`/jobs/${job.id}/artifacts`}>
            <Package className="h-3 w-3" /> {t('artifacts.title')}
          </Link>
        </Button>
      </div>
      {(job.status === 'queued' || job.status === 'running') && (
        <details className="group mt-2 text-sm text-text-muted">
          <summary className="flex min-h-10 cursor-pointer items-center gap-1">
            <ChevronRight
              className="h-4 w-4 transition-transform group-open:rotate-90"
              aria-hidden
            />
            {t('jobs.manualStatus')}
          </summary>
          <div className="flex flex-wrap gap-2 pb-1">
            {job.status === 'queued' && (
              <Button
                size="sm"
                variant="outline"
                className="min-h-10 sm:min-h-10"
                disabled={pending}
                onClick={() => onStatus(job.id, 'running')}
              >
                <Play className="h-3 w-3" /> {t('jobs.start')}
              </Button>
            )}
            {job.status === 'running' && (
              <>
                <Button
                  size="sm"
                  variant="outline"
                  className="min-h-10 sm:min-h-10"
                  disabled={pending}
                  onClick={() => onStatus(job.id, 'success')}
                >
                  <CheckCircle2 className="h-3 w-3" /> {t('jobs.pass')}
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  className="min-h-10 sm:min-h-10"
                  disabled={pending}
                  onClick={() => onStatus(job.id, 'failed')}
                >
                  <XCircle className="h-3 w-3" /> {t('jobs.fail')}
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  className="min-h-10 sm:min-h-10"
                  disabled={pending}
                  onClick={() => onStatus(job.id, 'canceled')}
                >
                  <Square className="h-3 w-3" /> {t('jobs.cancel')}
                </Button>
              </>
            )}
          </div>
        </details>
      )}
    </div>
  )
}

function PipelinePlanCard({ plan }: { plan: PipelinePlan }) {
  const { t } = useTranslation()
  return (
    <Card className="min-w-0 px-4">
      <details className="group">
        <summary className="flex min-h-12 cursor-pointer items-center justify-between gap-3 py-2">
          <span className="flex min-w-0 items-center gap-2">
            <FileCode2 className="h-4 w-4 shrink-0 text-accent" aria-hidden />
            <span className="text-sm font-semibold">{t('pipelines.planTitle')}</span>
          </span>
          <span className="flex min-w-0 items-center gap-2 text-xs text-text-muted">
            <span className="break-all">{plan.config_source}</span>
            <ChevronRight
              className="h-4 w-4 shrink-0 transition-transform group-open:rotate-90"
              aria-hidden
            />
          </span>
        </summary>
        <dl className="grid min-w-0 gap-3 border-t border-border py-3 text-xs sm:grid-cols-2 lg:grid-cols-3">
          <PlanFact label={t('pipelines.planParser')} value={plan.parser_version} />
          <PlanFact
            label={t('pipelines.planCommit')}
            value={plan.resolved_commit_sha ?? '-'}
            mono
          />
          <PlanFact
            label={t('pipelines.planEdges')}
            value={String(planDependencyCount(plan.plan))}
          />
          <PlanFact label={t('pipelines.planConfigHash')} value={plan.config_sha256} mono />
          <PlanFact label={t('pipelines.planHash')} value={plan.plan_sha256} mono />
        </dl>
      </details>
    </Card>
  )
}

function PlanFact({
  label,
  value,
  mono = false,
}: {
  label: string
  value: string
  mono?: boolean
}) {
  return (
    <div className="min-w-0">
      <dt className="text-text-muted">{label}</dt>
      <dd className={`mt-1 break-all ${mono ? 'font-mono text-[11px]' : 'font-medium'}`}>
        {value}
      </dd>
    </div>
  )
}

function planDependencyCount(plan: unknown): number {
  if (!plan || typeof plan !== 'object') return 0
  const dependencies = (plan as { dependencies?: unknown }).dependencies
  return Array.isArray(dependencies) ? dependencies.length : 0
}

function JobLogPanel({
  jobId,
  live,
  logMessage,
  setLogMessage,
}: {
  jobId: string
  live: boolean
  logMessage: string
  setLogMessage: (v: string) => void
}) {
  const { t } = useTranslation()
  const attemptsQuery = useJobAttempts(jobId, live)
  const attempts = useMemo(() => attemptsQuery.data ?? [], [attemptsQuery.data])
  const [selectedAttemptId, setSelectedAttemptId] = useState<string | null>(null)
  const [logSearch, setLogSearch] = useState('')
  const [appliedLogSearch, setAppliedLogSearch] = useState('')
  const selectedAttempt = attempts.find((a) => a.id === selectedAttemptId) ?? attempts[0]
  const logPages = useJobLogPages(jobId, selectedAttempt?.id, appliedLogSearch, live)
  const logs = logPages.data?.pages.flatMap((page) => page.items) ?? []
  const appendLog = useAppendLog()
  const activeAttemptId = attempts[0]?.id
  const canAppend = !!selectedAttempt && selectedAttempt.id === activeAttemptId

  useEffect(() => {
    const timeout = window.setTimeout(() => setAppliedLogSearch(logSearch.trim()), 300)
    return () => window.clearTimeout(timeout)
  }, [logSearch])

  useEffect(() => {
    if (attempts.length === 0) {
      setSelectedAttemptId(null)
      return
    }
    if (!selectedAttemptId || !attempts.some((a) => a.id === selectedAttemptId)) {
      setSelectedAttemptId(attempts[0].id)
    }
  }, [attempts, selectedAttemptId])

  return (
    <div className="mt-3 space-y-3">
      {attemptsQuery.isError && (
        <div role="alert" className="flex flex-wrap items-center gap-2 text-sm text-danger">
          <span>{t('jobs.attemptsError')}</span>
          <Button
            type="button"
            variant="outline"
            className="min-h-10"
            onClick={() => void attemptsQuery.refetch()}
          >
            {t('common.retry')}
          </Button>
        </div>
      )}
      {attempts.length > 0 && (
        <div className="space-y-2">
          <div className="flex flex-wrap gap-2">
            {attempts.map((attempt) => (
              <Button
                key={attempt.id}
                type="button"
                size="sm"
                variant={attempt.id === selectedAttempt?.id ? 'default' : 'outline'}
                className="min-h-10 sm:min-h-10"
                onClick={() => setSelectedAttemptId(attempt.id)}
              >
                {t('jobs.attempt')} #{attempt.attempt_no} · {t(`pipelines.${attempt.status}`)}
              </Button>
            ))}
          </div>
          {selectedAttempt && (
            <div className="grid gap-2 text-xs text-text-muted sm:grid-cols-2 lg:grid-cols-4">
              <span>
                {t('jobs.trigger')}: {selectedAttempt.trigger}
              </span>
              <span>
                {t('jobs.exitCode')}: {selectedAttempt.exit_code ?? '-'}
              </span>
              <span>
                {t('jobs.startedAt')}: {formatAttemptTime(selectedAttempt.started_at)}
              </span>
              <span>
                {t('jobs.finishedAt')}: {formatAttemptTime(selectedAttempt.finished_at)}
              </span>
            </div>
          )}
          {selectedAttempt?.error_tail && (
            <p className="rounded-md border border-danger/40 bg-danger/10 p-2 text-xs text-danger">
              {selectedAttempt.error_tail}
            </p>
          )}
        </div>
      )}
      {selectedAttempt && (
        <>
          <div className="flex flex-wrap gap-2">
            <Input
              type="search"
              aria-label={t('jobs.searchLogs')}
              value={logSearch}
              onChange={(e) => setLogSearch(e.target.value)}
              placeholder={t('jobs.searchLogs')}
              className="min-h-10 min-w-40 flex-1 font-mono text-sm"
            />
            {logSearch && (
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="min-h-10 sm:min-h-10"
                onClick={() => setLogSearch('')}
              >
                {t('common.clear')}
              </Button>
            )}
            <Button
              type="button"
              size="sm"
              variant="outline"
              className="min-h-10 sm:min-h-10"
              onClick={() => void logPages.refetch()}
            >
              <RotateCcw className="h-4 w-4" aria-hidden /> {t('jobs.refreshLogs')}
            </Button>
          </div>
          {logPages.isError && (
            <div role="alert" className="flex flex-wrap items-center gap-2 text-sm text-danger">
              <span>{t('jobs.logsError')}</span>
              <Button
                type="button"
                variant="outline"
                className="min-h-10"
                onClick={() => void logPages.refetch()}
              >
                {t('common.retry')}
              </Button>
            </div>
          )}
          {!logPages.isError && (
            <pre
              role="log"
              aria-label={t('jobs.logs')}
              className="max-h-80 overflow-auto rounded-md bg-zinc-950 p-3 text-xs text-green-400"
            >
              {logPages.isLoading
                ? t('common.loading')
                : logs.length === 0
                  ? t('jobs.noLogs')
                  : logs
                      .map((l) => `${String(l.sequence).padStart(3, '0')}  ${l.message}`)
                      .join('\n')}
            </pre>
          )}
          {logPages.hasNextPage && !logPages.isError && (
            <Button
              type="button"
              size="sm"
              variant="outline"
              className="min-h-10 sm:min-h-10"
              disabled={logPages.isFetchingNextPage}
              onClick={() => void logPages.fetchNextPage()}
            >
              {logPages.isFetchingNextPage ? t('common.loading') : t('jobs.loadMoreLogs')}
            </Button>
          )}
        </>
      )}
      {!selectedAttempt && !attemptsQuery.isLoading && !attemptsQuery.isError && (
        <p className="text-sm text-text-muted">{t('jobs.noLogs')}</p>
      )}
      {canAppend && (
        <details className="group text-sm text-text-muted">
          <summary className="flex min-h-10 cursor-pointer items-center gap-1">
            <ChevronRight
              className="h-4 w-4 transition-transform group-open:rotate-90"
              aria-hidden
            />
            {t('jobs.manualLog')}
          </summary>
          <form
            className="flex flex-wrap gap-2"
            onSubmit={(e) => {
              e.preventDefault()
              if (!logMessage.trim() || appendLog.isPending) return
              appendLog.mutate(
                { jobId, message: logMessage.trim() },
                { onSuccess: () => setLogMessage(''), onError: (err) => toast.error(err.message) },
              )
            }}
          >
            <Input
              aria-label={t('jobs.logMessage')}
              value={logMessage}
              onChange={(e) => setLogMessage(e.target.value)}
              placeholder={t('jobs.logMessage')}
              className="min-h-10 min-w-40 flex-1 font-mono text-sm"
              disabled={appendLog.isPending}
            />
            <Button
              type="submit"
              size="sm"
              className="min-h-10 sm:min-h-10"
              disabled={appendLog.isPending || !logMessage.trim()}
            >
              {t('jobs.append')}
            </Button>
          </form>
        </details>
      )}
    </div>
  )
}

function formatAttemptTime(value: string | null): string {
  return value ? new Date(value).toLocaleString() : '-'
}

function JobTestReportPanel({ jobId }: { jobId: string }) {
  const { t } = useTranslation()
  const { data: reports = [], isLoading, isError, refetch } = useTestReport(jobId)
  if (isLoading) return null
  if (isError)
    return (
      <div role="alert" className="flex flex-wrap items-center gap-2 text-sm text-danger">
        <span>{t('jobs.reportError')}</span>
        <Button type="button" variant="outline" className="min-h-10" onClick={() => void refetch()}>
          {t('common.retry')}
        </Button>
      </div>
    )
  if (reports.length === 0) return null
  const total = reports.reduce<{ total: number; passed: number; failed: number; skipped: number }>(
    (acc, r) => ({
      total: acc.total + r.tests_total,
      passed: acc.passed + r.tests_passed,
      failed: acc.failed + r.tests_failed,
      skipped: acc.skipped + r.tests_skipped,
    }),
    { total: 0, passed: 0, failed: 0, skipped: 0 },
  )
  return (
    <Card className="p-4">
      <div className="flex items-center gap-2 border-b border-border pb-3">
        <ClipboardCheck className="h-4 w-4 text-accent" />
        <h3 className="text-sm font-semibold">
          {t('jobs.testReport', 'Тест-отчёты')} — {selectedLabel(total)}
        </h3>
      </div>
      <div className="mt-3 divide-y divide-border border-y border-border md:hidden">
        {reports.map((r: TestReport) => (
          <div key={r.id} className="py-3 text-sm">
            <p className="font-medium">{r.suite_name}</p>
            <p className="mt-1 text-xs text-text-muted">
              ✓{r.tests_passed} ✗{r.tests_failed} ⇒{r.tests_skipped} / {r.tests_total}
              {r.duration_ms != null && ` · ${r.duration_ms}ms`}
            </p>
          </div>
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
                <TableCell className={r.tests_failed > 0 ? 'text-destructive' : ''}>
                  {r.tests_failed}
                </TableCell>
                <TableCell className="text-text-muted">{r.tests_skipped}</TableCell>
                <TableCell>{r.tests_total}</TableCell>
                <TableCell className="text-text-muted">
                  {r.duration_ms != null ? `${r.duration_ms}ms` : '-'}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </Card>
  )

  function selectedLabel(sum: {
    total: number
    passed: number
    failed: number
    skipped: number
  }): string {
    return `${sum.passed}/${sum.total}`
  }
}
