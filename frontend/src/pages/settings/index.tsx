import { useTranslation } from 'react-i18next'
import { Card } from '@/shared/ui/card'
import { Settings, Server, Database, GitBranch, Info } from 'lucide-react'

export function SettingsPage() {
  const { t } = useTranslation()

  const cicdVars = [
    { key: 'CICD_DATABASE_URL', descKey: 'settings.env.databaseUrl' },
    { key: 'CICD_BIND', descKey: 'settings.env.bind' },
    { key: 'CICD_AUTH_SECRET', descKey: 'settings.env.authSecret' },
    { key: 'CICD_SECRETS_KEY', descKey: 'settings.env.secretsKey' },
    { key: 'CICD_GIT_ROOT', descKey: 'settings.env.gitRoot' },
    { key: 'CICD_GIT_TOKEN', descKey: 'settings.env.gitToken' },
    { key: 'CICD_GIT_INTERNAL_TOKEN', descKey: 'settings.env.gitInternalToken' },
    { key: 'CICD_ARTIFACTS_DIR', descKey: 'settings.env.artifactsDir' },
    { key: 'CICD_EMBEDDED_RUNNER_ENABLED', descKey: 'settings.env.embeddedRunnerEnabled' },
    { key: 'CICD_RUNNER_MODE', descKey: 'settings.env.runnerMode' },
    { key: 'CICD_RUNNER_REGISTRATION_TOKEN', descKey: 'settings.env.runnerRegistrationToken' },
    { key: 'CICD_RUNNER_CREDENTIAL', descKey: 'settings.env.runnerCredential' },
    { key: 'CICD_RUNNER_NAME', descKey: 'settings.env.runnerName' },
    { key: 'CICD_RUNNER_TAGS', descKey: 'settings.env.runnerTags' },
    { key: 'CICD_RUNNER_TOTAL_SLOTS', descKey: 'settings.env.runnerTotalSlots' },
    { key: 'CICD_RUNNER_POLL_INTERVAL_SECONDS', descKey: 'settings.env.runnerPollIntervalSeconds' },
    { key: 'CICD_RUNNER_NO_CHECKOUT', descKey: 'settings.env.runnerNoCheckout' },
    { key: 'CICD_RUNNER_WORK_DIR', descKey: 'settings.env.runnerWorkDir' },
    { key: 'CICD_RUNNER_KEEP_WORKSPACE', descKey: 'settings.env.runnerKeepWorkspace' },
  ]

  return (
    <div className="space-y-6">
      <div className="space-y-1">
        <div className="flex items-center gap-2">
          <Settings className="h-5 w-5 text-accent" />
          <h1 className="text-2xl font-bold">{t('settings.title')}</h1>
        </div>
        <p className="text-sm text-text-muted">{t('settings.description')}</p>
      </div>

      <div className="grid gap-4 sm:grid-cols-2">
        <Card className="p-4">
          <div className="flex items-center gap-2">
            <Server className="h-4 w-4 text-accent" />
            <h3 className="font-medium">{t('settings.api')}</h3>
          </div>
          <dl className="mt-3 space-y-1 text-sm">
            <div className="flex justify-between gap-4"><dt className="text-text-muted">{t('settings.version')}</dt><dd>0.1.0</dd></div>
            <div className="flex justify-between gap-4"><dt className="text-text-muted">{t('settings.apiPort')}</dt><dd>:22801</dd></div>
            <div className="flex justify-between gap-4"><dt className="text-text-muted">{t('settings.dashboardPort')}</dt><dd>:22802</dd></div>
          </dl>
        </Card>

        <Card className="p-4">
          <div className="flex items-center gap-2">
            <GitBranch className="h-4 w-4 text-accent" />
            <h3 className="font-medium">{t('settings.git')}</h3>
          </div>
          <dl className="mt-3 space-y-1 text-sm">
            <div className="flex justify-between gap-4"><dt className="text-text-muted">{t('settings.storagePath')}</dt><dd className="text-right">/var/lib/forge/git</dd></div>
            <div className="flex justify-between gap-4"><dt className="text-text-muted">{t('settings.httpPort')}</dt><dd>:22801</dd></div>
          </dl>
        </Card>

        <Card className="p-4">
          <div className="flex items-center gap-2">
            <Database className="h-4 w-4 text-accent" />
            <h3 className="font-medium">{t('settings.database')}</h3>
          </div>
          <dl className="mt-3 space-y-1 text-sm">
            <div className="flex justify-between gap-4"><dt className="text-text-muted">{t('settings.engine')}</dt><dd>PostgreSQL 17</dd></div>
            <div className="flex justify-between gap-4"><dt className="text-text-muted">{t('settings.port')}</dt><dd>:22543</dd></div>
          </dl>
        </Card>

        <Card className="p-4 sm:col-span-2">
          <div className="flex items-center gap-2">
            <Info className="h-4 w-4 text-accent" />
            <h3 className="font-medium">{t('settings.envVars')}</h3>
          </div>
          <dl className="mt-3 space-y-2 text-sm">
            {cicdVars.map(v => (
              <div key={v.key} className="flex flex-col gap-0.5 sm:flex-row sm:justify-between">
                <dt><code className="rounded bg-surface-raised px-1.5 py-0.5 text-xs">{v.key}</code></dt>
                <dd className="text-text-muted sm:max-w-xl sm:text-right">{t(v.descKey)}</dd>
              </div>
            ))}
          </dl>
        </Card>
      </div>
    </div>
  )
}
