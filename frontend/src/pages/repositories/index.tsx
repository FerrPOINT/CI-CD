import { useApplicationAccess } from "@pdlc/ui/auth";
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  PageNavigation,
  PageHeader,
  CardTitle,
  Card,
  CardContent,
  Input,
  NamespaceLink as Link,
  LoadingState,
  ErrorState,
  TextLink,
} from "@pdlc/ui/ui";
import { withNamespaceLocation } from "@pdlc/ui/lib";
import { api } from "@/api/client";
import type { components } from "@/api/schema";
export function RepositoriesPage() {
  const canAdmin = useApplicationAccess()?.permits([], true) === true;
  const [offset, setOffset] = useState(0);
  const [search, setSearch] = useState("");
  const catalog = useQuery({
    queryKey: ["repository-catalog", offset, search],
    queryFn: ({ signal }) =>
      api<components["schemas"]["CatalogPage"]>(
        `/catalog/repositories?limit=50&offset=${offset}&search=${encodeURIComponent(search)}`,
        { signal },
      ),
  });
  return (
    <div className="space-y-5">
      <PageHeader
        title="Репозитории"
        actions={
          <>
            {canAdmin && (
              <TextLink asChild>
                <Link to="/projects?project_scope=all">
                  Добавить репозиторий в проект
                </Link>
              </TextLink>
            )}
          </>
        }
      />
      <Input
        className="max-w-md"
        aria-label="Поиск репозиториев"
        placeholder="Поиск репозиториев…"
        value={search}
        onChange={(event) => {
          setSearch(event.target.value);
          setOffset(0);
        }}
      />
      {catalog.isPending ? (
        <LoadingState message="Загружаем репозитории…" />
      ) : catalog.isError ? (
        <ErrorState message="Каталог недоступен." />
      ) : (
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          {catalog.data.items.map((repo) => (
            <Card asChild interactive key={repo.id}>
              <Link
                to={withNamespaceLocation(
                  `/catalog/repositories/${repo.id}?project_scope=all`,
                  repo.namespace ?? null,
                )}
              >
                <CardContent>
                  <CardTitle asChild>
                    <h2>{repo.public_name}</h2>
                  </CardTitle>
                  <p className="mt-2 text-sm text-text-muted">
                    {repo.kind === "hosted" ? "Git в Forge" : "Внешний Git"} ·{" "}
                    {repo.availability
                      ? "Хранилище недоступно"
                      : !repo.namespace
                        ? "Не подключён к проекту"
                        : repo.state === "archived"
                          ? "В архиве"
                          : repo.ready
                            ? "Подключён"
                            : "Инициализируется"}
                  </p>
                </CardContent>
              </Link>
            </Card>
          ))}
          {!catalog.data.items.length && (
            <p className="text-text-muted">Репозиториев пока нет.</p>
          )}
        </div>
      )}
      <PageNavigation
        page={Math.ceil(offset / 50) + 1}
        hasNext={!!catalog.data && catalog.data.items.length >= 50}
        onPageChange={(nextPage) =>
          setOffset(
            nextPage < Math.ceil(offset / 50) + 1
              ? Math.max(0, offset - 50)
              : offset + 50,
          )
        }
        label="Страницы репозиториев"
        previousLabel="Назад"
        nextLabel="Далее"
      />
    </div>
  );
}
