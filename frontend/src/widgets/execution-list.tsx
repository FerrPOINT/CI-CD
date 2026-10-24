import { useQuery } from "@tanstack/react-query";
import { useSearchParams } from "react-router";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ResourceListDescription,
  PageNavigation,
  TextLink,
  EmptyState,
  Badge,
  SectionHeader,
  Button,
  NamespaceLink as Link,
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
  ResourceList,
  ResourceListRow,
  Notice,
  LoadingState,
} from "@pdlc/ui/ui";
import { type NamespaceLocation, withNamespaceLocation } from "@pdlc/ui/lib";
import type { components } from "@/api/schema";
import { api, ApiError } from "@/api/client";
import { workspaceKey, workspacePath } from "@/api/workspaces";
import { executionBoundary } from "@/api/execution-filters";
import {
  executionFilterKeys,
  ExecutionFilterForm,
  readExecutionFilters,
  selectedResourceKey,
} from "./execution-filters";

export function ExecutionList({
  namespace,
  repositoryId,
  deployment = false,
  preview = false,
}: {
  namespace?: NamespaceLocation;
  repositoryId?: string;
  deployment?: boolean;
  preview?: boolean;
}) {
  const { t } = useTranslation();
  const [params, setParams] = useSearchParams();
  const [filtersOpen, setFiltersOpen] = useState(false);
  const applied = readExecutionFilters(
    preview ? new URLSearchParams() : params,
  );
  const activeKeys = executionFilterKeys.filter(
    (key) => applied[key] && !(repositoryId && key === "repository_id"),
  );
  const requestedOffset = Number(params.get("offset"));
  const offset =
    preview || !Number.isSafeInteger(requestedOffset)
      ? 0
      : Math.max(0, requestedOffset);
  const pageSize = preview ? 5 : 50;
  const query = new URLSearchParams({
    limit: String(pageSize),
    offset: String(offset),
  });
  for (const key of executionFilterKeys)
    if (applied[key])
      query.set(
        key,
        ["since", "until"].includes(key)
          ? executionBoundary(applied[key], key === "until")
          : applied[key],
      );
  const path = repositoryId
    ? `/catalog/repositories/${repositoryId}/${deployment ? "deployments" : "pipelines"}`
    : namespace
      ? `/workspace-projects/${workspaceKey(namespace)}/${deployment ? "deployments" : "pipelines"}`
      : "/workspace-pipelines";
  const list = useQuery({
    queryKey: [
      "executions",
      namespace?.registry_instance_id,
      namespace?.namespace_id,
      repositoryId,
      deployment,
      query.toString(),
    ],
    queryFn: ({ signal }) =>
      api<components["schemas"]["ExecutionPage"]>(`${path}?${query}`, {
        signal,
      }),
    refetchInterval: (current) =>
      current.state.data?.items.some((row) =>
        ["queued", "running", "pending"].includes(row.status),
      )
        ? 3000
        : false,
  });
  const emptyConfigurations = useQuery({
    queryKey: [
      "execution-empty-configurations",
      namespace?.registry_instance_id,
      namespace?.namespace_id,
      repositoryId,
    ],
    enabled: list.data?.total === 0 && !activeKeys.length,
    queryFn: ({ signal }) => {
      const scope = namespace
        ? `&registry_instance_id=${namespace.registry_instance_id}&namespace_id=${namespace.namespace_id}`
        : "";
      return api<components["schemas"]["ConfigPage"]>(
        `${repositoryId ? `/catalog/repositories/${repositoryId}/delivery-configs` : "/delivery-configurations"}?limit=1&managed=true${scope}`,
        { signal },
      );
    },
  });
  const selectedRepository = useQuery({
    queryKey: selectedResourceKey(
      "repository",
      namespace,
      applied.repository_id,
    ),
    enabled: !!applied.repository_id,
    queryFn: ({ signal }) =>
      api<components["schemas"]["CatalogRepository"]>(
        `/catalog/repositories/${applied.repository_id}`,
        { signal },
      ).then((item) => item.public_name),
  });
  const selectedConfiguration = useQuery({
    queryKey: selectedResourceKey(
      "configuration",
      namespace,
      applied.configuration_id,
    ),
    enabled: !!applied.configuration_id,
    queryFn: ({ signal }) =>
      api<components["schemas"]["DeliveryConfig"]>(
        `/delivery-configurations/${applied.configuration_id}`,
        { signal },
      ).then(
        (item) =>
          `${item.name}${item.repository_name ? ` · ${item.repository_name}` : ""}`,
      ),
  });
  const denied =
    list.error instanceof ApiError && [401, 403].includes(list.error.status);
  const display = denied ? undefined : list.data;
  const statusLabel = (status: string) =>
    t(
      deployment
        ? `deployments.status${status[0].toUpperCase()}${status.slice(1)}`
        : `pipelines.${status}`,
      { defaultValue: status },
    );
  const approvalLabel = (state: string) =>
    ({
      pending: "Ожидает подтверждения",
      approved: "Подтверждено",
      rejected: "Отклонено",
      not_required: "Не требуется",
    })[state] ?? state;
  function reset() {
    setParams((current) => {
      const next = new URLSearchParams(current);
      executionFilterKeys.forEach((key) => next.delete(key));
      next.delete("offset");
      return next;
    });
  }
  const emptyAction = activeKeys.length ? (
    <Button type="button" variant="outline" onClick={reset}>
      Сбросить фильтры
    </Button>
  ) : (
    <Button asChild variant="link">
      <Link
        to={
          repositoryId
            ? `/catalog/repositories/${repositoryId}?tab=configurations`
            : namespace
              ? workspacePath(namespace, "repositories")
              : "/repositories"
        }
      >
        Открыть репозитории и CI-конфигурации
      </Link>
    </Button>
  );
  const errorMessage = denied
    ? "Нет доступа к этому списку."
    : list.error?.message === "invalid_execution_period"
      ? "Дата начала позже даты окончания. Измените период."
      : list.error?.message?.includes("mismatch")
        ? "Выбранные ресурсы не соответствуют проекту. Измените фильтры."
        : "Не удалось загрузить список. Повторите запрос.";
  return (
    <section
      className="min-w-0 space-y-4"
      aria-label={deployment ? "Список деплоев" : "Список пайплайнов"}
      aria-busy={list.isFetching || undefined}
    >
      {!preview && (
        <SectionHeader
          title={<>{deployment ? "Деплои" : "Пайплайны"}</>}
          actions={
            <>
              <Button
                type="button"
                variant="outline"
                onClick={() => setFiltersOpen(true)}
              >
                Фильтры{activeKeys.length ? ` (${activeKeys.length})` : ""}
              </Button>
              <Button
                type="button"
                variant="outline"
                disabled={list.isFetching}
                onClick={() => void list.refetch()}
              >
                Обновить
              </Button>
            </>
          }
        />
      )}
      {!!activeKeys.length && (
        <div className="flex flex-wrap items-center gap-2 text-sm">
          {activeKeys.map((key) => (
            <span
              key={key}
              className="max-w-full break-words rounded-md bg-surface-raised px-2 py-1"
            >
              {key === "repository_id"
                ? (selectedRepository.data ?? "Репозиторий недоступен")
                : key === "configuration_id"
                  ? (selectedConfiguration.data ?? "Конфигурация недоступна")
                  : key === "status"
                    ? statusLabel(applied[key])
                    : key === "approval_state"
                      ? approvalLabel(applied[key])
                      : key === "environment_id"
                        ? "Выбрано окружение"
                        : `${({ git_ref: "Git", since: "С", until: "По" } as Record<string, string>)[key]}: ${applied[key]}`}
            </span>
          ))}
          <Button type="button" variant="ghost" onClick={reset}>
            Сбросить
          </Button>
        </div>
      )}
      <Dialog open={filtersOpen} onOpenChange={setFiltersOpen}>
        <DialogContent className="max-w-2xl">
          <DialogTitle>
            Фильтры {deployment ? "деплоев" : "пайплайнов"}
          </DialogTitle>
          <DialogDescription>
            Изменения применяются после нажатия «Применить».
          </DialogDescription>
          <ExecutionFilterForm
            key={params.toString()}
            params={params}
            namespace={namespace}
            repositoryId={repositoryId}
            deployment={deployment}
            onCancel={() => setFiltersOpen(false)}
            onApply={(draft) => {
              setParams((current) => {
                const next = new URLSearchParams(current);
                executionFilterKeys.forEach((key) => {
                  if (draft[key].trim()) next.set(key, draft[key].trim());
                  else next.delete(key);
                });
                next.delete("offset");
                return next;
              });
              setFiltersOpen(false);
            }}
          />
        </DialogContent>
      </Dialog>
      {list.isError && (
        <Notice
          role="alert"

          tone="danger"
          appearance="panel"
          actions={
            <Button
              type="button"
              variant="outline"
              onClick={() => void list.refetch()}
            >
              Повторить
            </Button>
          }
        >
          <p>
            {display
              ? "Не удалось обновить. Показаны последние загруженные данные."
              : errorMessage}
          </p>
        </Notice>
      )}
      {list.isPending ? (
        <LoadingState message="Загружаем запуски…" />
      ) : (
        display && (
          <>
            {!!display.items.length && (
              <ResourceList asChild variant="bordered" density="comfortable">
                <ul>
                  {display.items.map((row) => {
                    const target = deployment
                      ? `/delivery-configs/${row.configuration_id}/environments?environment_id=${row.environment_id}&deployment_id=${row.id}`
                      : `/pipelines/${row.pipeline_id ?? row.id}`;
                    const tone =
                      row.status === "success"
                        ? "success"
                        : row.status === "failed"
                          ? "danger"
                          : ["running", "queued", "pending"].includes(
                                row.status,
                              )
                            ? "warning"
                            : "muted";
                    const seconds =
                      row.started_at && row.finished_at
                        ? Math.max(
                            0,
                            Math.round(
                              (Date.parse(row.finished_at) -
                                Date.parse(row.started_at)) /
                                1000,
                            ),
                          )
                        : undefined;
                    return (
                      <ResourceListRow asChild key={row.id}>
                        <li className="flex min-w-0 flex-wrap items-start justify-between gap-3">
                          <div className="min-w-0 flex-1">
                            <TextLink asChild>
                              <Link
                                className="font-semibold"
                                to={withNamespaceLocation(
                                  target,
                                  row.namespace,
                                )}
                              >
                                {deployment ? "Деплой" : "Пайплайн"} #
                                {row.id.slice(0, 8)}
                              </Link>
                            </TextLink>
                            <div className="mt-1 flex flex-wrap gap-x-2 text-sm">
                              <TextLink asChild>
                                <Link
                                  to={withNamespaceLocation(
                                    `/catalog/repositories/${row.repository_id}`,
                                    row.namespace,
                                  )}
                                >
                                  {row.repository_name}
                                </Link>
                              </TextLink>
                              <span aria-hidden>·</span>
                              <TextLink asChild>
                                <Link
                                  to={withNamespaceLocation(
                                    `/delivery-configs/${row.configuration_id}/settings`,
                                    row.namespace,
                                  )}
                                >
                                  {row.configuration_name}
                                </Link>
                              </TextLink>
                            </div>
                            <p className="mt-1 break-words text-sm text-text-muted">
                              {row.git_ref} ·{" "}
                              <time dateTime={row.created_at}>
                                {new Date(row.created_at).toLocaleString()}
                              </time>
                              {row.environment_name
                                ? ` · ${row.environment_name}`
                                : ""}
                              {seconds !== undefined ? ` · ${seconds} с` : ""}
                            </p>
                            {row.commit_sha && (
                              <ResourceListDescription asChild className="mt-1"><p >
                                Коммит{" "}
                                <TextLink asChild>
                                  <Link
                                    className="font-mono"
                                    to={withNamespaceLocation(
                                      `/catalog/repositories/${row.repository_id}?tab=files&ref=${encodeURIComponent(row.commit_sha)}`,
                                      row.namespace,
                                    )}
                                  >
                                    {row.commit_sha.slice(0, 8)}
                                  </Link>
                                </TextLink>
                              </p></ResourceListDescription>
                            )}
                          </div>
                          <div className="flex flex-wrap items-center gap-2">
                            <Badge tone={tone}>{statusLabel(row.status)}</Badge>
                            {row.approval_state && (
                              <span className="text-sm text-text-muted">
                                {approvalLabel(row.approval_state)}
                              </span>
                            )}
                            <Button asChild variant="link">
                              <Link
                                to={withNamespaceLocation(
                                  target,
                                  row.namespace,
                                )}
                              >
                                Открыть
                              </Link>
                            </Button>
                            {deployment && row.pipeline_id && (
                              <Button asChild variant="link">
                                <Link
                                  to={withNamespaceLocation(
                                    `/pipelines/${row.pipeline_id}`,
                                    row.namespace,
                                  )}
                                >
                                  Пайплайн
                                </Link>
                              </Button>
                            )}
                          </div>
                        </li>
                      </ResourceListRow>
                    );
                  })}
                </ul>
              </ResourceList>
            )}
            {!display.items.length && (
              <EmptyState
                message={
                  activeKeys.length
                    ? "По выбранным фильтрам ничего не найдено."
                    : emptyConfigurations.data?.total === 0
                      ? "Нет CI-конфигураций."
                      : `${deployment ? "Деплоев" : "Пайплайнов"} пока нет.`
                }
                description={
                  activeKeys.length
                    ? "Измените условия или сбросьте фильтры."
                    : emptyConfigurations.data?.total === 0
                      ? "Создайте CI-конфигурацию в репозитории."
                      : "Запустите настроенную CI-конфигурацию в репозитории."
                }
                action={!preview ? emptyAction : undefined}
              />
            )}
            {!preview && (
              <div className="flex flex-wrap items-center justify-between gap-2">
                <p className="text-sm text-text-muted">
                  {display.total
                    ? `${offset + (display.items.length ? 1 : 0)}–${offset + display.items.length} из ${display.total}`
                    : "0 результатов"}
                </p>
                {display.total > pageSize && (
                  <PageNavigation
                    page={Math.ceil(offset / pageSize) + 1}
                    hasNext={offset + pageSize < display.total}
                    onPageChange={(nextPage) =>
                      setParams((current) => {
                        const next = new URLSearchParams(current);
                        next.set(
                          "offset",
                          String(
                            nextPage < Math.ceil(offset / pageSize) + 1
                              ? Math.max(0, offset - pageSize)
                              : offset + pageSize,
                          ),
                        );
                        return next;
                      })
                    }
                    label="Страницы запусков"
                    previousLabel="Назад"
                    nextLabel="Далее"
                    summary={
                      <>
                        {Math.floor(offset / pageSize) + 1} /{" "}
                        {Math.ceil(display.total / pageSize)}
                      </>
                    }
                  />
                )}
              </div>
            )}
          </>
        )
      )}
    </section>
  );
}
