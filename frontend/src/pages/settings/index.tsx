import {
  DescriptionItem,
  PageHeader,
  SectionHeader,
  NamespaceLink as Link,
  SearchInput,
  EmptyState,
  TextLink,
} from "@pdlc/ui/ui";

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Settings } from "lucide-react";

const configGroups = [
  {
    titleKey: "settings.platformGroup",
    vars: [
      { key: "CICD_DATABASE_URL", descKey: "settings.env.databaseUrl" },
      { key: "CICD_BIND", descKey: "settings.env.bind" },
      { key: "CICD_AUTH_SECRET", descKey: "settings.env.authSecret" },
      {
        key: "CICD_CORS_ALLOWED_ORIGINS",
        descKey: "settings.env.corsAllowedOrigins",
      },
      { key: "CICD_SECRETS_KEY", descKey: "settings.env.secretsKey" },
      { key: "CICD_GIT_ROOT", descKey: "settings.env.gitRoot" },
      { key: "CICD_GIT_TOKEN", descKey: "settings.env.gitToken" },
      {
        key: "CICD_GIT_INTERNAL_TOKEN",
        descKey: "settings.env.gitInternalToken",
      },
      { key: "CICD_ARTIFACTS_DIR", descKey: "settings.env.artifactsDir" },
      {
        key: "CICD_ARTIFACT_RETENTION_DAYS",
        descKey: "settings.env.artifactRetentionDays",
      },
    ],
  },
  {
    titleKey: "settings.runnerGroup",
    vars: [
      {
        key: "CICD_EMBEDDED_RUNNER_ENABLED",
        descKey: "settings.env.embeddedRunnerEnabled",
      },
      { key: "CICD_RUNNER_MODE", descKey: "settings.env.runnerMode" },
      {
        key: "CICD_RUNNER_REGISTRATION_TOKEN",
        descKey: "settings.env.runnerRegistrationToken",
      },
      {
        key: "CICD_RUNNER_CREDENTIAL",
        descKey: "settings.env.runnerCredential",
      },
      { key: "CICD_RUNNER_NAME", descKey: "settings.env.runnerName" },
      { key: "CICD_RUNNER_TAGS", descKey: "settings.env.runnerTags" },
      {
        key: "CICD_RUNNER_TOTAL_SLOTS",
        descKey: "settings.env.runnerTotalSlots",
      },
      {
        key: "CICD_RUNNER_POLL_INTERVAL_SECONDS",
        descKey: "settings.env.runnerPollIntervalSeconds",
      },
      {
        key: "CICD_RUNNER_NO_CHECKOUT",
        descKey: "settings.env.runnerNoCheckout",
      },
      { key: "CICD_RUNNER_WORK_DIR", descKey: "settings.env.runnerWorkDir" },
      {
        key: "CICD_RUNNER_KEEP_WORKSPACE",
        descKey: "settings.env.runnerKeepWorkspace",
      },
    ],
  },
];

export function SettingsPage() {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const query = search.trim().toLocaleLowerCase();
  const filteredGroups = configGroups
    .map((group) => ({
      ...group,
      vars: group.vars.filter(
        (variable) =>
          variable.key.toLocaleLowerCase().includes(query) ||
          t(variable.descKey).toLocaleLowerCase().includes(query),
      ),
    }))
    .filter((group) => group.vars.length > 0);

  return (
    <div className="space-y-5">
      <PageHeader
        title="Конфигурация сервера"
        leading={
          <>
            <Settings className="h-5 w-5 shrink-0 text-accent" aria-hidden />
          </>
        }
        actions={
          <>
            <TextLink asChild>
              <Link className="block" to="/delivery-configs">
                Непривязанные CI-конфигурации
              </Link>
            </TextLink>
          </>
        }
      />
      <p className="text-sm text-text-muted">{t("settings.description")}</p>

      <section aria-labelledby="defaults-heading">
        <SectionHeader
          headingId="defaults-heading"
          className="mb-2"
          title={<>{t("settings.defaults")}</>}
        />
        <dl className="grid grid-cols-2 border-y border-border text-sm sm:grid-cols-3">
          <DescriptionItem
            className="py-3 pr-3"
            label={<>{t("settings.apiPort")}</>}
            technical
          >
            :22801
          </DescriptionItem>
          <DescriptionItem
            className="border-l border-border px-3 py-3"
            label={<>{t("settings.dashboardPort")}</>}
            technical
          >
            :22802
          </DescriptionItem>
          <DescriptionItem
            className="border-t border-border py-3 pr-3 sm:border-l sm:border-t-0 sm:px-3"
            label={<>{t("settings.database")}</>}
            technical
          >
            PostgreSQL 17
          </DescriptionItem>
        </dl>
        <p className="mt-2 text-xs text-text-muted">
          {t("settings.defaultsNote")}
        </p>
      </section>

      <section aria-labelledby="variables-heading">
        <SectionHeader
          headingId="variables-heading"
          className="mb-3"
          title={<>{t("settings.envVars")}</>}
          actions={
            <>
              <div className="w-full sm:w-72">
                <SearchInput
                  aria-label={t("settings.search")}
                  placeholder={t("settings.search")}
                  value={search}
                  onChange={(event) => setSearch(event.target.value)}
                  className="w-full"
                  clear={
                    search
                      ? {
                          label: t("settings.clearSearch"),
                          onClear: () => setSearch(""),
                        }
                      : undefined
                  }
                />
              </div>
            </>
          }
        />

        {filteredGroups.length === 0 ? (
          <div role="status">
            <EmptyState message={t("settings.noMatches")} />
          </div>
        ) : (
          filteredGroups.map((group) => (
            <div key={group.titleKey} className="mb-5">
              <SectionHeader
                level={3}
                className="mb-1"
                title={<>{t(group.titleKey)}</>}
              />
              <dl className="divide-y divide-border border-y border-border">
                {group.vars.map((variable) => (
                  <DescriptionItem
                    key={variable.key}
                    label={<code className="font-mono">{variable.key}</code>}
                    layout="inline"
                  >
                    {t(variable.descKey)}
                  </DescriptionItem>
                ))}
              </dl>
            </div>
          ))
        )}
      </section>
    </div>
  );
}
