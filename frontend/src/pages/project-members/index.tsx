import { useEffect, useState } from 'react'
import { Link, useParams } from 'react-router'
import { useTranslation } from 'react-i18next'
import {
  useDeleteProjectMembership,
  useProjectMemberships,
  useProjects,
  useUpsertProjectMembership,
  useUsers,
} from '@/api/hooks'
import type { ProjectMembership, ProjectRole } from '@/api/types'
import { Button } from '@/shared/ui/button'
import { Card } from '@/shared/ui/card'
import { ConfirmDialog } from '@/shared/ui/confirm-dialog'
import { Label } from '@/shared/ui/label'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/ui/table'
import { UserAvatar } from '@/shared/ui/user-avatar'
import { Trash2, UserPlus, Users } from 'lucide-react'
import { toast } from 'sonner'

const PROJECT_ROLES: ProjectRole[] = ['maintainer', 'developer', 'viewer']

export function ProjectMembersPage() {
  const { t } = useTranslation()
  const { projectId } = useParams<{ projectId: string }>()
  const { data: projects = [] } = useProjects()
  const project = projects.find(p => p.id === projectId)
  const { data: users = [] } = useUsers()
  const { data: memberships = [], isLoading } = useProjectMemberships(projectId)
  const upsert = useUpsertProjectMembership(projectId)
  const remove = useDeleteProjectMembership(projectId)
  const [pendingDelete, setPendingDelete] = useState<ProjectMembership | null>(null)
  const [form, setForm] = useState<{ user_id: string; role: ProjectRole }>({ user_id: '', role: 'viewer' })

  useEffect(() => {
    if (!form.user_id && users.length > 0) {
      setForm(current => ({ ...current, user_id: users[0].id }))
    }
  }, [form.user_id, users])

  function handleSubmit(event: React.FormEvent) {
    event.preventDefault()
    if (!form.user_id) return
    upsert.mutate(form, {
      onSuccess: () => toast.success(t('projectMembers.saved')),
      onError: (err) => toast.error(err.message),
    })
  }

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-sm text-text-muted">
            <Link to="/projects" className="hover:text-text-primary">{t('navigation.projects')}</Link>
            <span>/</span>
            <span className="truncate">{project?.name ?? projectId}</span>
          </div>
          <div className="mt-1 flex min-w-0 items-center gap-2">
            <Users className="h-5 w-5 shrink-0 text-accent" />
            <h1 className="truncate text-2xl font-bold">{t('projectMembers.title')}</h1>
          </div>
        </div>
      </div>

      <Card className="p-4">
        <form onSubmit={handleSubmit} className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_180px_auto] sm:items-end">
          <div className="space-y-1.5">
            <Label htmlFor="member-user">{t('projectMembers.user')}</Label>
            <select
              id="member-user"
              required
              className="h-9 w-full rounded-md border border-border bg-surface px-3 text-sm"
              value={form.user_id}
              onChange={event => setForm({ ...form, user_id: event.target.value })}
            >
              {users.map(user => (
                <option key={user.id} value={user.id}>{user.username}</option>
              ))}
            </select>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="member-role">{t('projectMembers.role')}</Label>
            <select
              id="member-role"
              className="h-9 w-full rounded-md border border-border bg-surface px-3 text-sm"
              value={form.role}
              onChange={event => setForm({ ...form, role: event.target.value as ProjectRole })}
            >
              {PROJECT_ROLES.map(role => <option key={role} value={role}>{role}</option>)}
            </select>
          </div>
          <Button type="submit" disabled={upsert.isPending || users.length === 0}>
            <UserPlus className="h-4 w-4" />
            {t('projectMembers.add')}
          </Button>
        </form>
      </Card>

      {isLoading ? (
        <p className="text-sm text-text-muted">{t('common.loading')}</p>
      ) : memberships.length === 0 ? (
        <Card className="p-8 text-center"><p className="text-text-muted">{t('projectMembers.empty')}</p></Card>
      ) : (
        <>
          <ul className="grid gap-3 md:hidden">
            {memberships.map(member => (
              <li key={member.user_id}>
                <Card className="p-4">
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <UserAvatar name={member.username} size="md" withName />
                      <span className="mt-2 inline-block rounded-md bg-surface-raised px-2 py-0.5 text-xs font-medium">{member.role}</span>
                    </div>
                    <span className={`shrink-0 rounded-full px-2 py-0.5 text-xs ${member.user_enabled ? 'bg-emerald-500/15 text-emerald-500' : 'bg-surface-raised text-text-muted'}`}>
                      {member.user_enabled ? t('users.active') : t('users.disabled')}
                    </span>
                  </div>
                  <Button size="sm" variant="outline" className="mt-3 min-h-9 w-full text-danger hover:text-danger" onClick={() => setPendingDelete(member)}>
                    <Trash2 className="h-3 w-3" />
                    {t('common.delete')}
                  </Button>
                </Card>
              </li>
            ))}
          </ul>

          <Card className="hidden md:block">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t('projectMembers.user')}</TableHead>
                  <TableHead>{t('projectMembers.role')}</TableHead>
                  <TableHead>{t('users.enabled')}</TableHead>
                  <TableHead>{t('projectMembers.updated')}</TableHead>
                  <TableHead className="w-20"><span className="sr-only">{t('common.actions')}</span></TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {memberships.map(member => (
                  <TableRow key={member.user_id}>
                    <TableCell className="font-medium">
                      <UserAvatar name={member.username} size="sm" withName />
                    </TableCell>
                    <TableCell>
                      <span className="rounded-md bg-surface-raised px-2 py-0.5 text-xs font-medium">{member.role}</span>
                    </TableCell>
                    <TableCell>
                      <span className={`rounded-full px-2 py-0.5 text-xs ${member.user_enabled ? 'bg-emerald-500/15 text-emerald-500' : 'bg-surface-raised text-text-muted'}`}>
                        {member.user_enabled ? t('users.active') : t('users.disabled')}
                      </span>
                    </TableCell>
                    <TableCell className="text-xs text-text-muted">{new Date(member.updated_at).toLocaleString()}</TableCell>
                    <TableCell>
                      <Button size="sm" variant="ghost" aria-label={`${t('common.delete')} ${member.username}`} className="h-7 px-2 text-xs text-danger hover:text-danger" onClick={() => setPendingDelete(member)}>
                        <Trash2 className="h-3 w-3" />
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </Card>
        </>
      )}

      <ConfirmDialog
        open={pendingDelete !== null}
        title={pendingDelete ? `${t('projectMembers.deleteConfirm')} "${pendingDelete.username}"?` : ''}
        onCancel={() => setPendingDelete(null)}
        onConfirm={() => {
          if (pendingDelete) {
            remove.mutate(pendingDelete.user_id, {
              onSuccess: () => toast.success(t('projectMembers.deleted')),
              onError: (err) => toast.error(err.message),
            })
          }
          setPendingDelete(null)
        }}
      />
    </div>
  )
}
