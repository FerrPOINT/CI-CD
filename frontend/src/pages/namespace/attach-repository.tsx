import { useState, type FormEvent } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Button, Input, Label } from '@sdlc/ui/ui'
import { useSessionCommand, type NamespaceLocation } from '@sdlc/ui/lib'
import { api } from '@/api/client'
import { useAuth } from '@/shared/auth/auth-provider'
import type { components } from '@/api/schema'

type Attach = components['schemas']['AttachRepository']
export function AttachRepository({
  groupId,
  namespace,
}: {
  groupId: string
  namespace: NamespaceLocation
}) {
  const { session } = useAuth()
  const cache = useQueryClient()
  const [offset, setOffset] = useState(0)
  const [repository, setRepository] = useState('')
  const [slug, setSlug] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string>()
  const journal = useSessionCommand<{ repository: string; command: Attach }>(
    `forge:repository-attach:${session?.subject}:${namespace.registry_instance_id}:${namespace.namespace_id}`,
  )
  const available = useQuery({
    queryKey: [
      'unbound-repositories',
      session?.subject,
      namespace.registry_instance_id,
      namespace.namespace_id,
      offset,
    ],
    queryFn: ({ signal }) =>
      api<components['schemas']['CatalogPage']>(
        `/catalog/available-repositories?limit=50&offset=${offset}`,
        { signal },
      ),
  })
  async function attach(event: FormEvent) {
    event.preventDefault()
    if (!session?.subject) {
      setError('Требуется действующая SSO-сессия')
      return
    }
    const command = journal.capture({
      repository,
      command: { operation_id: crypto.randomUUID(), group_id: groupId, slug },
    })
    setBusy(true)
    setError(undefined)
    try {
      await api<components['schemas']['AttachReadback']>(
        `/catalog/repositories/${command.repository}/group`,
        { method: 'PUT', body: JSON.stringify(command.command) },
      )
      journal.clear()
      setRepository('')
      setSlug('')
      await Promise.all([
        cache.invalidateQueries({ queryKey: ['unbound-repositories'] }),
        cache.invalidateQueries({
          queryKey: [
            'namespace-repositories',
            namespace.registry_instance_id,
            namespace.namespace_id,
          ],
        }),
      ])
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Подключение недоступно')
    } finally {
      setBusy(false)
    }
  }
  return (
    <form
      onSubmit={attach}
      className="max-w-xl space-y-4 rounded-lg border border-border bg-surface p-4"
    >
      <h2 className="font-semibold">Подключить существующий репозиторий Forge</h2>
      <p className="text-sm text-text-muted">
        История Git, номера PR и прежний clone URL сохраняются.
      </p>
      {available.isError ? (
        <p role="alert" className="text-danger">
          Каталог недоступен.
        </p>
      ) : (
        <div className="space-y-2">
          <Label htmlFor="existing-repository">Репозиторий</Label>
          <select
            id="existing-repository"
            required
            value={journal.command?.repository ?? repository}
            disabled={busy || Boolean(journal.command) || available.isPending}
            onChange={(e) => {
              setRepository(e.target.value)
              setSlug(available.data?.items.find((r) => r.id === e.target.value)?.slug ?? '')
            }}
            className="min-h-10 w-full rounded-md border border-border bg-surface px-3 text-sm"
          >
            <option value="">Выберите репозиторий</option>
            {available.data?.items.map((r) => (
              <option key={r.id} value={r.id}>
                {r.public_name}
              </option>
            ))}
            {journal.command &&
              !available.data?.items.some((r) => r.id === journal.command?.repository) && (
                <option value={journal.command.repository}>Исходный репозиторий операции</option>
              )}
          </select>
        </div>
      )}
      <div className="flex gap-2">
        <Button
          type="button"
          variant="outline"
          disabled={offset === 0 || busy || Boolean(journal.command)}
          onClick={() => setOffset(Math.max(0, offset - 50))}
        >
          Назад
        </Button>
        <Button
          type="button"
          variant="outline"
          disabled={(available.data?.items.length ?? 0) < 50 || busy || Boolean(journal.command)}
          onClick={() => setOffset(offset + 50)}
        >
          Далее
        </Button>
        <Button type="button" variant="outline" onClick={() => void available.refetch()}>
          Обновить
        </Button>
      </div>
      <div className="space-y-2">
        <Label htmlFor="existing-repository-slug">Имя внутри проекта</Label>
        <Input
          id="existing-repository-slug"
          required
          value={journal.command?.command.slug ?? slug}
          disabled={busy || Boolean(journal.command)}
          onChange={(e) => setSlug(e.target.value)}
        />
      </div>
      {error && (
        <p role="alert" className="text-danger">
          {error}. Повтор использует исходную операцию.
        </p>
      )}
      <Button disabled={busy || (!journal.command && (!repository || !slug))}>
        {busy ? 'Подключаем…' : journal.command ? 'Повторить исходную операцию' : 'Подключить'}
      </Button>
    </form>
  )
}
