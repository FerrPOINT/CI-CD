import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { useProjects, useCreateProject, useUpdateProject, useDeleteProject } from '@/api/hooks'
import { Card } from '@/shared/ui/card'
import { Button } from '@/shared/ui/button'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'
import { FolderGit2, Plus, ChevronRight, Pencil, Trash2, GitFork, KeyRound, Globe, Clock, Webhook, BarChart3 } from 'lucide-react'
import { toast } from 'sonner'
import type { Project } from '@/api/types'

export function ProjectsPage() {
  const { t } = useTranslation()
  const { data: projects = [], isLoading } = useProjects()
  const createProject = useCreateProject()
  const updateProject = useUpdateProject()
  const deleteProject = useDeleteProject()
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState({ name: '', repository_url: '', default_branch: 'main' })
  const [editing, setEditing] = useState<Project | null>(null)

  function handleDelete(project: Project) {
    if (!window.confirm(`${t('projects.deleteConfirm')} "${project.name}"?`)) return
    deleteProject.mutate(project.id, {
      onSuccess: () => toast.success(t('projects.deleted')),
      onError: (err) => toast.error(err.message),
    })
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    createProject.mutate(form, {
      onSuccess: () => { setShowForm(false); setForm({ name: '', repository_url: '', default_branch: 'main' }); toast.success('Project created') },
      onError: (err) => toast.error(err.message),
    })
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">{t('projects.title')}</h1>
        <Button size="sm" onClick={() => setShowForm(v => !v)}>
          <Plus className="h-4 w-4" />
          {t('projects.create')}
        </Button>
      </div>

      {showForm && (
        <Card className="p-4">
          <form onSubmit={handleSubmit} className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            <div className="space-y-1.5">
              <Label htmlFor="name">{t('projects.name')}</Label>
              <Input id="name" required value={form.name} onChange={e => setForm({ ...form, name: e.target.value })} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="repo">{t('projects.repositoryUrl')}</Label>
              <Input id="repo" required placeholder="git@github.com:org/repo.git" value={form.repository_url} onChange={e => setForm({ ...form, repository_url: e.target.value })} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="branch">{t('projects.defaultBranch')}</Label>
              <Input id="branch" required value={form.default_branch} onChange={e => setForm({ ...form, default_branch: e.target.value })} />
            </div>
            <div className="sm:col-span-2 lg:col-span-3 flex gap-2">
              <Button type="submit" disabled={createProject.isPending}>{t('projects.create')}</Button>
              <Button type="button" variant="ghost" onClick={() => setShowForm(false)}>{t('common.cancel')}</Button>
            </div>
          </form>
        </Card>
      )}

      {editing && (
        <Card className="p-4">
          <form
            onSubmit={(e) => {
              e.preventDefault()
              updateProject.mutate(
                { id: editing.id, name: editing.name, repository_url: editing.repository_url, default_branch: editing.default_branch },
                {
                  onSuccess: () => { setEditing(null); toast.success(t('projects.updated')) },
                  onError: (err) => toast.error(err.message),
                },
              )
            }}
            className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3"
          >
            <div className="space-y-1.5">
              <Label htmlFor="edit-name">{t('projects.name')}</Label>
              <Input id="edit-name" required value={editing.name} onChange={(e) => setEditing({ ...editing, name: e.target.value })} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="edit-repo">{t('projects.repositoryUrl')}</Label>
              <Input id="edit-repo" required value={editing.repository_url} onChange={(e) => setEditing({ ...editing, repository_url: e.target.value })} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="edit-branch">{t('projects.defaultBranch')}</Label>
              <Input id="edit-branch" required value={editing.default_branch} onChange={(e) => setEditing({ ...editing, default_branch: e.target.value })} />
            </div>
            <div className="flex gap-2 sm:col-span-2 lg:col-span-3">
              <Button type="submit" disabled={updateProject.isPending}>{t('common.save')}</Button>
              <Button type="button" variant="ghost" onClick={() => setEditing(null)}>{t('common.cancel')}</Button>
            </div>
          </form>
        </Card>
      )}

      {isLoading ? (
        <p className="text-sm text-text-muted">{t('common.loading')}</p>
      ) : projects.length === 0 ? (
        <Card className="p-8 text-center"><p className="text-text-muted">{t('projects.empty')}</p></Card>
      ) : (
        <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {projects.map(p => (
            <Card key={p.id} className="group p-4 transition-colors hover:border-accent">
              <Link to={`/projects/${p.id}/pipelines`}>
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <FolderGit2 className="h-4 w-4 text-accent" />
                    <span className="font-medium">{p.name}</span>
                  </div>
                  <ChevronRight className="h-4 w-4 text-text-muted" />
                </div>
                <p className="mt-2 truncate text-xs text-text-muted">{p.repository_url}</p>
                <div className="mt-2 flex items-center gap-2 text-xs text-text-muted">
                  <code className="rounded bg-surface-raised px-1.5 py-0.5">{p.default_branch}</code>
                </div>
              </Link>
              <div className="mt-3 flex gap-1 opacity-0 transition-opacity group-hover:opacity-100 flex-wrap">
                <Button asChild size="sm" variant="ghost" className="h-7 gap-1 px-2 text-xs">
                  <Link to={`/repositories?project=${encodeURIComponent(p.name)}`}>
                    <GitFork className="h-3 w-3" /> {t('projects.repositories')}
                  </Link>
                </Button>
                <Button asChild size="sm" variant="ghost" className="h-7 gap-1 px-2 text-xs">
                  <Link to={`/projects/${p.id}/secrets`}>
                    <KeyRound className="h-3 w-3" /> {t('secrets.title')}
                  </Link>
                </Button>
                <Button asChild size="sm" variant="ghost" className="h-7 gap-1 px-2 text-xs">
                  <Link to={`/projects/${p.id}/environments`}>
                    <Globe className="h-3 w-3" /> {t('environments.title')}
                  </Link>
                </Button>
                <Button asChild size="sm" variant="ghost" className="h-7 gap-1 px-2 text-xs">
                  <Link to={`/projects/${p.id}/schedules`}>
                    <Clock className="h-3 w-3" /> {t('schedules.title')}
                  </Link>
                </Button>
                <Button asChild size="sm" variant="ghost" className="h-7 gap-1 px-2 text-xs">
                  <Link to={`/projects/${p.id}/webhooks`}>
                    <Webhook className="h-3 w-3" /> {t('webhooks.title')}
                  </Link>
                </Button>
                <Button asChild size="sm" variant="ghost" className="h-7 gap-1 px-2 text-xs">
                  <Link to={`/projects/${p.id}/reports`}>
                    <BarChart3 className="h-3 w-3" /> {t('reports.title')}
                  </Link>
                </Button>
                <Button size="sm" variant="ghost" className="h-7 gap-1 px-2 text-xs" onClick={() => setEditing(p)}>
                  <Pencil className="h-3 w-3" /> {t('projects.edit')}
                </Button>
                <Button size="sm" variant="ghost" className="h-7 gap-1 px-2 text-xs text-danger hover:text-danger" onClick={() => handleDelete(p)}>
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
