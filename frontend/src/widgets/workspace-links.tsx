import { usePlatformServices } from '@sdlc/ui/ui'
import { withNamespaceLocation, type NamespaceLocation } from '@sdlc/ui/lib'
export function WorkspaceLinks({ namespace }: { namespace?: NamespaceLocation }) {
  const { services } = usePlatformServices()
  const links = namespace
    ? [
        ['task-tracker', 'Задачи', '/namespace'],
        ['wiki', 'Документы', '/namespace'],
        ['admin-panel', 'Подключения', `/namespaces/${namespace.namespace_id}`],
      ]
    : [['admin-panel', 'Создать проект', '/namespaces']]
  return (
    <div className="flex flex-wrap gap-4 text-sm">
      {links.map(([key, label, path]) => {
        const origin = services.find((service) => service.key === key)?.ui_url
        return origin ? (
          <a
            key={key}
            className="text-accent"
            href={withNamespaceLocation(origin + path, namespace ?? null)}
          >
            {label}
          </a>
        ) : (
          <span key={key} className="text-text-muted">
            {label}: недоступно
          </span>
        )
      })}
    </div>
  )
}
