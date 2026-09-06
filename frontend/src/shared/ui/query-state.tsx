// K2/K3: unified query states — loading skeleton, empty state, error (with
// 403 → ForbiddenPage and 401-expiry handled by the terminal hook).

import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Inbox } from 'lucide-react'
import { ApiError } from '@/api/client'
import { ForbiddenPage } from '@/pages/forbidden'

export function QueryState<T>({
  data,
  isLoading,
  error,
  isEmpty,
  empty,
  children,
}: {
  data: T | undefined
  isLoading: boolean
  error: unknown
  isEmpty?: (data: T) => boolean
  empty?: { title?: string; description?: string }
  children: (data: T) => ReactNode
}) {
  const { t } = useTranslation()

  if (isLoading) {
    return (
      <div className="space-y-2" aria-busy="true" aria-label={t('common.loading', 'Загрузка')}>
        <div className="h-8 animate-pulse rounded bg-surface-raised" />
        <div className="h-8 animate-pulse rounded bg-surface-raised" />
        <div className="h-8 animate-pulse rounded bg-surface-raised" />
      </div>
    )
  }

  if (error) {
    if (error instanceof ApiError && error.status === 403) {
      return <ForbiddenPage />
    }
    return (
      <div className="rounded-md border border-status-failed/40 bg-surface p-4 text-sm text-text-secondary" role="alert">
        {error instanceof Error ? error.message : t('errors.unknown', 'Неизвестная ошибка')}
      </div>
    )
  }

  if (data === undefined || data === null) {
    return <div className="p-4 text-sm text-text-muted">{t('common.noData', 'Нет данных')}</div>
  }

  if (isEmpty?.(data)) {
    return (
      <div className="flex flex-col items-center justify-center gap-2 rounded-md border border-dashed border-border p-8 text-center">
        <Inbox className="h-8 w-8 text-text-muted" aria-hidden />
        <p className="text-sm font-medium">{empty?.title ?? t('common.empty', 'Пусто')}</p>
        {empty?.description && <p className="max-w-sm text-xs text-text-secondary">{empty.description}</p>}
      </div>
    )
  }

  return <>{children(data)}</>
}
