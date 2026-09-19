import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Search, Settings, X } from 'lucide-react'
import { Button } from '@sdlc/ui/ui'

const configGroups = [
  {
    titleKey: 'settings.platformGroup',
    vars: [
      { key: 'CICD_DATABASE_URL', descKey: 'settings.env.databaseUrl' },
      { key: 'CICD_BIND', descKey: 'settings.env.bind' },
      { key: 'CICD_AUTH_SECRET', descKey: 'settings.env.authSecret' },
      { key: 'CICD_CORS_ALLOWED_ORIGINS', descKey: 'settings.env.corsAllowedOrigins' },
      { key: 'CICD_SECRETS_KEY', descKey: 'settings.env.secretsKey' },
      { key: 'CICD_GIT_ROOT', descKey: 'settings.env.gitRoot' },
      { key: 'CICD_GIT_TOKEN', descKey: 'settings.env.gitToken' },
      { key: 'CICD_GIT_INTERNAL_TOKEN', descKey: 'settings.env.gitInternalToken' },
      { key: 'CICD_ARTIFACTS_DIR', descKey: 'settings.env.artifactsDir' },
      { key: 'CICD_ARTIFACT_RETENTION_DAYS', descKey: 'settings.env.artifactRetentionDays' },
    ],
  },
  {
    titleKey: 'settings.runnerGroup',
    vars: [
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
    ],
  },
]

export function SettingsPage() {
  const { t } = useTranslation()
  const [search, setSearch] = useState('')
  const query = search.trim().toLocaleLowerCase()
  const filteredGroups = configGroups.map(group => ({
    ...group,
    vars: group.vars.filter(variable =>
      variable.key.toLocaleLowerCase().includes(query) || t(variable.descKey).toLocaleLowerCase().includes(query),
    ),
  })).filter(group => group.vars.length > 0)

  return (
    <div className="max-w-5xl space-y-5">
      <div className="flex items-center gap-2">
        <Settings className="h-5 w-5 shrink-0 text-accent" aria-hidden />
        <h1 className="text-xl font-bold sm:text-2xl">{t('settings.title')}</h1>
      </div>
      <p className="text-sm text-text-muted">{t('settings.description')}</p>

      <section aria-labelledby="defaults-heading">
        <h2 id="defaults-heading" className="mb-2 text-base font-semibold">{t('settings.defaults')}</h2>
        <dl className="grid grid-cols-2 border-y border-border text-sm sm:grid-cols-3">
          <div className="min-w-0 py-3 pr-3"><dt className="text-text-muted">{t('settings.apiPort')}</dt><dd className="mt-1 font-mono">:22801</dd></div>
          <div className="min-w-0 border-l border-border px-3 py-3"><dt className="text-text-muted">{t('settings.dashboardPort')}</dt><dd className="mt-1 font-mono">:22802</dd></div>
          <div className="min-w-0 border-t border-border py-3 pr-3 sm:border-l sm:border-t-0 sm:px-3"><dt className="text-text-muted">{t('settings.database')}</dt><dd className="mt-1 font-mono">PostgreSQL 17</dd></div>
        </dl>
        <p className="mt-2 text-xs text-text-muted">{t('settings.defaultsNote')}</p>
      </section>

      <section aria-labelledby="variables-heading">
        <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
          <h2 id="variables-heading" className="text-base font-semibold">{t('settings.envVars')}</h2>
          <div className="relative w-full sm:w-72">
            <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted" aria-hidden />
            <input
              type="search"
              aria-label={t('settings.search')}
              placeholder={t('settings.search')}
              value={search}
              onChange={event => setSearch(event.target.value)}
              className="min-h-10 w-full rounded-md border border-border bg-surface py-2 pl-9 pr-10 text-sm text-text-primary outline-none focus-visible:border-accent"
            />
            {search && <Button type="button" variant="ghost" size="icon" aria-label={t('settings.clearSearch')} onClick={() => setSearch('')} className="absolute right-0 top-0 h-10 w-10"><X className="h-4 w-4" /></Button>}
          </div>
        </div>

        {filteredGroups.length === 0 ? (
          <p role="status" className="border-y border-border py-5 text-sm text-text-muted">{t('settings.noMatches')}</p>
        ) : filteredGroups.map(group => (
          <div key={group.titleKey} className="mb-5">
            <h3 className="mb-1 text-sm font-semibold text-text-secondary">{t(group.titleKey)}</h3>
            <dl className="divide-y divide-border border-y border-border">
              {group.vars.map(variable => (
                <div key={variable.key} className="grid gap-1 py-2 text-sm lg:grid-cols-[minmax(0,19rem)_minmax(0,1fr)] lg:gap-4">
                  <dt className="min-w-0 break-all font-mono text-xs text-text-primary">{variable.key}</dt>
                  <dd className="min-w-0 text-text-muted">{t(variable.descKey)}</dd>
                </div>
              ))}
            </dl>
          </div>
        ))}
      </section>
    </div>
  )
}
