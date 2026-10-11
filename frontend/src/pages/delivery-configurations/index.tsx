import { useState } from 'react'
import { Button, Input, NamespaceLink as Link } from '@sdlc/ui/ui'
import { useConfigurations } from '@/api/workspaces'
export function DeliveryConfigurationsPage() {
  const [offset, setOffset] = useState(0)
  const [search, setSearch] = useState('')
  const configs = useConfigurations(undefined, offset, search)
  return (
    <div className="space-y-5">
      <h1 className="text-2xl font-semibold">Непривязанные CI-конфигурации</h1>
      <p className="text-text-muted">
        Существующие настройки и история сохранены. Подключите конфигурацию на странице нужного
        репозитория.
      </p>
      <Input
        className="max-w-md"
        aria-label="Поиск конфигураций"
        placeholder="Поиск конфигураций…"
        value={search}
        onChange={(event) => {
          setSearch(event.target.value)
          setOffset(0)
        }}
      />
      {configs.isPending ? (
        <p role="status">Загружаем конфигурации…</p>
      ) : configs.isError ? (
        <p role="alert">Конфигурации недоступны.</p>
      ) : (
        <ul className="divide-y divide-border rounded-lg border border-border bg-surface">
          {configs.data.items.map((config) => (
            <li key={config.id} className="p-4">
              <Link className="text-accent" to={`/delivery-configs/${config.id}/pipelines`}>
                {config.name}
              </Link>
              <p className="break-all text-sm text-text-muted">{config.repository_url}</p>
            </li>
          ))}
          {!configs.data.total && (
            <li className="p-4 text-text-muted">Все конфигурации подключены.</li>
          )}
        </ul>
      )}
      <div className="flex gap-2">
        <Button
          variant="outline"
          disabled={!offset}
          onClick={() => setOffset(Math.max(0, offset - 50))}
        >
          Назад
        </Button>
        <Button
          variant="outline"
          disabled={!configs.data || offset + 50 >= configs.data.total}
          onClick={() => setOffset(offset + 50)}
        >
          Далее
        </Button>
      </div>
    </div>
  )
}
