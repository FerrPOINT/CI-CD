import { useTranslation } from 'react-i18next'
import { QueryState } from '@/shared/ui/query-state'
import { useParams } from 'react-router'
import { useArtifacts } from '@/api/hooks'
import { downloadArtifact, saveDownloadedArtifact } from '@/api/client'
import { Card } from '@sdlc/ui/ui'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@sdlc/ui/ui'
import { Package } from 'lucide-react'
import { toast } from 'sonner'

export function ArtifactsPage() {
  const { t } = useTranslation()
  const { jobId } = useParams()
  const { data: artifacts = [], isLoading, error: listError } = useArtifacts(jobId)

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-2">
        <Package className="h-5 w-5 text-accent" />
        <h1 className="text-2xl font-bold">{t('artifacts.title')}</h1>
      </div>

      <QueryState data={artifacts} isLoading={isLoading} error={listError} isEmpty={(list) => list.length === 0} empty={{ title: t('artifacts.empty') }}>
        {() => (

        <Card>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t('artifacts.name')}</TableHead>
                <TableHead>{t('artifacts.size')}</TableHead>
                <TableHead>{t('artifacts.type')}</TableHead>
                <TableHead>{t('artifacts.digest')}</TableHead>
                <TableHead>{t('artifacts.created')}</TableHead>
                <TableHead>{t('artifacts.expires')}</TableHead>
                <TableHead className="w-24"><span className="sr-only">{t('common.actions')}</span></TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {artifacts.map(a => {
                const state = artifactState(a)
                return (
                  <TableRow key={a.id}>
                    <TableCell className="font-medium">{a.name}</TableCell>
                    <TableCell className="text-xs text-text-muted">{formatBytes(a.size_bytes)}</TableCell>
                    <TableCell className="text-xs text-text-muted">{a.content_type}</TableCell>
                    <TableCell className="font-mono text-xs text-text-muted" title={a.sha256 ?? undefined}>{a.sha256 ? a.sha256.slice(0, 12) : 'n/a'}</TableCell>
                    <TableCell className="text-xs text-text-muted">{new Date(a.created_at).toLocaleString()}</TableCell>
                    <TableCell className="text-xs text-text-muted">{new Date(a.expires_at).toLocaleString()}</TableCell>
                    <TableCell>
                      {state === 'available' ? (
                        <button
                          type="button"
                          onClick={() => {
                            void downloadArtifact(a.id)
                              .then(saveDownloadedArtifact)
                              .catch(() => toast.error(t('artifacts.downloadFailed')))
                          }}
                          className="text-xs text-accent hover:underline"
                        >
                          {t('artifacts.download')}
                        </button>
                      ) : (
                        <span className="text-xs text-text-muted">{t(`artifacts.${state}`)}</span>
                      )}
                    </TableCell>
                  </TableRow>
                )
              })}
            </TableBody>
          </Table>
        </Card>
      
        )}
      </QueryState>
    </div>
  )
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`
}

function artifactState(artifact: { expires_at: string; purged_at: string | null }): 'available' | 'expired' | 'purged' {
  if (artifact.purged_at) return 'purged'
  if (new Date(artifact.expires_at).getTime() <= Date.now()) return 'expired'
  return 'available'
}
