import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'
import { useArtifacts } from '@/api/hooks'
import { Card } from '@/shared/ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/ui/table'
import { Package } from 'lucide-react'

export function ArtifactsPage() {
  const { t } = useTranslation()
  const { jobId } = useParams()
  const { data: artifacts = [], isLoading } = useArtifacts(jobId)

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-2">
        <Package className="h-5 w-5 text-accent" />
        <h1 className="text-2xl font-bold">{t('artifacts.title')}</h1>
      </div>

      {isLoading ? (
        <p className="text-sm text-text-muted">{t('common.loading')}</p>
      ) : artifacts.length === 0 ? (
        <Card className="p-8 text-center"><p className="text-text-muted">{t('artifacts.empty')}</p></Card>
      ) : (
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
                <TableHead className="w-24"></TableHead>
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
                        <a
                          href={`/api/v1/artifacts/${a.id}/download`}
                          className="text-xs text-accent hover:underline"
                        >
                          {t('artifacts.download')}
                        </a>
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
