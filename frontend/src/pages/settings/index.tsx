import { useTranslation } from 'react-i18next'
import { Card } from '@/shared/ui/card'
import { Settings, Server, Database, GitBranch, Info } from 'lucide-react'

export function SettingsPage() {
  const { t } = useTranslation()

  const cicdVars = [
    { key: 'CICD_DATABASE_URL', desc: 'PostgreSQL connection string' },
    { key: 'CICD_BIND', desc: 'API bind address (default: 0.0.0.0:22801)' },
    { key: 'CICD_GIT_ROOT', desc: 'Git storage path (default: /var/lib/forge/git)' },
    { key: 'CICD_GIT_TOKEN', desc: 'Git HTTP token (optional)' },
    { key: 'CICD_GIT_INTERNAL_TOKEN', desc: 'Internal git push token (optional)' },
  ]

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-2">
        <Settings className="h-5 w-5 text-accent" />
        <h1 className="text-2xl font-bold">{t('navigation.settings')}</h1>
      </div>

      <div className="grid gap-4 sm:grid-cols-2">
        <Card className="p-4">
          <div className="flex items-center gap-2">
            <Server className="h-4 w-4 text-accent" />
            <h3 className="font-medium">API</h3>
          </div>
          <dl className="mt-3 space-y-1 text-sm">
            <div className="flex justify-between"><dt className="text-text-muted">Version</dt><dd>0.1.0</dd></div>
            <div className="flex justify-between"><dt className="text-text-muted">Port</dt><dd>:22801</dd></div>
          </dl>
        </Card>

        <Card className="p-4">
          <div className="flex items-center gap-2">
            <GitBranch className="h-4 w-4 text-accent" />
            <h3 className="font-medium">Git</h3>
          </div>
          <dl className="mt-3 space-y-1 text-sm">
            <div className="flex justify-between"><dt className="text-text-muted">Storage path</dt><dd>/var/lib/forge/git</dd></div>
            <div className="flex justify-between"><dt className="text-text-muted">HTTP port</dt><dd>:22802</dd></div>
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

        <Card className="p-4 sm:col-span-2">
          <div className="flex items-center gap-2">
            <Info className="h-4 w-4 text-accent" />
            <h3 className="font-medium">CICD_ Environment Variables</h3>
          </div>
          <dl className="mt-3 space-y-2 text-sm">
            {cicdVars.map(v => (
              <div key={v.key} className="flex flex-col gap-0.5 sm:flex-row sm:justify-between">
                <dt><code className="rounded bg-surface-raised px-1.5 py-0.5 text-xs">{v.key}</code></dt>
                <dd className="text-text-muted">{v.desc}</dd>
              </div>
            ))}
          </dl>
        </Card>
      </div>
    </div>
  )
}