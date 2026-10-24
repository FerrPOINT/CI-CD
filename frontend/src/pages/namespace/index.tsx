import {
  PageNavigation,
  PageHeader,
  CardTitle,
  Card,
  CardContent,
  LoadingState,
  NamespaceLink as Link,
  Button,
  Input,
  Select,
  Dialog,
  DialogContent,
  DialogTitle,
  ErrorState,
  Notice,
  TextLink,
  FormField,
} from "@pdlc/ui/ui";
import { useState, type FormEvent } from "react";
import { useSearchParams } from "react-router";

import { useQuery, useQueryClient } from "@tanstack/react-query";

import { useSessionCommand, withNamespaceLocation } from "@pdlc/ui/lib";
import { api } from "@/api/client";
import { catalogErrorMessage } from "@/api/catalog-errors";
import { useAuth } from "@/shared/auth/auth-provider";
import type { components } from "@/api/schema";
import { useNamespaceContext } from "@/widgets/namespace-context";
import { AttachRepository } from "./attach-repository";
type CreateRepository = components["schemas"]["CreateRepository"];
export function NamespacePage({ embedded = false }: { embedded?: boolean }) {
  const { ref, malformed, query } = useNamespaceContext();
  const resource = query.data;
  const cache = useQueryClient();
  const { session } = useAuth();
  const [params, setParams] = useSearchParams();
  const offset = Math.max(0, Number(params.get("repository_offset")) || 0);
  const search = params.get("repository_search") ?? "";
  function setOffset(value: number) {
    setParams((current) => {
      const next = new URLSearchParams(current);
      if (value) next.set("repository_offset", String(value));
      else next.delete("repository_offset");
      return next;
    });
  }
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [kind, setKind] = useState<"hosted" | "external" | "existing">(
    "hosted",
  );
  const journal = useSessionCommand<CreateRepository>(
    `forge:repository-create:${session?.subject}:${ref?.registry_instance_id}:${ref?.namespace_id}`,
  );
  const [adding, setAdding] = useState(() => Boolean(journal.command));
  const effectiveKind = journal.command?.kind ?? kind;
  const repositories = useQuery({
    queryKey: [
      "namespace-repositories",
      ref?.registry_instance_id,
      ref?.namespace_id,
      resource?.binding.resource.resource_id,
      offset,
      search,
    ],
    enabled: Boolean(resource),
    queryFn: ({ signal }) =>
      api<components["schemas"]["CatalogPage"]>(
        `/git-groups/${resource!.binding.resource.resource_id}/repositories?limit=50&offset=${offset}&search=${encodeURIComponent(search)}`,
        { signal },
      ),
  });
  async function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    if (!session?.subject) {
      setError("Для создания требуется действующая SSO-сессия");
      return;
    }
    const data = new FormData(form);
    const command = journal.capture({
      id: crypto.randomUUID(),
      slug: String(data.get("slug")),
      kind,
      visibility: String(data.get("visibility")),
      external_url:
        kind === "external" ? String(data.get("external_url")) : null,
      provider_refs: {},
    });
    setBusy(true);
    setError(undefined);
    try {
      await api(
        `/git-groups/${resource!.binding.resource.resource_id}/repositories`,
        {
          method: "POST",
          body: JSON.stringify(command),
        },
      );
      journal.clear();
      form.reset();
      setAdding(false);
      await cache.invalidateQueries({
        queryKey: [
          "namespace-repositories",
          ref?.registry_instance_id,
          ref?.namespace_id,
        ],
      });
      await cache.invalidateQueries({
        queryKey: [
          "workspace-summary",
          ref?.registry_instance_id,
          ref?.namespace_id,
        ],
      });
    } catch (e) {
      setError(catalogErrorMessage(e));
    } finally {
      setBusy(false);
    }
  }
  if (malformed) return <ErrorState message="Некорректная ссылка на проект." />;
  if (!ref)
    return (
      <div className="space-y-3">
        <PageHeader title="Репозитории проекта" />
        <p>Выберите проект в верхней панели.</p>
        <TextLink asChild>
          <Link to="/repositories">Каталог Forge</Link>
        </TextLink>
      </div>
    );
  if (query.isPending)
    return <LoadingState message="Разрешаем привязку Git-группы…" />;
  if (query.isError || !resource)
    return (
      <ErrorState message="Привязка Git-группы недоступна. Другая группа не выбрана." />
    );
  return (
    <div className="space-y-5">
      {!embedded && <PageHeader title="Репозитории проекта" />}
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-sm text-text-muted">
          {resource.binding.state === "archived"
            ? "Проект в архиве. Push, merge и новые pipeline закрыты."
            : "Репозитории проекта"}
        </p>
        {resource.binding.state === "active" && (
          <Button type="button" onClick={() => setAdding(true)}>
            {journal.command ? "Проверить создание" : "Добавить репозиторий"}
          </Button>
        )}
      </div>
      <Input
        aria-label="Поиск репозиториев проекта"
        placeholder="Поиск репозиториев…"
        value={search}
        onChange={(event) => {
          setParams(
            (current) => {
              const next = new URLSearchParams(current);
              if (event.target.value)
                next.set("repository_search", event.target.value);
              else next.delete("repository_search");
              next.delete("repository_offset");
              return next;
            },
            { replace: true },
          );
        }}
        className="max-w-md"
      />
      {repositories.isPending ? (
        <LoadingState message="Загружаем репозитории…" />
      ) : repositories.isError ? (
        <Notice role="alert" tone="danger" appearance="inline">
          Каталог репозиториев недоступен.
        </Notice>
      ) : (
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          {repositories.data?.items.map((repo) => (
            <Card asChild key={repo.id}>
              <article>
                <CardContent className="space-y-3">
                  <CardTitle asChild>
                    <h2>
                      <TextLink asChild>
                        <Link
                          to={withNamespaceLocation(
                            `/catalog/repositories/${repo.id}`,
                            ref,
                          )}
                        >
                          {repo.public_name}
                        </Link>
                      </TextLink>
                    </h2>
                  </CardTitle>
                  <p className="mt-2 text-sm text-text-muted">
                    {repo.kind === "hosted" ? "Git в Forge" : "Внешний Git"} ·{" "}
                    {repo.availability
                      ? "Git-хранилище недоступно"
                      : repo.ready
                        ? "Подключён"
                        : "Инициализируется"}
                  </p>
                  <div className="flex flex-wrap gap-3 text-sm">
                    {[
                      ["pipelines", "Пайплайны"],
                      ["deployments", "Деплои"],
                      ["configurations", "CI-конфигурации"],
                    ].map(([tab, label]) => (
                      <TextLink asChild>
                        <Link
                          key={tab}

                          to={withNamespaceLocation(
                            `/catalog/repositories/${repo.id}?tab=${tab}`,
                            ref,
                          )}
                        >
                          {label}
                        </Link>
                      </TextLink>
                    ))}
                  </div>
                </CardContent>
              </article>
            </Card>
          ))}
          {repositories.data?.items.length === 0 && (
            <div className="space-y-2 text-text-muted">
              <p>
                {search ? "Репозитории не найдены." : "Репозиториев пока нет."}
              </p>
              {search && (
                <Button
                  variant="outline"
                  onClick={() =>
                    setParams((current) => {
                      const next = new URLSearchParams(current);
                      next.delete("repository_search");
                      next.delete("repository_offset");
                      return next;
                    })
                  }
                >
                  Сбросить поиск
                </Button>
              )}
            </div>
          )}
        </div>
      )}
      {repositories.data && (
        <p className="text-sm text-text-muted">
          {repositories.data.total
            ? `${offset + 1}–${offset + repositories.data.items.length} из ${repositories.data.total}`
            : "0 репозиториев"}
        </p>
      )}
      {(repositories.data?.total ?? 0) > 50 && (
        <PageNavigation
          page={Math.ceil(offset / 50) + 1}
          hasNext={offset + 50 < (repositories.data?.total ?? 0)}
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
      )}
      {resource.binding.state === "active" && (
        <Dialog
          open={adding}
          onOpenChange={(open) => {
            if (!busy) setAdding(open);
          }}
        >
          <DialogContent aria-describedby={undefined}>
            <DialogTitle>
              {journal.command
                ? "Проверить создание репозитория"
                : "Добавить репозиторий"}
            </DialogTitle>
            <FormField id="repository-action" label={"Действие"}>
              {(baseFieldAttrs) => (
                <Select
                  {...baseFieldAttrs}
                  value={effectiveKind}
                  disabled={busy || !!journal.command}
                  onValueChange={(nextValue) =>
                    setKind(nextValue as typeof kind)
                  }
                  options={[
                    { value: "hosted", label: "Создать Git в Forge" },
                    { value: "external", label: "Подключить внешний Git" },
                    {
                      value: "existing",
                      label: "Подключить существующий репозиторий Forge",
                    },
                  ]}
                />
              )}
            </FormField>
            {effectiveKind === "existing" ? (
              <AttachRepository
                groupId={resource.binding.resource.resource_id}
                namespace={ref}
                onAttached={() => setAdding(false)}
              />
            ) : (
              <Card asChild className="max-w-xl">
                <form onSubmit={create}>
                  <CardContent className="space-y-4">
                    <FormField id="repo-slug" label={"Имя репозитория"}>
                      {(baseFieldAttrs) => (
                        <Input
                          {...baseFieldAttrs}
                          name="slug"
                          required
                          defaultValue={journal.command?.slug}
                          disabled={busy || !!journal.command}
                        />
                      )}
                    </FormField>
                    {effectiveKind === "external" && (
                      <FormField id="external-url" label={"Git URL"}>
                        {(baseFieldAttrs) => (
                          <Input
                            {...baseFieldAttrs}
                            name="external_url"
                            required
                            defaultValue={journal.command?.external_url ?? ""}
                            disabled={busy || !!journal.command}
                          />
                        )}
                      </FormField>
                    )}
                    <FormField id="visibility" label={"Видимость Git"}>
                      {(baseFieldAttrs) => (
                        <Select
                          {...baseFieldAttrs}
                          name="visibility"
                          defaultValue={
                            journal.command?.visibility ?? "private"
                          }
                          disabled={busy || !!journal.command}
                          options={[
                            { value: "private", label: "По авторизации" },
                            { value: "public", label: "Публичное чтение" },
                          ]}
                        />
                      )}
                    </FormField>
                    {error && (
                      <Notice role="alert" tone="danger" appearance="inline">
                        {error}. Проверьте результат создания, прежде чем
                        продолжить.
                      </Notice>
                    )}
                    <Button disabled={busy}>
                      {busy
                        ? "Добавляем…"
                        : journal.command
                          ? "Проверить создание"
                          : "Добавить"}
                    </Button>
                  </CardContent>
                </form>
              </Card>
            )}
          </DialogContent>
        </Dialog>
      )}
    </div>
  );
}
