import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useUsers, useCreateUser, useUpdateUser, useApiTokens, useCreateApiToken, useDeleteApiToken, useProjects } from '@/api/hooks'
import { Card } from '@/shared/ui/card'
import { Button } from '@/shared/ui/button'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/ui/table'
import { Users as UsersIcon, Plus, KeyRound, Trash2, Copy } from 'lucide-react'
import { toast } from 'sonner'
import { ConfirmDialog } from '@/shared/ui/confirm-dialog'
import { UserAvatar } from '@/shared/ui/user-avatar'
import { formatDate } from '@/shared/lib/format'
import type { UserRole, ApiToken } from '@/api/types'

const ROLES: UserRole[] = ['admin', 'maintainer', 'developer', 'viewer']
const TOKEN_SCOPES = ['api:read', 'api:write', 'git:read', 'git:write'] as const
const DEFAULT_TOKEN_SCOPES = [...TOKEN_SCOPES]

export function UsersPage() {
  const { t } = useTranslation()
  const { data: users = [], isLoading } = useUsers()
  const createUser = useCreateUser()
  const updateUser = useUpdateUser()
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState({ username: '', role: 'viewer' as UserRole, password: '' })

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    const input = {
      username: form.username.trim(),
      role: form.role,
      ...(form.password ? { password: form.password } : {}),
    }
    createUser.mutate(input, {
      onSuccess: () => { setShowForm(false); setForm({ username: '', role: 'viewer', password: '' }); toast.success(t('users.created')) },
      onError: (err) => toast.error(err.message),
    })
  }

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2">
          <UsersIcon className="h-5 w-5 shrink-0 text-accent" />
          <h1 className="truncate text-2xl font-bold">{t('users.title')}</h1>
        </div>
        <Button size="sm" className="shrink-0" onClick={() => setShowForm(v => !v)}>
          <Plus className="h-4 w-4" />
          {t('users.create')}
        </Button>
      </div>

      {showForm && (
        <Card className="p-4">
          <form onSubmit={handleSubmit} className="grid gap-3 sm:grid-cols-2">
            <div className="space-y-1.5">
              <Label htmlFor="username">{t('users.username')}</Label>
              <Input id="username" required value={form.username} onChange={e => setForm({ ...form, username: e.target.value })} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="role">{t('users.role')}</Label>
              <select id="role" className="h-9 w-full rounded-md border border-border bg-surface px-3 text-sm" value={form.role} onChange={e => setForm({ ...form, role: e.target.value as UserRole })}>
                {ROLES.map(r => <option key={r} value={r}>{r}</option>)}
              </select>
            </div>
            <div className="space-y-1.5 sm:col-span-2">
              <Label htmlFor="user-password">{t('users.password')}</Label>
              <Input
                id="user-password"
                type="password"
                autoComplete="new-password"
                value={form.password}
                onChange={e => setForm({ ...form, password: e.target.value })}
              />
              <p className="text-xs text-text-muted">{t('users.passwordHint')}</p>
            </div>
            <div className="flex gap-2 sm:col-span-2">
              <Button type="submit" disabled={createUser.isPending}>{t('users.create')}</Button>
              <Button type="button" variant="ghost" onClick={() => setShowForm(false)}>{t('common.cancel')}</Button>
            </div>
          </form>
        </Card>
      )}

      {isLoading ? (
        <p className="text-sm text-text-muted">{t('common.loading')}</p>
      ) : users.length === 0 ? (
        <Card className="p-8 text-center"><p className="text-text-muted">{t('users.empty')}</p></Card>
      ) : (
        <>
        <ul className="grid gap-3 md:hidden">
          {users.map(u => (
            <li key={u.id}>
              <Card className="p-4">
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="flex items-center gap-2.5">
                      <UserAvatar name={u.username} size="md" />
                      <p className="truncate font-medium">{u.username}</p>
                    </div>
                    <span className="mt-1 inline-block rounded-md bg-surface-raised px-2 py-0.5 text-xs font-medium">{u.role}</span>
                  </div>
                  <span className={`shrink-0 rounded-full px-2 py-0.5 text-xs ${u.enabled ? 'bg-emerald-500/15 text-emerald-500' : 'bg-surface-raised text-text-muted'}`}>
                    {u.enabled ? t('users.active') : t('users.disabled')}
                  </span>
                </div>
                <p className="mt-2 text-xs text-text-muted">{new Date(u.created_at).toLocaleString()}</p>
                <Button size="sm" variant="outline" className="mt-3 min-h-9 w-full" onClick={() =>
                  updateUser.mutate({ id: u.id, username: u.username, role: u.role, enabled: !u.enabled }, {
                    onSuccess: () => toast.success(t('users.updated')),
                    onError: (err) => toast.error(err.message),
                  })
                }>
                  {u.enabled ? t('users.disable') : t('users.enable')}
                </Button>
              </Card>
            </li>
          ))}
        </ul>

        <Card className="hidden md:block">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t('users.username')}</TableHead>
                <TableHead>{t('users.role')}</TableHead>
                <TableHead>{t('users.enabled')}</TableHead>
                <TableHead>{t('users.created')}</TableHead>
                <TableHead>{t('users.toggle')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {users.map(u => (
                <TableRow key={u.id}>
                  <TableCell className="font-medium">
                    <UserAvatar name={u.username} size="sm" withName />
                  </TableCell>
                  <TableCell>
                    <span className="rounded-md bg-surface-raised px-2 py-0.5 text-xs font-medium">{u.role}</span>
                  </TableCell>
                  <TableCell>
                    <span className={`rounded-full px-2 py-0.5 text-xs ${u.enabled ? 'bg-emerald-500/15 text-emerald-500' : 'bg-surface-raised text-text-muted'}`}>
                      {u.enabled ? t('users.active') : t('users.disabled')}
                    </span>
                  </TableCell>
                  <TableCell className="text-xs text-text-muted">{new Date(u.created_at).toLocaleString()}</TableCell>
                  <TableCell>
                    <Button size="sm" variant="ghost" className="h-7 px-2 text-xs" onClick={() =>
                      updateUser.mutate({ id: u.id, username: u.username, role: u.role, enabled: !u.enabled }, {
                        onSuccess: () => toast.success(t('users.updated')),
                        onError: (err) => toast.error(err.message),
                      })
                    }>
                      {u.enabled ? t('users.disable') : t('users.enable')}
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </Card>
        </>
      )}

      <ApiTokensSection />
    </div>
  )
}

function ApiTokensSection() {
  const { t, i18n } = useTranslation()
  const { data: tokens = [], isLoading } = useApiTokens()
  const { data: projects = [], isLoading: projectsLoading } = useProjects()
  const createToken = useCreateApiToken()
  const deleteToken = useDeleteApiToken()
  const [pendingToken, setPendingToken] = useState<ApiToken | null>(null)
  const [newToken, setNewToken] = useState<string | null>(null)
  const [name, setName] = useState('')
  const [projectId, setProjectId] = useState('')
  const [expiresInDays, setExpiresInDays] = useState('30')
  const [scopes, setScopes] = useState<string[]>(DEFAULT_TOKEN_SCOPES)

  useEffect(() => {
    if (projects.length === 0) {
      setProjectId('')
      return
    }
    if (!projectId || !projects.some(project => project.id === projectId)) {
      setProjectId(projects[0].id)
    }
  }, [projectId, projects])

  function handleCreate(e: React.FormEvent) {
    e.preventDefault()
    const days = Number(expiresInDays)
    if (!projectId) {
      toast.error(t('tokens.projectRequired'))
      return
    }
    if (!Number.isInteger(days) || days < 1 || days > 365) {
      toast.error(t('tokens.invalidExpiry'))
      return
    }
    if (scopes.length === 0) {
      toast.error(t('tokens.scopeRequired'))
      return
    }
    createToken.mutate({ name, project_id: projectId, scopes, expires_in_days: days }, {
      onSuccess: (data) => {
        setNewToken(data.value)
        setName('')
        setExpiresInDays('30')
        setScopes(DEFAULT_TOKEN_SCOPES)
        toast.success(t('tokens.created'))
      },
      onError: (err) => toast.error(err.message),
    })
  }

  function toggleScope(scope: string) {
    setScopes(current =>
      current.includes(scope)
        ? current.filter(item => item !== scope)
        : [...current, scope],
    )
  }

  function projectLabel(id: string | null) {
    if (!id) return t('tokens.legacyGlobal')
    return projects.find(project => project.id === id)?.name ?? id
  }

  function dateLabel(value: string | null) {
    return value ? formatDate(value, i18n.language) : t('tokens.noExpiry')
  }

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <KeyRound className="h-5 w-5 text-accent" />
        <h2 className="text-lg font-semibold">{t('tokens.title')}</h2>
      </div>

      <Card className="p-4">
        <form onSubmit={handleCreate} className="grid gap-3 lg:grid-cols-[minmax(12rem,1.2fr)_minmax(12rem,1fr)_8rem_auto] lg:items-end">
          <div className="flex-1 space-y-1.5">
            <Label htmlFor="token-name">{t('tokens.name')}</Label>
            <Input id="token-name" required value={name} onChange={e => setName(e.target.value)} />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="token-project">{t('tokens.project')}</Label>
            <select
              id="token-project"
              required
              className="h-9 w-full rounded-md border border-border-strong bg-surface px-3 text-sm text-text-primary"
              value={projectId}
              onChange={e => setProjectId(e.target.value)}
              disabled={projectsLoading || projects.length === 0}
            >
              {projects.length === 0 ? (
                <option value="">{t('tokens.noProjects')}</option>
              ) : (
                projects.map(project => <option key={project.id} value={project.id}>{project.name}</option>)
              )}
            </select>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="token-expires">{t('tokens.expiresInDays')}</Label>
            <Input
              id="token-expires"
              required
              min={1}
              max={365}
              type="number"
              value={expiresInDays}
              onChange={e => setExpiresInDays(e.target.value)}
            />
          </div>
          <Button type="submit" disabled={createToken.isPending || projects.length === 0}>
            <Plus className="h-4 w-4" />
            {t('tokens.create')}
          </Button>
          <fieldset className="grid gap-2 sm:grid-cols-2 lg:col-span-4">
            <legend className="mb-1 text-sm font-medium text-text-secondary">{t('tokens.scopes')}</legend>
            {TOKEN_SCOPES.map(scope => (
              <label key={scope} className="flex min-h-9 items-center gap-2 rounded-md border border-border bg-surface px-3 text-sm text-text-secondary">
                <input
                  type="checkbox"
                  className="h-4 w-4 rounded border-border text-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                  checked={scopes.includes(scope)}
                  onChange={() => toggleScope(scope)}
                />
                <span>{t(`tokens.scopeLabels.${scope.replace(':', '')}`)}</span>
              </label>
            ))}
          </fieldset>
        </form>
      </Card>

      {newToken && (
        <Card className="border-emerald-500/30 bg-emerald-500/5 p-4">
          <div className="flex items-center gap-2">
            <p className="flex-1 break-all font-mono text-sm">{newToken}</p>
            <Button size="sm" variant="ghost" className="h-8 gap-1 text-xs" onClick={() => { navigator.clipboard.writeText(newToken); toast.success(t('tokens.copied')) }}>
              <Copy className="h-3 w-3" /> {t('tokens.copy')}
            </Button>
          </div>
          <p className="mt-2 text-xs text-amber-600 dark:text-amber-400">{t('tokens.copyWarning')}</p>
          <Button size="sm" variant="ghost" className="mt-2 h-7 text-xs" onClick={() => setNewToken(null)}>{t('common.dismiss')}</Button>
        </Card>
      )}

      {isLoading ? (
        <p className="text-sm text-text-muted">{t('common.loading')}</p>
      ) : tokens.length === 0 ? (
        <Card className="p-6 text-center"><p className="text-text-muted">{t('tokens.empty')}</p></Card>
      ) : (
        <>
          <ul className="grid gap-3 md:hidden">
            {tokens.map(tok => (
              <li key={tok.id}>
                <Card className="p-4">
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <p className="break-words font-medium">{tok.name}</p>
                      <p className="mt-1 break-words text-sm text-text-secondary">{projectLabel(tok.project_id)}</p>
                    </div>
                    <Button size="sm" variant="ghost" aria-label={`${t('common.delete')} ${tok.name}`} className="h-8 shrink-0 px-2 text-danger hover:text-danger" onClick={() => setPendingToken(tok)}>
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                  <div className="mt-3 flex flex-wrap gap-1">
                    {tok.scopes.map(scope => (
                      <span key={scope} className="rounded-md bg-surface-raised px-2 py-0.5 font-mono text-[11px] text-text-secondary">{scope}</span>
                    ))}
                  </div>
                  <dl className="mt-3 grid gap-2 text-xs text-text-muted">
                    <div className="flex items-center justify-between gap-3">
                      <dt>{t('tokens.expires')}</dt>
                      <dd className="text-right">{dateLabel(tok.expires_at)}</dd>
                    </div>
                    <div className="flex items-center justify-between gap-3">
                      <dt>{t('tokens.lastUsed')}</dt>
                      <dd className="text-right">{tok.last_used_at ? formatDate(tok.last_used_at, i18n.language) : t('tokens.neverUsed')}</dd>
                    </div>
                    <div className="flex items-center justify-between gap-3">
                      <dt>{t('tokens.hint')}</dt>
                      <dd className="font-mono">{tok.token_hint || 'n/a'}</dd>
                    </div>
                    <div className="flex items-center justify-between gap-3">
                      <dt>{t('tokens.created')}</dt>
                      <dd className="text-right">{formatDate(tok.created_at, i18n.language)}</dd>
                    </div>
                  </dl>
                </Card>
              </li>
            ))}
          </ul>

          <Card className="hidden overflow-x-auto md:block">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t('tokens.name')}</TableHead>
                  <TableHead>{t('tokens.project')}</TableHead>
                  <TableHead>{t('tokens.scopes')}</TableHead>
                  <TableHead>{t('tokens.expires')}</TableHead>
                  <TableHead>{t('tokens.lastUsed')}</TableHead>
                  <TableHead>{t('tokens.hint')}</TableHead>
                  <TableHead>{t('tokens.created')}</TableHead>
                  <TableHead className="w-20"><span className="sr-only">{t('common.actions')}</span></TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {tokens.map(tok => (
                  <TableRow key={tok.id}>
                    <TableCell className="whitespace-nowrap font-medium">{tok.name}</TableCell>
                    <TableCell className="min-w-36 text-sm text-text-secondary">{projectLabel(tok.project_id)}</TableCell>
                    <TableCell className="min-w-52">
                      <div className="flex flex-wrap gap-1">
                        {tok.scopes.map(scope => (
                          <span key={scope} className="rounded-md bg-surface-raised px-2 py-0.5 font-mono text-[11px] text-text-secondary">{scope}</span>
                        ))}
                      </div>
                    </TableCell>
                    <TableCell className="min-w-36 text-xs text-text-muted">{dateLabel(tok.expires_at)}</TableCell>
                    <TableCell className="min-w-36 text-xs text-text-muted">
                      {tok.last_used_at ? formatDate(tok.last_used_at, i18n.language) : t('tokens.neverUsed')}
                    </TableCell>
                    <TableCell className="font-mono text-xs text-text-muted">{tok.token_hint}</TableCell>
                    <TableCell className="text-xs text-text-muted">{formatDate(tok.created_at, i18n.language)}</TableCell>
                    <TableCell>
                      <Button size="sm" variant="ghost" aria-label={`${t('common.delete')} ${tok.name}`} className="h-7 px-2 text-xs text-danger hover:text-danger" onClick={() => setPendingToken(tok)}>
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
        open={pendingToken !== null}
        title={t('tokens.deleteConfirm')}
        onCancel={() => setPendingToken(null)}
        onConfirm={() => {
          if (pendingToken) deleteToken.mutate(pendingToken.id, { onSuccess: () => toast.success(t('tokens.deleted')), onError: (err: Error) => toast.error(err.message) })
          setPendingToken(null)
        }}
      />
    </div>
  )
}
