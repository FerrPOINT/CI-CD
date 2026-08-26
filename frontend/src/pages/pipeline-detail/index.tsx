import { useState } from 'react'
import { useParams, Link } from 'react-router'
import { useTranslation } from 'react-i18next'
import { usePipeline, useUpdateJobStatus, useJobLogs, useAppendLog, useCancelPipeline, useRetryPipeline } from '@/api/hooks'
import { Card } from '@/shared/ui/card'
import { Button } from '@/shared/ui/button'
import { Input } from '@/shared/ui/input'
import { ChevronRight, Terminal, Play, CheckCircle2, XCircle, Square, Ban, RotateCcw, Package } from 'lucide-react'
import { toast } from 'sonner'
import type { Status } from '@/api/types'

const statusColors: Record<Status, string> = {
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

  const { pipeline, stages } = data
  const selectedJob = stages.flatMap(s => s.jobs).find(j => j.id === selectedJobId)

  function handleStatus(jobId: string, status: Status) {
    updateStatus.mutate({ jobId, status }, {
      onError: e => toast.error(e.message),
    })
  }

  return (
    <div className="space-y-6">
      <div>
        <div className="flex items-center gap-2 text-sm text-text-muted">
          <Link to="/projects" className="hover:text-text-primary">{t('navigation.projects')}</Link>
          <ChevronRight className="h-3 w-3" />
          <Link to={`/projects/${pipeline.project_id}/pipelines`} className="hover:text-text-primary">{t('navigation.pipelines')}</Link>
          <ChevronRight className="h-3 w-3" />
          <span>#{pipeline.id.slice(0, 8)}</span>
        </div>
        <div className="mt-2 flex items-center gap-3">
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

      <div className="grid gap-4 lg:grid-cols-3">
        {stages.map(stage => (
          <Card key={stage.id} className="p-4">
            <div className="flex items-center justify-between border-b border-border pb-3">
              <h3 className="text-sm font-semibold uppercase tracking-wide">{stage.name}</h3>
              <span className={`h-2.5 w-2.5 rounded-full ${statusColors[stage.status]}`} />
            </div>
            <div className="mt-3 space-y-3">
              {stage.jobs.map(job => (
                <div key={job.id} className="rounded-md border border-border p-3">
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <span className={`h-2 w-2 rounded-full ${statusColors[job.status]}`} />
                      <span className="text-sm font-medium">{job.name}</span>
                    </div>
                    <span className="text-xs text-text-muted">{t(`pipelines.${job.status}`)}</span>
                  </div>
                  <p className="mt-2 text-xs text-text-muted">
                    <code className="rounded bg-surface-raised px-1 py-0.5">{job.image}</code>
                    <span className="mx-1">·</span>
                    <code>{job.command}</code>
                  </p>
                  <div className="mt-3 flex items-center gap-1.5">
                    {job.status === 'queued' && (
                      <Button size="sm" variant="outline" onClick={() => handleStatus(job.id, 'running')}>
                        <Play className="h-3 w-3" /> {t('jobs.start')}
                      </Button>
                    )}
                    {job.status === 'running' && (
                      <>
                        <Button size="sm" variant="outline" onClick={() => handleStatus(job.id, 'success')}>
                          <CheckCircle2 className="h-3 w-3" /> {t('jobs.pass')}
                        </Button>
                        <Button size="sm" variant="outline" onClick={() => handleStatus(job.id, 'failed')}>
                          <XCircle className="h-3 w-3" /> {t('jobs.fail')}
                        </Button>
                        <Button size="sm" variant="ghost" onClick={() => handleStatus(job.id, 'canceled')}>
                          <Square className="h-3 w-3" /> {t('jobs.cancel')}
                        </Button>
                      </>
                    )}
                    <Button size="sm" variant="ghost" onClick={() => setSelectedJobId(job.id)}>
                      <Terminal className="h-3 w-3" /> {t('jobs.logs')}
                    </Button>
                    <Button asChild size="sm" variant="ghost">
                      <Link to={`/jobs/${job.id}/artifacts`}>
                        <Package className="h-3 w-3" /> {t('artifacts.title')}
                      </Link>
                    </Button>
                  </div>
                </div>
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
    </div>
  )
}

function JobLogPanel({ jobId, logMessage, setLogMessage }: { jobId: string; logMessage: string; setLogMessage: (v: string) => void }) {
  const { data: logs = [] } = useJobLogs(jobId)
  const appendLog = useAppendLog()

  return (
    <div className="mt-3 space-y-3">
      <pre className="max-h-80 overflow-auto rounded-md bg-zinc-950 p-3 text-xs text-green-400">
        {logs.length === 0 ? '<no logs>' : logs.map(l => `${String(l.sequence).padStart(3, '0')}  ${l.message}`).join('\n')}
      </pre>
      <form
        className="flex gap-2"
        onSubmit={(e) => {
          e.preventDefault()
          if (!logMessage.trim()) return
          appendLog.mutate({ jobId, message: logMessage.trim() }, { onSuccess: () => setLogMessage(''), onError: err => toast.error(err.message) })
        }}
      >
        <Input value={logMessage} onChange={e => setLogMessage(e.target.value)} placeholder="Log message…" className="font-mono text-sm" />
        <Button type="submit" size="sm">Append</Button>
      </form>
    </div>
  )
}
