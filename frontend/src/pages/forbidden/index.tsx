// K2: 403 UX — rendered whenever the API answers forbidden for the current
// identity (server-enforced role policy, see AUTHZ_CONTRACT).

import { Link } from 'react-router'
import { useTranslation } from 'react-i18next'
import { ShieldAlert } from 'lucide-react'
import { Button } from '@sdlc/ui/ui'

export function ForbiddenPage() {
  const { t } = useTranslation()
  return (
    <div className="flex min-h-[60vh] flex-col items-center justify-center gap-4 p-8 text-center">
      <ShieldAlert className="h-12 w-12 text-status-failed" aria-hidden />
      <h1 className="text-xl font-semibold">{t('errors.forbidden.title', 'Доступ запрещён')}</h1>
      <p className="max-w-md text-sm text-text-secondary">
        {t('errors.forbidden.description', 'У вашей роли нет прав на это действие или раздел. Обратитесь к администратору проекта.')}
      </p>
      <Button asChild variant="outline">
        <Link to="/">{t('errors.forbidden.back', 'На главную')}</Link>
      </Button>
    </div>
  )
}
