import { useEffect, useState } from 'react'
import { Link, useParams, useSearchParams } from 'react-router'
import { useTranslation } from 'react-i18next'
import { ChevronRight, FileDiff, GitCompareArrows } from 'lucide-react'
import { useRepositoryComparison, useRepositoryRefs } from '@/api/hooks'
import type { ChangeStatus, Comparison } from '@/api/types'
import { QueryState } from '@/shared/ui/query-state'
import { Button, Input, Label } from '@sdlc/ui/ui'

const changeStatusStyles: Record<ChangeStatus, string> = {
  added: 'bg-success/15 text-text-primary',
  modified: 'bg-warning/15 text-text-primary',
  deleted: 'bg-danger/15 text-text-primary',
}

function PatchView({ patch }: { patch: string }) {
  return (
    <pre className="max-h-[48rem] overflow-auto rounded-md bg-zinc-950 p-4 text-xs leading-relaxed">
      {patch.split('\n').map((line, index) => {
        const key = `${index}-${line}`
        if (line.startsWith('+++') || line.startsWith('---')) return <span key={key} className="block font-mono text-indigo-300">{line}</span>
        if (line.startsWith('@@')) return <span key={key} className="block font-mono text-sky-400">{line}</span>
        if (line.startsWith('+')) return <span key={key} className="block bg-green-500/10 font-mono text-green-400">{line}</span>
        if (line.startsWith('-')) return <span key={key} className="block bg-red-500/10 font-mono text-red-400">{line}</span>
        if (line.startsWith('diff --git') || line.startsWith('index ')) return <span key={key} className="block font-mono text-zinc-400">{line}</span>
        return <span key={key} className="block font-mono text-zinc-300">{line}</span>
      })}
    </pre>
  )
}

function ComparisonResults({ comparison }: { comparison: Comparison }) {
  const { t } = useTranslation()
  const totalAdditions = comparison.files.reduce((sum, file) => sum + file.additions, 0)
  const totalDeletions = comparison.files.reduce((sum, file) => sum + file.deletions, 0)

  if (comparison.files.length === 0) {
    return <p role="status" className="border-y border-border py-6 text-center text-sm text-text-muted">{t('compare.noChanges')}</p>
  }

  return (
    <section className="space-y-4" aria-label={t('compare.filesChanged')}>
      <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 border-y border-border py-3 text-sm">
        <span className="text-text-muted">{t('compare.mergeBase')}:</span>
        <code className="break-all rounded bg-surface-raised px-1.5 py-0.5">{comparison.merge_base}</code>
        <span className="font-mono text-text-primary">+{totalAdditions}</span>
        <span className="font-mono text-text-primary">−{totalDeletions}</span>
      </div>
      <h2 className="text-sm font-semibold uppercase tracking-wide">{t('compare.filesChanged')} <span className="font-normal text-text-muted">({comparison.files.length})</span></h2>
      <ul className="divide-y divide-border border-y border-border">
        {comparison.files.map((file) => (
          <li key={file.path} className="flex min-w-0 flex-col gap-1 py-3 sm:flex-row sm:items-center sm:gap-3">
            <div className="flex min-w-0 flex-1 items-start gap-2">
              <FileDiff className="mt-0.5 h-4 w-4 shrink-0 text-text-muted" aria-hidden />
              <code className="min-w-0 break-all text-xs">{file.path}</code>
            </div>
            <div className="flex shrink-0 items-center gap-3 pl-6 text-xs sm:pl-0">
              <span className={`rounded px-2 py-0.5 font-medium ${changeStatusStyles[file.status]}`}>{t(`compare.status_${file.status}`)}</span>
              {file.binary ? (
                <span className="text-text-muted">{t('compare.binaryFile')}</span>
              ) : (
                <span className="font-mono text-text-primary">+{file.additions} −{file.deletions}</span>
              )}
            </div>
          </li>
        ))}
      </ul>
      {comparison.patch.trim() && (
        <div className="min-w-0 space-y-3">
          <h2 className="text-sm font-semibold uppercase tracking-wide">{t('compare.patch')}</h2>
          <PatchView patch={comparison.patch} />
        </div>
      )}
    </section>
  )
}

