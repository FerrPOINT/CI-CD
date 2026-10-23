import { useApplicationAccess } from "@pdlc/ui/auth";
import { Fragment, type ReactNode } from "react";
import { useParams, useLocation } from "react-router";
import { useQuery } from "@tanstack/react-query";
import {
  NamespaceLink as Link,
  ProjectAvatar,
  SectionNavigation,
  SectionNavigationItem,
  ErrorState,
  LoadingState,
} from "@pdlc/ui/ui";
import { withNamespaceLocation } from "@pdlc/ui/lib";
import { api } from "@/api/client";
import {
  useWorkspace,
  workspaceKey,
  workspacePath,
  type Configuration,
} from "@/api/workspaces";
import { useNamespaceRef } from "./namespace-context";
import type { components } from "@/api/schema";
export function ConfigurationContext({
  children,
  configurationId,
  showSettings = true,
}: {
  children: ReactNode;
  configurationId?: string;
  showSettings?: boolean;
}) {
  const { projectId: routeId } = useParams();
  const location = useLocation();
  const canAdmin = useApplicationAccess()?.permits([], true) === true;
  const projectId = configurationId ?? routeId;
  const { ref, malformed } = useNamespaceRef();
  const config = useQuery({
    queryKey: [
      "delivery-config",
      ref?.registry_instance_id,
      ref?.namespace_id,
      projectId,
    ],
    queryFn: ({ signal }) =>
      api<Configuration>(`/delivery-configurations/${projectId}`, { signal }),
  });
  const repository = useQuery({
    queryKey: [
      "catalog-repository",
      ref?.registry_instance_id,
      ref?.namespace_id,
      config.data?.repository_id,
    ],
    enabled: !!config.data?.repository_id,
    queryFn: ({ signal }) =>
      api<components["schemas"]["CatalogRepository"]>(
        `/catalog/repositories/${config.data!.repository_id}`,
        { signal },
      ),
  });
  const namespace = repository.data?.namespace ?? null;
  const project = useWorkspace(namespace);
  if (
    malformed ||
    (ref &&
      repository.data &&
      (!namespace || workspaceKey(ref) !== workspaceKey(namespace)))
  )
    return <ErrorState message="Конфигурация принадлежит другому проекту." />;
  if (config.isError || repository.isError)
    return <ErrorState message="Конфигурация недоступна." />;
  if (config.isPending || (config.data.repository_id && repository.isPending))
    return <LoadingState message="Загружаем конфигурацию…" />;
  return (
    <div className="space-y-4">
      <nav
        aria-label="Контекст CI-конфигурации"
        className="flex flex-wrap items-center gap-2 text-sm"
      >
        {project.data && (
          <>
            <ProjectAvatar projectKey={project.data.project_key} size="xs" />
            <Link
              className="text-accent"
              to={withNamespaceLocation(
                workspacePath(project.data),
                project.data,
              )}
            >
              {project.data.name} · {project.data.project_key}
            </Link>
            <span>/</span>
          </>
        )}
        {repository.data ? (
          <>
            <Link
              className="text-accent"
              to={withNamespaceLocation(
                `/catalog/repositories/${repository.data.id}`,
                namespace,
              )}
            >
              {repository.data.public_name}
            </Link>
            <span>/</span>
          </>
        ) : (
          <Link className="text-accent" to="/delivery-configs">
            Непривязанные конфигурации /
          </Link>
        )}
        <span>CI: {config.data.name}</span>
      </nav>
      {showSettings && (
        <SectionNavigation aria-label="Разделы CI-конфигурации">
          {[
            ["settings", "Настройки"],
            ["pipelines", "Запуски"],
            ["secrets", "Секреты"],
            ["environments", "Окружения"],
            ["schedules", "Расписания"],
            ["webhooks", "Вебхуки"],
            ["reports", "Отчёты"],
          ]
            .filter(
              ([section]) =>
                canAdmin || ["pipelines", "reports"].includes(section),
            )
            .map(([section, label]) => (
              <SectionNavigationItem
                asChild
                key={section}
                active={
                  location.pathname ===
                  `/delivery-configs/${projectId}/${section}`
                }
              >
                <Link
                  key={section}
                  to={withNamespaceLocation(
                    `/delivery-configs/${projectId}/${section}`,
                    namespace,
                  )}
                >
                  {label}
                </Link>
              </SectionNavigationItem>
            ))}
        </SectionNavigation>
      )}
      <Fragment key={projectId}>{children}</Fragment>
    </div>
  );
}
