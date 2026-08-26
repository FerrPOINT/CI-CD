import { useState } from 'react'
import { Link } from 'react-router'
import { useTranslation } from 'react-i18next'
import { useRepositories, useCreateRepository, useDeleteRepository } from '@/api/hooks'
import { Card } from '@/shared/ui/card'
import { Button } from '@/shared/ui/button'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'
import { GitFork, Plus, Trash2, Copy, Check, FolderOpen } from 'lucide-react'
import { toast } from 'sonner'
import type { Repository } from '@/api/types'

function buildCloneUrl(name: string): string {
  return `${window.location.origin}/git/${name}.git`
}

export function RepositoriesPage() {
  const { t } = useTranslation()
  const { data: repositories = [], isLoading } = useRepositories()
  const createRepository = useCreateRepository()
  const deleteRepository = useDeleteRepository()
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState({ name: '' })
  const [copied, setCopied] = useState<string | null>(null)

  function handleDelete(repo: Repository) {
    if (!window.confirm(`${t('repositories.deleteConfirm')} "${repo.name}"?`)) return
    deleteRepository.mutate(repo.name, {
      onSuccess: () => toast.success(t('repositories.deleted')),
      onError: (err) => toast.error(err.message),
    })
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    createRepository.mutate(form, {
      onSuccess: () => { setShowForm(false); setForm({ name: '' }); toast.success('Repository created') },
      onError: (err) => toast.error(err.message),
    })
  }

  function copyUrl(name: string) {
    const url = buildCloneUrl(name)
    navigator.clipboard.writeText(url).then(() => {
      setCopied(name)
      setTimeout(() => setCopied(null), 2000)
      toast.success('Clone URL copied')
    })
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">{t('repositories.title')}</h1>
        <Button size="sm" onClick={() => setShowForm(v => !v)}>
          <Plus className="h-4 w-4" />
          {t('repositories.create')}
        </Button>
      </div>

      {showForm && (
        <Card className="p-4">
          <form onSubmit={handleSubmit} className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            <div className="space-y-1.5">
              <Label htmlFor="repo-name">{t('repositories.name')}</Label>
              <Input id="repo-name" required placeholder="my-repo" value={form.name} onChange={e => setForm({ ...form, name: e.target.value })} />
            </div>
            <div className="sm:col-span-2 lg:col-span-3 flex gap-2">
              <Button type="submit" disabled={createRepository.isPending}>{t('repositories.create')}</Button>
              <Button type="button" variant="ghost" onClick={() => setShowForm(false)}>{t('common.cancel')}</Button>
            </div>
          </form>
        </Card>
      )}

      {isLoading ? (
        <p className="text-sm text-text-muted">{t('common.loading')}</p>
      ) : repositories.length === 0 ? (
        <Card className="p-8 text-center"><p className="text-text-muted">{t('repositories.empty')}</p></Card>
      ) : (
        <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {repositories.map(r => (
            <Card key={r.id} className="group p-4 transition-colors hover:border-accent">
              <div className="flex items-center gap-2">
                <GitFork className="h-4 w-4 text-accent" />
                <span className="font-medium">{r.name}</span>
              </div>
              <p className="mt-2 text-xs text-text-muted">
                {new Date(r.created_at).toLocaleString()}
              </p>
              <div className="mt-2 flex items-center gap-1">
                <code className="flex-1 truncate rounded bg-surface-raised px-1.5 py-0.5 text-xs">
                  {buildCloneUrl(r.name)}
                </code>
                <Button size="sm" variant="ghost" className="h-7 w-7 p-0" onClick={() => copyUrl(r.name)}>
                  {copied === r.name ? <Check className="h-3 w-3 text-success" /> : <Copy className="h-3 w-3" />}
                </Button>
              </div>
              <div className="mt-3 flex gap-1 opacity-0 transition-opacity group-hover:opacity-100">
                <Button asChild size="sm" variant="ghost" className="h-7 gap-1 px-2 text-xs">
                  <Link to={`/repositories/${encodeURIComponent(r.name)}`}>
                    <FolderOpen className="h-3 w-3" /> {t('repositories.open')}
                  </Link>
                </Button>
                <Button size="sm" variant="ghost" className="h-7 gap-1 px-2 text-xs text-danger hover:text-danger" onClick={() => handleDelete(r)}>
                  <Trash2 className="h-3 w-3" /> {t('common.delete')}
                </Button>
              </div>
            </Card>
          ))}
        </div>
      )}
    </div>
  )
}