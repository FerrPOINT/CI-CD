import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'
import { Copy, Download, Loader2, Package, Search } from 'lucide-react'
import { toast } from 'sonner'
import { downloadArtifact, saveDownloadedArtifact } from '@/api/client'
import { useArtifacts } from '@/api/hooks'
import type { Artifact } from '@/api/types'
import { Button, Input } from '@sdlc/ui/ui'
import { QueryState } from '@/shared/ui/query-state'

const pageSize = 20
type ArtifactStatus = 'available' | 'expired' | 'purged'
type StatusFilter = 'all' | ArtifactStatus

export function ArtifactsPage() {
  const { t } = useTranslation()
  const { jobId } = useParams()
  const { data: artifacts = [], isLoading, error: listError, refetch } = useArtifacts(jobId)
  const [search, setSearch] = useState('')
  const [status, setStatus] = useState<StatusFilter>('all')
  const [page, setPage] = useState(1)
  const [downloadingId, setDownloadingId] = useState<string | null>(null)
  const [actionError, setActionError] = useState<{ id: string; message: string } | null>(null)

  const normalizedSearch = search.trim().toLocaleLowerCase()
  const filtered = artifacts.filter((artifact) =>
    (status === 'all' || artifactState(artifact) === status) &&
    (artifact.name.toLocaleLowerCase().includes(normalizedSearch) ||
      artifact.content_type.toLocaleLowerCase().includes(normalizedSearch)),
  )
  const totalPages = Math.max(1, Math.ceil(filtered.length / pageSize))
  const currentPage = Math.min(page, totalPages)
  const visible = filtered.slice((currentPage - 1) * pageSize, currentPage * pageSize)

  async function handleDownload(artifact: Artifact) {
    if (downloadingId) return
    setActionError(null)
    setDownloadingId(artifact.id)
    try {
      saveDownloadedArtifact(await downloadArtifact(artifact.id))
    } catch {
      setActionError({ id: artifact.id, message: t('artifacts.downloadFailed') })
      void refetch()
    } finally {
      setDownloadingId(null)
    }
  }

  async function handleCopy(artifact: Artifact) {
    if (!artifact.sha256) return
    setActionError(null)
    try {
      await navigator.clipboard.writeText(artifact.sha256)
      toast.success(t('artifacts.copied'))
    } catch {
      setActionError({ id: artifact.id, message: t('artifacts.copyFailed') })
    }
  }

  return (
    <div className="min-w-0 max-w-5xl space-y-5">
      <header className="flex items-center gap-2">
        <Package className="h-5 w-5 text-accent" aria-hidden />
        <h1 className="text-xl font-bold sm:text-2xl">{t('artifacts.title')}</h1>
      </header>

      <QueryState
        data={artifacts}
        isLoading={isLoading}
        error={listError}
        errorMessage={t('artifacts.loadError')}
        isEmpty={(list) => list.length === 0}
        empty={{ title: t('artifacts.empty') }}
        onRetry={() => void refetch()}
      >
        {() => (
          <section aria-label={t('artifacts.title')} className="space-y-3">
            <div className="flex flex-wrap gap-2">
              <div className="relative min-w-0 flex-1 sm:max-w-xl">
                <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted" aria-hidden />
                <Input
                  type="search"
                  aria-label={t('artifacts.search')}
                  placeholder={t('artifacts.search')}
                  className="min-h-10 pl-9"
                  value={search}
                  onChange={(event) => { setSearch(event.target.value); setPage(1) }}
                />
              </div>
              <select
                aria-label={t('artifacts.status')}
                className="min-h-10 rounded-md border border-border-strong bg-surface px-3 text-sm text-text-primary"
                value={status}
                onChange={(event) => { setStatus(event.target.value as StatusFilter); setPage(1) }}
              >
                <option value="all">{t('artifacts.all')}</option>
                <option value="available">{t('artifacts.available')}</option>
                <option value="expired">{t('artifacts.expired')}</option>
                <option value="purged">{t('artifacts.purged')}</option>
              </select>
            </div>
            <p className="text-xs text-text-muted">{t('artifacts.shown', { count: visible.length, total: filtered.length })}</p>
            {filtered.length === 0 ? (
              <p role="status" className="border-y border-border py-6 text-sm text-text-muted">{t('artifacts.noMatches')}</p>
            ) : (
              <ul className="divide-y divide-border border-y border-border">
                {visible.map((artifact) => {
                  const state = artifactState(artifact)
                  return (
                    <li key={artifact.id} className="min-w-0 py-2">
                      <div className="flex min-w-0 items-start gap-2">
                        <Package className="mt-1 h-4 w-4 shrink-0 text-accent" aria-hidden />
                        <div className="min-w-0 flex-1">
                          <p className="break-all text-sm font-medium">{artifact.name}</p>
                          <p className="break-all text-xs text-text-secondary">
                            {formatBytes(artifact.size_bytes)} / {artifact.content_type}
                          </p>
                          {state !== 'available' && (
                            <p className="text-xs text-text-muted">{t('artifacts.' + state)}</p>
                          )}
                        </div>
                        {artifact.sha256 && (
                          <button
                            type="button"
                            aria-label={t('artifacts.copyDigestFor', { name: artifact.name })}
                            title={t('artifacts.copyDigestFor', { name: artifact.name })}
                            className="inline-flex h-10 w-10 shrink-0 items-center justify-center rounded-md text-text-secondary hover:bg-surface-raised hover:text-text-primary focus-visible:outline-2 focus-visible:outline-accent"
                            onClick={() => void handleCopy(artifact)}
                          >
                            <Copy className="h-4 w-4" aria-hidden />
                          </button>
                        )}
                        {state === 'available' && (
                          <button
                            type="button"
                            aria-label={downloadingId === artifact.id
                              ? t('artifacts.downloading', { name: artifact.name })
                              : t('artifacts.downloadFor', { name: artifact.name })}
                            title={t('artifacts.downloadFor', { name: artifact.name })}
                            aria-busy={downloadingId === artifact.id}
                            disabled={downloadingId !== null}
                            className="inline-flex h-10 w-10 shrink-0 items-center justify-center rounded-md text-accent hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
                            onClick={() => void handleDownload(artifact)}
                          >
                            {downloadingId === artifact.id
                              ? <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
                              : <Download className="h-4 w-4" aria-hidden />}
                          </button>
                        )}
                      </div>
                      <div className="ml-6 flex flex-wrap gap-x-4 gap-y-1 text-xs text-text-muted">
                        <time dateTime={artifact.created_at} title={new Date(artifact.created_at).toLocaleString()}>
                          {t('artifacts.created')}: {formatDate(artifact.created_at)}
                        </time>
                        <time dateTime={artifact.expires_at} title={new Date(artifact.expires_at).toLocaleString()}>
                          {t('artifacts.expires')}: {formatDate(artifact.expires_at)}
                        </time>
                      </div>
                      {artifact.sha256 && (
                        <p className="ml-6 min-w-0 text-xs text-text-muted">
                          SHA-256: <code title={artifact.sha256} className="font-mono">{artifact.sha256.slice(0, 12)}...</code>
                        </p>
                      )}
                      {actionError?.id === artifact.id && (
                        <p role="alert" className="ml-6 break-words text-xs text-danger-strong">{actionError.message}</p>
                      )}
                    </li>
                  )
                })}
              </ul>
            )}
            {filtered.length > pageSize && (
              <nav aria-label={t('artifacts.pages')} className="flex items-center justify-end gap-2">
                <Button type="button" size="sm" variant="outline" className="min-h-10 sm:min-h-10" disabled={currentPage === 1} onClick={() => setPage(currentPage - 1)}>
                  {t('artifacts.previous')}
                </Button>
                <span className="text-sm text-text-muted">{currentPage} / {totalPages}</span>
                <Button type="button" size="sm" variant="outline" className="min-h-10 sm:min-h-10" disabled={currentPage === totalPages} onClick={() => setPage(currentPage + 1)}>
                  {t('artifacts.next')}
                </Button>
              </nav>
            )}
          </section>
        )}
      </QueryState>
    </div>
  )
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return bytes + ' B'
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KiB'
  return (bytes / (1024 * 1024)).toFixed(1) + ' MiB'
}

function formatDate(value: string): string {
  return new Date(value).toLocaleString(undefined, { dateStyle: 'short', timeStyle: 'short' })
}

function artifactState(artifact: Artifact): ArtifactStatus {
  if (artifact.purged_at) return 'purged'
  return new Date(artifact.expires_at).getTime() <= Date.now() ? 'expired' : 'available'
}