export function ComparePage() {
  const { t } = useTranslation()
  const { repo } = useParams<{ repo: string }>()
  const [searchParams, setSearchParams] = useSearchParams()
  const fromParam = searchParams.get('from')?.trim() ?? ''
  const toParam = searchParams.get('to')?.trim() ?? ''
  const [from, setFrom] = useState(fromParam)
  const [to, setTo] = useState(toParam)
  const sameRef = Boolean(from.trim() && from.trim() === to.trim())
  const canCompare = Boolean(fromParam && toParam && fromParam !== toParam)
  const refs = useRepositoryRefs(repo)
  const { data: comparison, isLoading, error, refetch } = useRepositoryComparison(repo, fromParam, canCompare ? toParam : '')

  useEffect(() => {
    setFrom(fromParam)
    setTo(toParam)
  }, [fromParam, toParam])

  if (!repo) return <p className="text-sm text-text-muted">{t('repositories.notFound')}</p>

  function applyComparison(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const base = from.trim()
    const head = to.trim()
    if (!base || !head || base === head) return
    setSearchParams(new URLSearchParams({ from: base, to: head }), { replace: true })
  }

  return (
    <div className="space-y-5">
      <div>
        <div className="flex min-w-0 flex-wrap items-center gap-2 text-sm text-text-muted">
          <Link to="/repositories" className="hover:text-text-primary">{t('navigation.repositories')}</Link>
          <ChevronRight className="h-3 w-3" aria-hidden />
          <Link to={`/repositories/${encodeURIComponent(repo)}`} className="break-all hover:text-text-primary">{repo}</Link>
          <ChevronRight className="h-3 w-3" aria-hidden />
          <span>{t('repositoryBrowser.compare')}</span>
        </div>
        <div className="mt-2 flex items-center gap-3">
          <GitCompareArrows className="h-6 w-6 text-accent" aria-hidden />
          <h1 className="text-2xl font-bold">{t('compare.title')}</h1>
        </div>
      </div>

      <form aria-label={t('compare.title')} onSubmit={applyComparison} className="grid gap-3 border-y border-border py-4 sm:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)_auto] sm:items-end">
        <div className="min-w-0 space-y-1.5">
          <Label htmlFor="compare-from">{t('compare.baseRef')}</Label>
          <Input id="compare-from" required list="compare-refs-from" className="min-h-10 font-mono" value={from} onChange={(event) => setFrom(event.target.value)} />
          <datalist id="compare-refs-from">{refs.data?.map((ref) => <option key={ref.name} value={ref.name} />)}</datalist>
        </div>
        <GitCompareArrows className="mb-3 hidden h-4 w-4 text-text-muted sm:block" aria-hidden />
        <div className="min-w-0 space-y-1.5">
          <Label htmlFor="compare-to">{t('compare.headRef')}</Label>
          <Input id="compare-to" required list="compare-refs-to" className="min-h-10 font-mono" value={to} onChange={(event) => setTo(event.target.value)} />
          <datalist id="compare-refs-to">{refs.data?.map((ref) => <option key={ref.name} value={ref.name} />)}</datalist>
        </div>
        <Button type="submit" className="min-h-10" disabled={sameRef}>{t('compare.compareAction')}</Button>
        {sameRef && <p role="alert" className="border-l-2 border-danger pl-2 text-sm text-text-primary sm:col-span-4">{t('compare.refsMustDiffer')}</p>}
        {refs.isLoading && <p role="status" className="text-xs text-text-muted sm:col-span-4">{t('compare.loadingRefs')}</p>}
        {Boolean(refs.error) && (
          <div role="alert" className="flex flex-wrap items-center gap-2 text-xs text-text-secondary sm:col-span-4">
            <span>{t('compare.refsUnavailable')}</span>
            <Button type="button" size="sm" variant="outline" className="min-h-10 sm:min-h-10" onClick={() => refs.refetch()}>{t('common.retry')}</Button>
          </div>
        )}
      </form>

      {!canCompare ? (
        <p role="status" className="py-4 text-sm text-text-muted">{fromParam && toParam ? t('compare.refsMustDiffer') : t('compare.chooseRefs')}</p>
      ) : (
        <QueryState data={comparison} isLoading={isLoading} error={error} onRetry={() => refetch()}>
          {(result) => <ComparisonResults comparison={result} />}
        </QueryState>
      )}
    </div>
  )
}
