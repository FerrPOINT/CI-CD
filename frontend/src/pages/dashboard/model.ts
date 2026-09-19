import type { Pipeline } from '@/api/types'

export function summarizePipelines(pipelines: Pipeline[]) {
  const recent = [...pipelines].sort((a, b) =>
    new Date(b.created_at).getTime() - new Date(a.created_at).getTime(),
  )

  return {
    queued: pipelines.filter(p => p.status === 'queued').length,
    running: pipelines.filter(p => p.status === 'running').length,
    failed: pipelines.filter(p => p.status === 'failed').length,
    recent: recent.slice(0, 8),
  }
}
