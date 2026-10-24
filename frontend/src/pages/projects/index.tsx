import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { ResourceListTitle,
  PageNavigation,
  PageHeader,
  Input,
  ProjectAvatar,
  NamespaceLink as Link,
  ResourceList,
  ResourceListRow,
  LoadingState,
  ErrorState,
} from "@pdlc/ui/ui";
import { withNamespaceLocation } from "@pdlc/ui/lib";
import { api } from "@/api/client";
import type { components } from "@/api/schema";
import { workspacePath, isUnarchivedWorkspace } from "@/api/workspaces";
import { WorkspaceLinks } from "@/widgets/workspace-links";
export function ProjectsPage() {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const [offset, setOffset] = useState(0);
  const catalog = useQuery({
    queryKey: ["workspace-projects", search, offset],
    select: (page: components["schemas"]["WorkspacePage"]) => ({
      ...page,
      items: page.items.filter(isUnarchivedWorkspace),
    }),
    queryFn: ({ signal }) =>
      api<components["schemas"]["WorkspacePage"]>(
        `/workspace-projects?limit=50&offset=${offset}&search=${encodeURIComponent(search)}`,
        { signal },
      ),
  });
  return (
    <div className="space-y-5">
      <PageHeader
        title="Проекты"
        actions={
          <>
            <WorkspaceLinks />
          </>
        }
      />
      <Input
        aria-label="Поиск проектов"
        placeholder="Поиск по имени или ключу…"
        value={search}
        onChange={(event) => {
          setSearch(event.target.value);
          setOffset(0);
        }}
        className="max-w-md"
      />
      {catalog.isPending ? (
        <LoadingState message="Загружаем проекты…" />
      ) : catalog.isError ? (
        <ErrorState
          message={"Каталог проектов недоступен: " + catalog.error.message}
        />
      ) : (
        <>
          <ResourceList asChild variant="bordered" density="comfortable">
            <ul>
              {catalog.data.items.map((project) => (
                <li
                  key={`${project.registry_instance_id}/${project.namespace_id}`}
                >
                  <ResourceListRow asChild interactive>
                    <Link
                      className="flex flex-wrap items-center gap-3"
                      to={withNamespaceLocation(
                        workspacePath(project) + "?project_scope=all",
                        project,
                      )}
                    >
                      <ProjectAvatar
                        projectKey={project.project_key}
                        size="md"
                      />
                      <div className="min-w-0 flex-1">
                        <ResourceListTitle asChild><span >{project.name}</span></ResourceListTitle>
                        <p className="text-sm text-text-muted">
                          {project.project_key} ·{" "}
                          {project.state === "archived"
                            ? "В архиве"
                            : "Активен"}
                          {project.stale ? " · Метаданные устарели" : ""}
                        </p>
                      </div>
                      <span className="text-sm text-text-muted">
                        {project.group_id
                          ? `Репозитории: ${project.repositories} · Git-группа ${project.group_slug}`
                          : "Git не подключён"}
                      </span>
                      <span className="text-sm">
                        {project.latest_status
                          ? t(`pipelines.${project.latest_status}`, {
                              defaultValue: project.latest_status,
                            })
                          : "Нет запусков"}
                      </span>
                    </Link>
                  </ResourceListRow>
                </li>
              ))}
            </ul>
          </ResourceList>
          {!catalog.data.total && (
            <p className="text-text-muted">
              Подключённых проектов пока нет. Создайте проект или подключите
              существующий через Admin.
            </p>
          )}
          <p className="text-sm text-text-muted">
            Показано: {catalog.data.items.length}
          </p>
        </>
      )}
      <PageNavigation
        page={Math.ceil(offset / 50) + 1}
        hasNext={!!catalog.data && offset + 50 < catalog.data.total}
        onPageChange={(nextPage) =>
          setOffset(
            nextPage < Math.ceil(offset / 50) + 1
              ? Math.max(0, offset - 50)
              : offset + 50,
          )
        }
        label="Страницы проектов"
        previousLabel="Назад"
        nextLabel="Далее"
      />
    </div>
  );
}
