import { ContentSection } from "@pdlc/ui/ui";
import {
  ActionGroup,
  ResourceListTitle,
  ResourceListDescription,
  TextLink,
  FormField,
} from "@pdlc/ui/ui";
import {
  Notice,
  PageNavigation,
  Checkbox,
  Button,
  toast,
  Input,
  Select,
  ResourceList,
  ResourceListRow,
  ConfirmDialog,
  SearchInput,
  PageHeader,
  SectionHeader,
} from "@pdlc/ui/ui";
import { useState, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useParams, useSearchParams } from "react-router";
import { ChevronDown, Globe, Plus, RotateCcw, Trash2 } from "lucide-react";

import {
  useCreateDeployment,
  useCreateEnvironment,
  useDeleteEnvironment,
  useDeployments,
  useEnvironments,
  useRecordDeploymentApproval,
  useRollbackDeployment,
} from "@/api/hooks";
import type { Deployment, Environment, EnvironmentStatus } from "@/api/types";

import { QueryState } from "@/shared/ui/query-state";

const pageSize = 20;
type StatusFilter = "all" | EnvironmentStatus;
type DeploymentAction = {
  deployment: Deployment;
  kind: "approved" | "rejected" | "rollback";
};

export function EnvironmentsPage() {
  const { t } = useTranslation();
  const { projectId } = useParams();
  const [params, setParams] = useSearchParams();
  const {
    data: environments = [],
    isLoading,
    error,
    refetch,
  } = useEnvironments(projectId);
  const createEnv = useCreateEnvironment(projectId);
  const deleteEnv = useDeleteEnvironment();
  const [showForm, setShowForm] = useState(false);
  const [form, setForm] = useState({
    name: "",
    url: "",
    protected: false,
    required_approvals: 1,
  });
  const [formError, setFormError] = useState<string | null>(null);
  const [pendingEnv, setPendingEnv] = useState<Environment | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const selectedEnvId = params.get("environment_id");
  function setSelectedEnvId(value: string | null) {
    setParams((current) => {
      const next = new URLSearchParams(current);
      if (value) next.set("environment_id", value);
      else next.delete("environment_id");
      next.delete("deployment_id");
      return next;
    });
  }
  const [search, setSearch] = useState("");
  const [status, setStatus] = useState<StatusFilter>("all");
  const [page, setPage] = useState(1);

  const query = search.trim().toLocaleLowerCase();
  const filtered = environments.filter(
    (env) =>
      (status === "all" || env.status === status) &&
      (env.name.toLocaleLowerCase().includes(query) ||
        env.url?.toLocaleLowerCase().includes(query)),
  );
  const totalPages = Math.max(1, Math.ceil(filtered.length / pageSize));
  const currentPage = Math.min(page, totalPages);
  const visiblePage = filtered.slice(
    (currentPage - 1) * pageSize,
    currentPage * pageSize,
  );
  const selectedEnvironment = environments.find(
    (environment) => environment.id === selectedEnvId,
  );
  const visible =
    selectedEnvironment &&
    !visiblePage.some((environment) => environment.id === selectedEnvId)
      ? [selectedEnvironment, ...visiblePage]
      : visiblePage;

  function createEnvironment(event: React.FormEvent) {
    event.preventDefault();
    setFormError(null);
    createEnv.mutate(
      {
        name: form.name.trim(),
        url: form.url.trim() || undefined,
        protected: form.protected,
        required_approvals: form.protected ? form.required_approvals : 0,
      },
      {
        onSuccess: () => {
          setShowForm(false);
          setForm({
            name: "",
            url: "",
            protected: false,
            required_approvals: 1,
          });
          toast.success(t("environments.created"));
        },
        onError: () => setFormError(t("environments.createFailed")),
      },
    );
  }

  function deleteEnvironment() {
    if (!pendingEnv) return;
    setDeleteError(null);
    deleteEnv.mutate(pendingEnv.id, {
      onSuccess: () => {
        if (selectedEnvId === pendingEnv.id) setSelectedEnvId(null);
        setPendingEnv(null);
        toast.success(t("environments.deleted"));
      },
      onError: () => setDeleteError(t("environments.deleteFailed")),
    });
  }

  return (
    <div className="min-w-0 space-y-5">
      <PageHeader
        title={t("environments.title")}
        leading={<Globe className="h-5 w-5 text-accent" aria-hidden />}
        actions={
          <Button
            type="button"
            size="sm"

            aria-expanded={showForm}
            onClick={() => {
              setShowForm(!showForm);
              setFormError(null);
            }}
          >
            <Plus className="h-4 w-4" aria-hidden />
            {t("environments.create")}
          </Button>
        }
      />

      {showForm && (
        <ContentSection asChild dividers="both">
          <form
            onSubmit={createEnvironment}
            className="grid gap-3 sm:grid-cols-2"
          >
            <FormField id="env-name" label={t("environments.name")} required>
              {(attrs) => (
                <Input
                  {...attrs}
                  value={form.name}
                  onChange={(event) =>
                    setForm({ ...form, name: event.target.value })
                  }
                />
              )}
            </FormField>
            <FormField id="env-url" label={t("environments.url")}>
              {(attrs) => (
                <Input
                  {...attrs}
                  type="url"
                  placeholder="https://app.example.com"
                  value={form.url}
                  onChange={(event) =>
                    setForm({ ...form, url: event.target.value })
                  }
                />
              )}
            </FormField>
            <Checkbox
              checked={form.protected}
              onCheckedChange={(checked) =>
                setForm({ ...form, protected: checked === true })
              }
            >
              {t("environments.protected")}
            </Checkbox>
            {form.protected && (
              <FormField
                id="env-required-approvals"
                label={t("environments.requiredApprovals")}
                required
              >
                {(attrs) => (
                  <Input
                    {...attrs}
                    type="number"
                    min={1}
                    max={10}
                    value={form.required_approvals}
                    onChange={(event) =>
                      setForm({
                        ...form,
                        required_approvals: Number(event.target.value) || 1,
                      })
                    }
                  />
                )}
              </FormField>
            )}
            {formError && (
              <Notice role="alert" tone="danger" appearance="inline">
                {formError}
              </Notice>
            )}
            <ActionGroup align="start" className="sm:col-span-2">
              <Button type="submit" disabled={createEnv.isPending}>
                {t("environments.create")}
              </Button>
              <Button
                type="button"
                variant="outline"
                onClick={() => {
                  setShowForm(false);
                  setFormError(null);
                }}
              >
                {t("common.cancel")}
              </Button>
            </ActionGroup>
          </form>
        </ContentSection>
      )}

      <QueryState
        data={environments}
        isLoading={isLoading}
        error={error}
        errorMessage={t("environments.loadFailed")}
        isEmpty={(list) => list.length === 0}
        empty={{ title: t("environments.empty") }}
        onRetry={() => void refetch()}
      >
        {() => (
          <section aria-label={t("environments.title")} className="space-y-3">
            <div className="flex flex-wrap gap-2">
              <div className="min-w-0 flex-1 sm:max-w-xl">
                <SearchInput
                  aria-label={t("environments.search")}
                  placeholder={t("environments.search")}
                  value={search}
                  onChange={(event) => {
                    setSearch(event.target.value);
                    setPage(1);
                  }}
                />
              </div>
              <Select
                aria-label={t("environments.statusFilter")}
                className="w-auto"
                value={status}
                onValueChange={(nextValue) => {
                  setStatus(nextValue as StatusFilter);
                  setPage(1);
                }}
                options={[
                  { value: "all", label: String(t("environments.all")) },
                  {
                    value: "available",
                    label: String(t("environments.statusAvailable")),
                  },
                  {
                    value: "stopped",
                    label: String(t("environments.statusStopped")),
                  },
                  {
                    value: "degraded",
                    label: String(t("environments.statusDegraded")),
                  },
                ]}
              />
            </div>
            <p className="text-xs text-text-muted">
              {t("environments.shown", {
                count: visible.length,
                total: filtered.length,
              })}
            </p>
            {filtered.length === 0 ? (
              <Notice tone="neutral" appearance="inline" role="status">
                {t("environments.noMatches")}
              </Notice>
            ) : (
              <ResourceList asChild>
                <ul>
                  {visible.map((env) => {
                    const expanded = selectedEnvId === env.id;
                    const safeUrl = externalUrl(env.url);
                    return (
                      <ResourceListRow asChild key={env.id}>
                        <li className="min-w-0">
                          <div className="flex min-w-0 items-start gap-2">
                            <Globe
                              className="mt-1 h-4 w-4 shrink-0 text-accent"
                              aria-hidden
                            />
                            <div className="min-w-0 flex-1">
                              <ResourceListTitle asChild className="break-all">
                                <p>{env.name}</p>
                              </ResourceListTitle>
                              <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-text-secondary">
                                <span className="inline-flex items-center gap-1">
                                  <span
                                    aria-hidden
                                    className={`h-2 w-2 rounded-full ${env.status === "available" ? "bg-success" : env.status === "degraded" ? "bg-warning" : "bg-text-muted"}`}
                                  />
                                  {t(
                                    "environments.status" +
                                      capitalize(env.status),
                                  )}
                                </span>
                                {env.protected && (
                                  <span>
                                    {t("environments.protected")} ·{" "}
                                    {env.required_approvals}
                                  </span>
                                )}
                              </div>
                              {env.url &&
                                (safeUrl ? (
                                  <TextLink
                                    href={safeUrl}
                                    target="_blank"
                                    rel="noopener noreferrer"
                                    className="block w-fit max-w-full truncate text-xs"
                                    title={env.url}
                                  >
                                    {env.url}
                                  </TextLink>
                                ) : (
                                  <ResourceListDescription
                                    asChild
                                    className="block max-w-full truncate"
                                  >
                                    <span title={env.url}>{env.url}</span>
                                  </ResourceListDescription>
                                ))}
                            </div>
                            <Button
                              variant="ghost"
                              type="button"
                              aria-label={t("environments.deploymentsFor", {
                                name: env.name,
                              })}
                              title={t("environments.deploymentsFor", {
                                name: env.name,
                              })}
                              aria-expanded={expanded}
                              aria-controls={`deployments-${env.id}`}
                              size="icon"
                              className="shrink-0"
                              onClick={() =>
                                setSelectedEnvId(expanded ? null : env.id)
                              }
                            >
                              <ChevronDown
                                className={`h-4 w-4 transition-transform ${expanded ? "rotate-180" : ""}`}
                                aria-hidden
                              />
                            </Button>
                            <Button
                              size="icon"
                              variant="destructive-outline"
                              type="button"
                              aria-label={t("environments.deleteFor", {
                                name: env.name,
                              })}
                              title={t("environments.deleteFor", {
                                name: env.name,
                              })}
                              className="shrink-0"
                              onClick={() => {
                                setPendingEnv(env);
                                setDeleteError(null);
                              }}
                            >
                              <Trash2 className="h-4 w-4" aria-hidden />
                            </Button>
                          </div>
                          {expanded && (
                            <ContentSection
                              asChild
                              dividers="start"
                              density="compact"
                            >
                              <div
                                id={`deployments-${env.id}`}
                                className="ml-6 mt-3"
                              >
                                <DeploymentsSection environmentId={env.id} />
                              </div>
                            </ContentSection>
                          )}
                        </li>
                      </ResourceListRow>
                    );
                  })}
                </ul>
              </ResourceList>
            )}
            {filtered.length > pageSize && (
              <PageNavigation
                label={t("environments.pages")}
                page={currentPage}
                pageCount={totalPages}
                previousLabel={t("environments.previous")}
                nextLabel={t("environments.next")}
                onPageChange={setPage}
              />
            )}
          </section>
        )}
      </QueryState>

      {!isLoading && !error && selectedEnvId && !selectedEnvironment && (
        <Notice role="alert" tone="danger" appearance="inline">
          Указанное окружение недоступно в этой CI-конфигурации.
        </Notice>
      )}
      <ConfirmDialog
        open={pendingEnv !== null}
        title={
          pendingEnv
            ? t("environments.deleteConfirmFor", { name: pendingEnv.name })
            : ""
        }
        description={t("environments.deleteWarning")}
        error={deleteError ?? undefined}
        closeOnConfirm={false}
        isPending={deleteEnv.isPending}
        onOpenChange={(open) => {
          if (open) return;
          setPendingEnv(null);
          setDeleteError(null);
        }}
        onConfirm={deleteEnvironment}
        confirmLabel={t("common.delete")}
        pendingLabel={t("common.delete")}
      />
    </div>
  );
}

