import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'
import { useSecrets, useUpsertSecret, useDeleteSecret } from '@/api/hooks'
import { Card } from '@sdlc/ui/ui'
import { Button } from '@sdlc/ui/ui'
import { Input } from '@sdlc/ui/ui'
import { Label } from '@sdlc/ui/ui'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@sdlc/ui/ui'
import { KeyRound, Plus, Trash2, ShieldCheck } from 'lucide-react'
import { toast } from 'sonner'
import { ConfirmDialog } from '@/shared/ui/confirm-dialog'
import type { SecretMetadata } from '@/api/types'

export function SecretsPage() {
  const { t } = useTranslation()
  const { projectId } = useParams()
  const { data: secrets = [], isLoading } = useSecrets(projectId)
  const upsert = useUpsertSecret(projectId)
  const del = useDeleteSecret()
  const [showForm, setShowForm] = useState(false)
  const [pendingDelete, setPendingDelete] = useState<SecretMetadata | null>(null)
  const [form, setForm] = useState({ key: '', value: '' })

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    upsert.mutate(form, {
      onSuccess: () => { setShowForm(false); setForm({ key: '', value: '' }); toast.success(t('secrets.saved')) },
      onError: (err) => toast.error(err.message),
    })
  }

  function handleDelete(s: SecretMetadata) {
    setPendingDelete(s) // confirm via ConfirmDialog, no direct mutate
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <KeyRound className="h-5 w-5 text-accent" />
          <h1 className="text-2xl font-bold">{t('secrets.title')}</h1>
        </div>
        <Button size="sm" onClick={() => setShowForm(v => !v)}>
          <Plus className="h-4 w-4" />
          {t('secrets.add')}
        </Button>
      </div>

      <Card className="border-amber-500/30 bg-amber-500/5 p-3">
        <div className="flex items-start gap-2 text-xs text-amber-600 dark:text-amber-400">
          <ShieldCheck className="h-4 w-4 shrink-0" />
          <span>{t('secrets.encryptedNotice')}</span>
        </div>
      </Card>

      {showForm && (
        <Card className="p-4">
          <form onSubmit={handleSubmit} className="grid gap-3">
            <div className="space-y-1.5">
              <Label htmlFor="secret-key">{t('secrets.key')}</Label>
              <Input id="secret-key" required placeholder="DATABASE_PASSWORD" value={form.key} onChange={e => setForm({ ...form, key: e.target.value })} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="secret-value">{t('secrets.value')}</Label>
              <Input id="secret-value" type="password" required value={form.value} onChange={e => setForm({ ...form, value: e.target.value })} />
            </div>
            <div className="flex gap-2">
              <Button type="submit" disabled={upsert.isPending}>{t('secrets.save')}</Button>
              <Button type="button" variant="ghost" onClick={() => setShowForm(false)}>{t('common.cancel')}</Button>
            </div>
          </form>
        </Card>
      )}

      {isLoading ? (
        <p className="text-sm text-text-muted">{t('common.loading')}</p>
      ) : secrets.length === 0 ? (
        <Card className="p-8 text-center"><p className="text-text-muted">{t('secrets.empty')}</p></Card>
      ) : (
        <Card>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t('secrets.key')}</TableHead>
                <TableHead>{t('secrets.updated')}</TableHead>
                <TableHead className="w-20"><span className="sr-only">{t('common.actions')}</span></TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {secrets.map(s => (
                <TableRow key={s.id}>
                  <TableCell className="font-mono text-sm">{s.key}</TableCell>
                  <TableCell className="text-xs text-text-muted">{new Date(s.updated_at).toLocaleString()}</TableCell>
                  <TableCell>
                    <Button size="sm" variant="ghost" className="h-7 gap-1 px-2 text-xs text-danger hover:text-danger" onClick={() => handleDelete(s)}>
                      <Trash2 className="h-3 w-3" /> {t('common.delete')}
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </Card>
      )}
      <ConfirmDialog
        open={pendingDelete !== null}
        title={pendingDelete ? `${t('secrets.deleteConfirm')} "${pendingDelete.key}"?` : ''}
        onCancel={() => setPendingDelete(null)}
        onConfirm={() => {
          if (pendingDelete) del.mutate(pendingDelete.id, { onSuccess: () => toast.success(t('secrets.deleted')), onError: (err) => toast.error(err.message) })
          setPendingDelete(null)
        }}
      />
    </div>
  )
}
