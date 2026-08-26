import { useState } from 'react'
import { Link, useParams, useSearchParams } from 'react-router'
import { useTranslation } from 'react-i18next'
import { ChevronRight, FileDiff, GitCompareArrows } from 'lucide-react'
import { useRepositoryComparison } from '@/api/hooks'
import { Button } from '@/shared/ui/button'
import { Card } from '@/shared/ui/card'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/shared/ui/table'
import type { ChangeStatus } from '@/api/types'

const changeStatusStyles: Record<ChangeStatus, string> = {
  added: 'bg-success/15 text-success',
  modified: 'bg-warning/15 text-warning',
  deleted: 'bg-danger/15 text-danger',
}

function PatchView({ patch }: { patch: string }) {
  return (
    <pre className="max-h-[32rem] overflow-auto rounded-md bg-zinc-950 p-4 text-xs leading-relaxed">
      {patch.split('\n').map((line, index) => {
        const key = `${index}-${line}`
        if (line.startsWith('+++') || line.startsWith('---')) {
          return <span key={key} className="block font-mono text-indigo-300">{line}</span>
        }
        if (line.startsWith('@@')) {
          return <span key={key} className="block font-mono text-sky-400">{line}</span>
        }
        if (line.startsWith('+')) {
          return <span key={key} className="block bg-green-500/10 font-mono text-green-400">{line}</span>
        }
        if (line.startsWith('-')) {
          return <span key={key} className="block bg-red-500/10 font-mono text-red-400">{line}</span>
        }
        if (line.startsWith('diff --git') || line.startsWith('index ')) {
          return <span key={key} className="block font-mono text-zinc-500">{line}</span>
        }
        return <span key={key} className="block font-mono text-zinc-300">{line}</span>
      })}
    </pre>
  )
}

export function ComparePage() {
  const { t } = useTranslation()
  const { repo } = useParams<{ repo: string }>()
  const [searchParams, setSearchParams] = useSearchParams()
  const fromParam = searchParams.get('from') ?? 'main'
  const toParam = searchParams.get('to') ?? 'feature/login'
  const [from, setFrom] = useState(fromParam)
  const [to, setTo] = useState(toParam)

  const { data: comparison, isLoading, isError, error } = useRepositoryComparison(repo, fromParam, toParam)

  if (!repo) return <p className="text-sm text-text-muted">{t('repositories.notFound')}</p>

  function applyComparison(e: React.FormEvent) {
    e.preventDefault()
    const params = new URLSearchParams()
    if (from.trim()) params.set('from', from.trim())
    if (to.trim()) params.set('to', to.trim())
    setSearchParams(params, { replace: true })
  }

  const totalAdditions = comparison?.files.reduce((sum, file) => sum + file.additions, 0) ?? 0
  const totalDeletions = comparison?.files.reduce((sum, file) => sum + file.deletions, 0) ?? 0

  return (
    <div className="space-y-6">
      <div>
        <div className="flex items-center gap-2 text-sm text-text-muted">
          <Link to="/repositories" className="hover:text-text-primary">{t('navigation.repositories')}</Link>
          <ChevronRight className="h-3 w-3" />
          <Link to={`/repositories/${encodeURIComponent(repo)}`} className="hover:text-text-primary">{repo}</Link>
          <ChevronRight className="h-3 w-3" />
          <span>{t('repositoryBrowser.compare')}</span>
        </div>
        <div className="mt-2 flex items-center gap-3">
          <GitCompareArrows className="h-6 w-6 text-accent" />
          <h1 className="text-2xl font-bold">{t('compare.title')}</h1>
        </div>
      </div>

      <Card className="p-4">
        <form onSubmit={applyComparison} className="grid gap-3 sm:grid-cols-[1fr_auto_1fr_auto] sm:items-end">
          <div className="space-y-1.5">
            <Label htmlFor="compare-from">{t('compare.sourceBranch')}</Label>
            <Input
              id="compare-from"
              required
              className="font-mono"
              placeholder="main"
              value={from}
              onChange={(e) => setFrom(e.target.value)}
            />
          </div>
          <GitCompareArrows className="mb-2 hidden h-4 w-4 text-text-muted sm:block" />
          <div className="space-y-1.5">
            <Label htmlFor="compare-to">{t('compare.targetBranch')}</Label>
            <Input
              id="compare-to"
              required
              className="font-mono"
              placeholder="feature/login"
              value={to}
              onChange={(e) => setTo(e.target.value)}
            />
          </div>
          <Button type="submit" className="mb-0.5">{t('compare.compareAction')}</Button>
        </form>
        <p className="mt-3 text-xs text-text-muted">
          {t('compare.mergeBase')}: <code className="rounded bg-surface-raised px-1.5 py-0.5">{comparison?.merge_base ?? '—'}</code>
        </p>
      </Card>

      {isLoading ? (
        <p className="text-sm text-text-muted">{t('common.loading')}</p>
      ) : isError ? (
        <Card className="p-6 text-sm text-danger">
          {t('common.error')}: {error instanceof Error ? error.message : String(error)}
        </Card>
      ) : !comparison || comparison.files.length === 0 ? (
        <Card className="p-8 text-center text-text-muted">{t('compare.noChanges')}</Card>
      ) : (
        <>
          <div className="flex flex-wrap items-center justify-between gap-2">
            <h2 className="text-sm font-semibold uppercase tracking-wide">{t('compare.filesChanged')}</h2>
            <div className="flex items-center gap-3 font-mono text-xs">
              <span className="text-success">+{totalAdditions}</span>
              <span className="text-danger">−{totalDeletions}</span>
            </div>
          </div>
          <Card className="overflow-hidden">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t('compare.path')}</TableHead>
                  <TableHead>{t('compare.status')}</TableHead>
                  <TableHead className="text-right">+</TableHead>
                  <TableHead className="text-right">−</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {comparison.files.map((file) => (
                  <TableRow key={file.path}>
                    <TableCell className="max-w-96 truncate font-mono text-xs"><FileDiff className="mr-2 inline h-4 w-4 text-text-muted" />{file.path}</TableCell>
                    <TableCell>
                      <span className={`rounded px-2 py-0.5 text-xs font-medium ${changeStatusStyles[file.status]}`}>
                        {t(`compare.status_${file.status}`)}
                      </span>
                    </TableCell>
                    <TableCell className="text-right font-mono text-xs text-success">+{file.additions}</TableCell>
                    <TableCell className="text-right font-mono text-xs text-danger">−{file.deletions}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </Card>

          {comparison.patch.trim().length > 0 && (
            <>
              <h2 className="text-sm font-semibold uppercase tracking-wide">{t('compare.patch')}</h2>
              <Card className="p-0">
                <PatchView patch={comparison.patch} />
              </Card>
            </>
          )}
        </>
      )}
    </div>
  )
}