function DeploymentsSection({ environmentId }: { environmentId: string }) {
  const { t } = useTranslation();
  const [params] = useSearchParams();
  const requestedDeployment = params.get("deployment_id");
  const selectedRowRef = useRef<HTMLLIElement>(null);
  const {
    data: deployments = [],
    isLoading,
    error,
    refetch,
  } = useDeployments(environmentId);
  const createDep = useCreateDeployment(environmentId);
  const recordApproval = useRecordDeploymentApproval(environmentId);
  const rollbackDeployment = useRollbackDeployment(environmentId);
  const [gitRef, setGitRef] = useState("");
  const [formError, setFormError] = useState<string | null>(null);
  const [action, setAction] = useState<DeploymentAction | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [page, setPage] = useState(1);
  const filtered = deployments.filter((deployment) =>
    deployment.git_ref
      .toLocaleLowerCase()
      .includes(search.trim().toLocaleLowerCase()),
  );
  const totalPages = Math.max(1, Math.ceil(filtered.length / pageSize));
  const currentPage = Math.min(page, totalPages);
  const visiblePage = filtered.slice(
    (currentPage - 1) * pageSize,
    currentPage * pageSize,
  );
  const selectedDeployment = deployments.find(
    (deployment) => deployment.id === requestedDeployment,
  );
  const visible =
    selectedDeployment &&
    !visiblePage.some((deployment) => deployment.id === requestedDeployment)
      ? [selectedDeployment, ...visiblePage]
      : visiblePage;
  const selectedId = selectedDeployment?.id;
  useEffect(() => {
    if (selectedId) {
      selectedRowRef.current?.focus({ preventScroll: true });
      selectedRowRef.current?.scrollIntoView({ block: "nearest" });
    }
  }, [selectedId]);
  const actionPending =
    recordApproval.isPending || rollbackDeployment.isPending;

  function createDeployment(event: React.FormEvent) {
    event.preventDefault();
    setFormError(null);
    createDep.mutate(
      { git_ref: gitRef.trim() },
      {
        onSuccess: () => {
          setGitRef("");
          toast.success(t("deployments.created"));
        },
        onError: () => setFormError(t("deployments.createFailed")),
      },
    );
  }

  function confirmAction() {
    if (!action) return;
    setActionError(null);
    const onSuccess = () => {
      toast.success(
        t(
          action.kind === "rollback"
            ? "deployments.rollbackCreated"
            : "deployments.approvalRecorded",
        ),
      );
      setAction(null);
    };
    const onError = () => setActionError(t("deployments.actionFailed"));
    if (action.kind === "rollback")
      rollbackDeployment.mutate(
        { deploymentId: action.deployment.id },
        { onSuccess, onError },
      );
    else
      recordApproval.mutate(
        { deploymentId: action.deployment.id, decision: action.kind },
        { onSuccess, onError },
      );
  }

  return (
    <section
      aria-label={t("environments.deployments")}
      className="min-w-0 space-y-3"
    >
      <SectionHeader title={t("environments.deployments")} />
      {!isLoading && !error && requestedDeployment && !selectedDeployment && (
        <Notice role="alert" tone="danger" appearance="inline">
          Указанный деплой не найден в выбранном окружении.
        </Notice>
      )}
      <form
        onSubmit={createDeployment}
        className="flex flex-wrap items-end gap-2"
      >
        <div className="min-w-0 flex-1 sm:max-w-xs">
          <FormField
            id={`git-ref-${environmentId}`}
            label={t("deployments.gitRef")}
            required
          >
            {(attrs) => (
              <Input
                {...attrs}
                placeholder="main"
                value={gitRef}
                onChange={(event) => setGitRef(event.target.value)}
              />
            )}
          </FormField>
        </div>
        <Button
          type="submit"
          size="sm"

          disabled={createDep.isPending}
        >
          {t("deployments.create")}
        </Button>
      </form>
      {formError && (
        <Notice role="alert" tone="danger" appearance="inline">
          {formError}
        </Notice>
      )}
      <QueryState
        data={deployments}
        isLoading={isLoading}
        error={error}
        errorMessage={t("deployments.loadFailed")}
        isEmpty={(list) => list.length === 0}
        empty={{ title: t("deployments.empty") }}
        onRetry={() => void refetch()}
      >
        {() => (
          <div className="space-y-2">
            {deployments.length > pageSize && (
              <SearchInput
                aria-label={t("deployments.search")}
                placeholder={t("deployments.search")}
                containerClassName="sm:max-w-xs"
                value={search}
                onChange={(event) => {
                  setSearch(event.target.value);
                  setPage(1);
                }}
              />
            )}
            {filtered.length === 0 ? (
              <Notice role="status" tone="neutral" appearance="inline">
                {t("deployments.noMatches")}
              </Notice>
            ) : (
              <ResourceList asChild>
                <ul>
                  {visible.map((deployment) => (
                    <ResourceListRow asChild key={deployment.id}>
                      <li
                        ref={
                          deployment.id === selectedId
                            ? selectedRowRef
                            : undefined
                        }
                        tabIndex={deployment.id === selectedId ? -1 : undefined}
                        aria-current={
                          deployment.id === selectedId ? "true" : undefined
                        }
                        className={`flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1  text-xs ${deployment.id === selectedId ? "rounded-md bg-surface-raised  outline-2 outline-accent" : ""}`}
                      >
                        <div className="min-w-0 flex-1 basis-40">
                          <ResourceListTitle asChild>
                            <p>
                              Деплой #{deployment.id.slice(0, 8)}
                              {deployment.id === selectedId ? " · Выбран" : ""}
                            </p>
                          </ResourceListTitle>
                          <ResourceListTitle asChild technical>
                            <p className="break-all">{deployment.git_ref}</p>
                          </ResourceListTitle>
                          <ResourceListDescription asChild>
                            <p>
                              {t(
                                "deployments.status" +
                                  capitalize(deployment.status),
                              )}{" "}
                              · {approvalLabel(deployment, t)}
                            </p>
                          </ResourceListDescription>
                          <ResourceListDescription asChild>
                            <p>
                              <time
                                dateTime={deployment.created_at}
                                title={new Date(
                                  deployment.created_at,
                                ).toLocaleString()}
                              >
                                {formatDate(deployment.created_at)}
                              </time>{" "}
                              ·{" "}
                              <span
                                title={
                                  deployment.rollback_of_id ??
                                  deployment.pipeline_id ??
                                  undefined
                                }
                              >
                                {traceLabel(deployment, t)}
                              </span>
                            </p>
                          </ResourceListDescription>
                        </div>
                        <ActionGroup align="start">
                          {deployment.approval_required &&
                            deployment.approval_state === "pending" && (
                              <>
                                <Button
                                  type="button"
                                  size="sm"
                                  variant="outline"
                                  aria-label={t("deployments.approveFor", {
                                    ref: deployment.git_ref,
                                  })}

                                  onClick={() => {
                                    setAction({ deployment, kind: "approved" });
                                    setActionError(null);
                                  }}
                                >
                                  {t("deployments.approve")}
                                </Button>
                                <Button
                                  type="button"
                                  size="sm"
                                  variant="outline"
                                  aria-label={t("deployments.rejectFor", {
                                    ref: deployment.git_ref,
                                  })}

                                  onClick={() => {
                                    setAction({ deployment, kind: "rejected" });
                                    setActionError(null);
                                  }}
                                >
                                  {t("deployments.reject")}
                                </Button>
                              </>
                            )}
                          {deployment.status === "success" && (
                            <Button
                              type="button"
                              size="sm"
                              variant="outline"
                              aria-label={t("deployments.rollbackFor", {
                                ref: deployment.git_ref,
                              })}

                              onClick={() => {
                                setAction({ deployment, kind: "rollback" });
                                setActionError(null);
                              }}
                            >
                              <RotateCcw className="h-4 w-4" aria-hidden />
                              {t("deployments.rollback")}
                            </Button>
                          )}
                        </ActionGroup>
                      </li>
                    </ResourceListRow>
                  ))}
                </ul>
              </ResourceList>
            )}
            {filtered.length > pageSize && (
              <PageNavigation
                label={t("deployments.pages")}
                page={currentPage}
                pageCount={totalPages}
                previousLabel={t("deployments.previous")}
                nextLabel={t("deployments.next")}
                onPageChange={setPage}
              />
            )}
          </div>
        )}
      </QueryState>
      <ConfirmDialog
        open={action !== null}
        title={
          action
            ? t("deployments.confirm" + capitalize(action.kind), {
                ref: action.deployment.git_ref,
              })
            : ""
        }
        description={t("deployments.actionWarning")}
        error={actionError ?? undefined}
        confirmLabel={
          action
            ? t(
                action.kind === "rollback"
                  ? "deployments.rollback"
                  : "deployments." +
                      (action.kind === "approved" ? "approve" : "reject"),
              )
            : ""
        }
        closeOnConfirm={false}
        isPending={actionPending}
        onOpenChange={(open) => {
          if (open) return;
          setAction(null);
          setActionError(null);
        }}
        onConfirm={confirmAction}
        pendingLabel={
          action
            ? t(
                action.kind === "rollback"
                  ? "deployments.rollback"
                  : "deployments." +
                      (action.kind === "approved" ? "approve" : "reject"),
              )
            : ""
        }
      />
    </section>
  );
}

