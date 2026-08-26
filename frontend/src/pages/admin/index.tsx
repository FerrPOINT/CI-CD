import { useTranslation } from 'react-i18next'
import { Card } from '@/shared/ui/card'
import { Settings, Server, Database } from 'lucide-react'

export function AdminPage() {
  const { t } = useTranslation()

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-2">
        <Settings className="h-5 w-5 text-accent" />
        <h1 className="text-2xl font-bold">{t('navigation.admin')}</h1>
      </div>

      <div className="grid gap-4 sm:grid-cols-2">
        <Card className="p-4">
          <div className="flex items-center gap-2">
            <Server className="h-4 w-4 text-accent" />
            <h3 className="font-medium">System</h3>
          </div>
          <dl className="mt-3 space-y-1 text-sm">
            <div className="flex justify-between"><dt className="text-text-muted">Version</dt><dd>0.1.0</dd></div>
            <div className="flex justify-between"><dt className="text-text-muted">API</dt><dd>:22801</dd></div>
            <div className="flex justify-between"><dt className="text-text-muted">Dashboard</dt><dd>:22802</dd></div>
          </dl>
        </Card>

        <Card className="p-4">
          <div className="flex items-center gap-2">
            <Database className="h-4 w-4 text-accent" />
            <h3 className="font-medium">Database</h3>
          </div>
          <dl className="mt-3 space-y-1 text-sm">
            <div className="flex justify-between"><dt className="text-text-muted">Engine</dt><dd>PostgreSQL 17</dd></div>
            <div className="flex justify-between"><dt className="text-text-muted">Port</dt><dd>:22543</dd></div>
          </dl>
        </Card>
      </div>
    </div>
  )
}
