import { ExternalLink, Users } from 'lucide-react'
import { Button } from '@sdlc/ui/ui'

const adminUrl = import.meta.env.VITE_ADMIN_UI_URL ?? 'http://localhost:7772'

export function UsersPage() {
  return <div className="space-y-4">
    <h1 className="flex items-center gap-2 text-xl font-semibold"><Users className="h-5 w-5" />Пользователи платформы</h1>
    <p className="text-sm text-text-secondary">Учётные записи и личные API-токены управляются в Admin Panel.</p>
    <Button asChild className="min-h-10 sm:min-h-10">
      <a href={`${adminUrl.replace(/\/$/, '')}/users`}>
        Открыть пользователей <ExternalLink className="h-4 w-4" aria-hidden="true" />
      </a>
    </Button>
  </div>
}
