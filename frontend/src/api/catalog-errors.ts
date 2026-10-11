import { ApiError } from './client'

const messages: Record<string, string> = {
  invalid_repository_properties: 'Проверьте тип Git, URL и видимость репозитория.',
  invalid_repository_command: 'Проверьте данные репозитория.',
  invalid_repository_attach: 'Проверьте выбранный репозиторий и имя внутри проекта.',
  invalid_delivery_configuration: 'Укажите название конфигурации и ветку.',
  repository_identity_or_slug_conflict: 'Репозиторий с этим именем уже существует в проекте.',
  namespace_resource_read_only: 'Проект доступен только для чтения.',
  repository_not_ready_or_attached: 'Репозиторий ещё не готов к настройке CI.',
  configuration_repository_mismatch: 'Конфигурация принадлежит другому репозиторию.',
}

export function catalogErrorMessage(error: unknown): string {
  if (error instanceof ApiError) {
    const specific = messages[error.message] ?? (error.code ? messages[error.code] : undefined)
    if (specific) return specific
    if (error.kind === 'network') return 'Связь прервалась. Проверьте результат операции перед повтором.'
    if (error.status === 403) return 'Нет прав на это действие.'
    if (error.status === 401) return 'Сессия завершилась. Войдите снова.'
  }
  return error instanceof Error ? error.message : 'Не удалось выполнить действие.'
}