function externalUrl(value: string | null): string | null {
  if (!value) return null;
  try {
    const url = new URL(value);
    return url.protocol === "https:" || url.protocol === "http:"
      ? url.href
      : null;
  } catch {
    return null;
  }
}

function capitalize(value: string): string {
  return value[0].toUpperCase() + value.slice(1);
}

function formatDate(value: string): string {
  return new Date(value).toLocaleString(undefined, {
    dateStyle: "short",
    timeStyle: "short",
  });
}

function approvalLabel(
  deployment: Deployment,
  t: (key: string) => string,
): string {
  if (!deployment.approval_required)
    return t("deployments.approvalNotRequired");
  if (deployment.approval_state === "approved")
    return `${t("deployments.approved")} ${deployment.approval_count}/${deployment.required_approvals}`;
  if (deployment.approval_state === "rejected")
    return t("deployments.rejected");
  return `${t("deployments.approvalPending")} ${deployment.approval_count}/${deployment.required_approvals}`;
}

function traceLabel(
  deployment: Deployment,
  t: (key: string) => string,
): string {
  if (deployment.rollback_of_id)
    return `${t("deployments.rollbackOf")} ${deployment.rollback_of_id.slice(0, 8)}`;
  return deployment.pipeline_id
    ? `${t("deployments.pipeline")} ${deployment.pipeline_id.slice(0, 8)}`
    : "—";
}
