import { Navigate, useLocation, useParams } from "react-router";
import { useQuery } from "@tanstack/react-query";
import {
  Card,
  CardContent,
  Metric,
  PageHeader,
  SectionHeader,
  NamespaceLink as Link,
  ProjectAvatar,
  SectionNavigation,
  SectionNavigationItem,
  Button,
  ErrorState,
  LoadingState,
  Notice,
  TextLink,
} from "@pdlc/ui/ui";
import { withNamespaceLocation } from "@pdlc/ui/lib";
import { useWorkspace, workspaceKey, workspacePath } from "@/api/workspaces";
import { api } from "@/api/client";
import type { components } from "@/api/schema";
import { useNamespaceRef } from "@/widgets/namespace-context";
import { WorkspaceLinks } from "@/widgets/workspace-links";
import { ExecutionList } from "@/widgets/execution-list";
import { RepositorySyncsPage } from "@/pages/repository-syncs";
import { NamespacePage } from "@/pages/namespace";
export function NamespaceRedirect() {
  const { ref, malformed, all } = useNamespaceRef();
  if (malformed) return <ErrorState message="Некорректная ссылка на проект." />;
  return (
    <Navigate
      replace
      to={
        ref
          ? withNamespaceLocation(
              workspacePath(ref) + (all ? "?project_scope=all" : ""),
              ref,
            )
          : "/projects?project_scope=all"
      }
    />
  );
}
export function LegacyConfigRedirect() {
  const { projectId } = useParams();
  const location = useLocation();
  return (
    <Navigate
      replace
      to={
        location.pathname.replace(
          `/projects/${projectId}`,
          `/delivery-configs/${projectId}`,
        ) + location.search
      }
    />
  );
}
export function WorkspacePage() {
  const { ref, malformed, all } = useNamespaceRef();
  const { "*": section = "" } = useParams();
  const project = useWorkspace(malformed ? null : ref);
  const summary = useQuery({
    queryKey: [
      "workspace-summary",
      ref?.registry_instance_id,
      ref?.namespace_id,
    ],
    enabled: !!ref && !!project.data?.group_id,
    queryFn: ({ signal }) =>
      api<components["schemas"]["WorkspaceSummary"]>(
        `/workspace-projects/${workspaceKey(ref!)}/summary`,
        { signal },
      ),
    refetchInterval: (current) =>
      current.state.data &&
      (current.state.data.queued || current.state.data.running)
        ? 3000
        : false,
  });
  if (malformed || !ref)
    return <ErrorState message="Некорректная ссылка на проект." />;
  if (project.isPending) return <LoadingState message="Загружаем проект…" />;
  if (project.isError)
    return (
      <ErrorState message={"Проект недоступен: " + project.error.message} />
    );
  const data = project.data;
  const sections = [
    ["", "Обзор"],
    ["repositories", "Репозитории"],
    ["syncs", "Синхронизации"],
    ["pipelines", "Пайплайны"],
    ["deployments", "Деплои"],
  ];
  return (
    <div className="space-y-5">
      <TextLink asChild>
        <Link to="/projects?project_scope=all">Все проекты</Link>
      </TextLink>
      <PageHeader
        title={data.name}
        leading={
          <>
            <ProjectAvatar projectKey={data.project_key} size="md" />
          </>
        }
        context={
          <>
            {data.project_key} ·{" "}
            {data.state === "archived" ? "В архиве" : "Активен"} ·{" "}
            {data.group_id
              ? `Git-группа: ${data.group_slug}`
              : "Git не подключён"}
          </>
        }
      />
      {data.stale && (
        <Notice role="status" tone="warning" appearance="inline">
          Показываем сохранённые метаданные Tracker. Существующие репозитории и
          CI доступны.
        </Notice>
      )}
      <WorkspaceLinks namespace={ref} />
      <SectionNavigation aria-label="Разделы проекта">
        {sections.map(([path, label]) => (
          <SectionNavigationItem asChild active={section === path} key={path}>
            <Link
              key={path}
              to={withNamespaceLocation(
                workspacePath(ref, path) + (all ? "?project_scope=all" : ""),
                ref,
              )}
            >
              {label}
            </Link>
          </SectionNavigationItem>
        ))}
      </SectionNavigation>
      {!data.group_id ? (
        <p>
          Git не подключён. Подключите группу репозиториев в настройках проекта
          Admin.
        </p>
      ) : section === "repositories" ? (
        <NamespacePage embedded />
      ) : section === "syncs" ? (
        <RepositorySyncsPage groupId={data.group_id} />
      ) : section === "pipelines" ? (
        <ExecutionList key={workspaceKey(ref) + section} namespace={ref} />
      ) : section === "deployments" ? (
        <ExecutionList
          key={workspaceKey(ref) + section}
          namespace={ref}
          deployment
        />
      ) : section === "" ? (
        <>
          {summary.isError && !summary.data ? (
            <Notice
              role="alert"
              tone="danger"
              appearance="panel"
              actions={
                <Button
                  variant="outline"
                  onClick={() => void summary.refetch()}
                >
                  Повторить
                </Button>
              }
            >
              <p>Сводка недоступна.</p>
            </Notice>
          ) : summary.isPending ? (
            <LoadingState message="Загружаем сводку…" />
          ) : (
            <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-5">
              {[
                ["Репозитории", summary.data.repositories, "repositories"],
                [
                  "CI-конфигурации",
                  summary.data.configurations,
                  "repositories",
                ],
                ["В очереди", summary.data.queued, "pipelines?status=queued"],
                [
                  "Выполняются",
                  summary.data.running,
                  "pipelines?status=running",
                ],
                ["С ошибкой", summary.data.failed, "pipelines?status=failed"],
              ].map(([label, count, target]) => (
                <Card asChild interactive key={label}>
                  <Link
                    to={withNamespaceLocation(
                      workspacePath(ref, String(target)) +
                        (all
                          ? String(target).includes("?")
                            ? "&project_scope=all"
                            : "?project_scope=all"
                          : ""),
                      ref,
                    )}
                  >
                    <CardContent>
                      <Metric appearance="inline" label={label} value={count} />
                    </CardContent>
                  </Link>
                </Card>
              ))}
            </div>
          )}
          <p className="text-sm text-text-muted">
            Сводка по всем репозиториям проекта за всё время.
          </p>
          {summary.isError && summary.data && (
            <Notice role="alert" tone="warning" appearance="inline">
              Не удалось обновить сводку. Показаны последние загруженные данные.
            </Notice>
          )}
          <SectionHeader
            title={<>Последние пайплайны</>}
            actions={
              <>
                <TextLink asChild>
                  <Link
                    to={withNamespaceLocation(
                      workspacePath(ref, "pipelines") +
                        (all ? "?project_scope=all" : ""),
                      ref,
                    )}
                  >
                    Все пайплайны
                  </Link>
                </TextLink>
              </>
            }
          />
          <ExecutionList key={workspaceKey(ref)} namespace={ref} preview />
        </>
      ) : (
        <ErrorState message="Раздел не найден." />
      )}
    </div>
  );
}
