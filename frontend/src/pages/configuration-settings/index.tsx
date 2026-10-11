import { useState, type FormEvent } from 'react'
import { useParams } from 'react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Button, Input, Label } from '@sdlc/ui/ui'
import { api } from '@/api/client'
import { useNamespaceRef } from '@/widgets/namespace-context'
import type { components } from '@/api/schema'
import type { Configuration } from '@/api/workspaces'

export function ConfigurationSettingsPage() {
  const { projectId } = useParams()
  const { ref } = useNamespaceRef()
  const cache = useQueryClient()
  const config = useQuery({
    queryKey: ['delivery-config', ref?.registry_instance_id, ref?.namespace_id, projectId],
    queryFn: ({ signal }) =>
      api<Configuration>(`/delivery-configurations/${projectId}`, { signal }),
  })
  const repository = useQuery({
    queryKey: [
      'catalog-repository',
      ref?.registry_instance_id,
      ref?.namespace_id,
      config.data?.repository_id,
    ],
    enabled: !!config.data?.repository_id,
    queryFn: ({ signal }) =>
      api<components['schemas']['CatalogRepository']>(
        `/catalog/repositories/${config.data!.repository_id}`,
        { signal },
      ),
  })
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState<string>()
  const writable =
    !!config.data && (!config.data.repository_id || repository.data?.state === 'active')
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const values = new FormData(event.currentTarget)
    const limit = String(values.get('limit'))
    const input: components['schemas']['UpdateProject'] = {
      name: String(values.get('name')).trim(),
      default_branch: String(values.get('branch')).trim(),
      max_running_jobs: limit ? Number(limit) : null,
      ...(!config.data?.repository_id ? { repository_url: String(values.get('url')).trim() } : {}),
    }
    setBusy(true)
    setMessage(undefined)
    try {
      await api(`/projects/${projectId}`, { method: 'PATCH', body: JSON.stringify(input) })
      await Promise.all([
        cache.invalidateQueries({ queryKey: ['delivery-config'] }),
        cache.invalidateQueries({ queryKey: ['delivery-configurations'] }),
        cache.invalidateQueries({ queryKey: ['executions'] }),
      ])
      setMessage('Настройки сохранены.')
    } catch (error) {
      setMessage(error instanceof Error ? error.message : 'Не удалось сохранить настройки.')
    } finally {
      setBusy(false)
    }
  }
  if (!config.data) return <p role="status">Загружаем настройки…</p>
  return (
    <form key={config.data.id} onSubmit={save} className="max-w-xl space-y-4">
      <h1 className="text-xl font-semibold">Настройки CI-конфигурации</h1>
      <Label htmlFor="configuration-name">Название конфигурации</Label>
      <Input
        id="configuration-name"
        name="name"
        defaultValue={config.data.name}
        required
        maxLength={255}
        disabled={!writable || busy}
      />
      <Label htmlFor="configuration-branch">Ветка по умолчанию</Label>
      <Input
        id="configuration-branch"
        name="branch"
        defaultValue={config.data.default_branch}
        required
        disabled={!writable || busy}
      />
      <Label htmlFor="configuration-limit">Одновременно выполняемых jobs</Label>
      <Input
        id="configuration-limit"
        name="limit"
        type="number"
        min={1}
        max={4096}
        defaultValue={config.data.max_running_jobs ?? ''}
        placeholder="Без ограничения"
        disabled={!writable || busy}
      />
      <Label htmlFor="configuration-url">Git URL</Label>
      <Input
        id="configuration-url"
        name="url"
        defaultValue={config.data.repository_url}
        readOnly={!!config.data.repository_id}
        required
        disabled={!writable || busy}
      />
      {config.data.repository_id && (
        <p className="text-sm text-text-muted">
          Источник checkout определяется связанным репозиторием.
        </p>
      )}
      {!writable && <p role="alert">Настройки доступны только для чтения.</p>}
      {message && <p role="status">{message}</p>}
      <Button disabled={!writable || busy}>Сохранить</Button>
    </form>
  )
}
