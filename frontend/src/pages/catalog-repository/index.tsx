import { useApplicationAccess } from "@pdlc/ui/auth";
import {
  CardContent,
  Card,
  PageHeader,
  SectionHeader,
  CopyButton,
  LoadingState,
  NamespaceLink as Link,
  Button,
  Input,
  Label,
  SectionNavigation,
  SectionNavigationItem,
  Dialog,
  DialogContent,
  DialogTitle,
  ProjectAvatar,
  ErrorState,
  Notice,
  TextLink,
} from "@pdlc/ui/ui";
import { useState, type FormEvent } from "react";

import { useParams, useSearchParams } from "react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { withNamespaceLocation } from "@pdlc/ui/lib";
import { api } from "@/api/client";
import { RepositoryConfigurations } from "@/widgets/repository-configurations";
import { RepositoryBrowserPage } from "@/pages/repository-browser";
import { ExecutionList } from "@/widgets/execution-list";
import { useWorkspace, workspacePath } from "@/api/workspaces";

import type { components } from "@/api/schema";
import { useNamespaceContext } from "@/widgets/namespace-context";
export function CatalogRepositoryPage() {
  const { id } = useParams();
  const canAdmin = useApplicationAccess()?.permits([], true) === true;
  const [params] = useSearchParams();
  const intent =
    params.get("task_id") && params.get("tracker_instance_id")
      ? `?${new URLSearchParams({ task_id: params.get("task_id")!, tracker_instance_id: params.get("tracker_instance_id")! })}`
      : "";
  const { ref, malformed } = useNamespaceContext();
  const cache = useQueryClient();
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [showPullForm, setShowPullForm] = useState(false);
  const tab = params.get("tab") ?? "git";
  function tabLocation(nextTab: string) {
    const next = new URLSearchParams(params);
    next.set("tab", nextTab);
    for (const key of [
      "offset",
      "repository_id",
      "configuration_id",
      "status",
      "git_ref",
      "since",
      "until",
      "environment_id",
      "approval_state",
    ])
      next.delete(key);
    return `/catalog/repositories/${id}?${next}`;
  }
  const repository = useQuery({
    queryKey: [
      "catalog-repository",
      ref?.registry_instance_id,
      ref?.namespace_id,
      id,
    ],
    queryFn: ({ signal }) =>
      api<components["schemas"]["CatalogRepository"]>(
        `/catalog/repositories/${id}`,
        { signal },
      ),
  });
  const repo = repository.data;
  const project = useWorkspace(repo?.namespace ?? ref);
  const pulls = useQuery({
    queryKey: [
      "catalog-pulls",
      ref?.registry_instance_id,
      ref?.namespace_id,
      id,
    ],
    enabled: repo?.kind === "hosted" && repo.ready,
    queryFn: ({ signal }) =>
      api<components["schemas"]["PullRequest"][]>(
        `/catalog/repositories/${id}/pulls`,
        { signal },
      ),
  });
  async function createPull(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const data = new FormData(form);
    setBusy(true);
    setError(undefined);
    try {
      await api(`/catalog/repositories/${id}/pulls`, {
        method: "POST",
        body: JSON.stringify({
          title: data.get("title"),
          source_branch: data.get("source_branch"),
          target_branch: data.get("target_branch"),
          description: "",
        }),
      });
      await cache.invalidateQueries({
        queryKey: [
          "catalog-pulls",
          ref?.registry_instance_id,
          ref?.namespace_id,
          id,
        ],
      });
      form.reset();
      setShowPullForm(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Не удалось создать PR");
    } finally {
      setBusy(false);
    }
  }
  if (repository.isPending)
    return <LoadingState message="Загружаем репозиторий…" />;
  if (malformed || repository.isError || !repo)
    return <ErrorState message="Репозиторий недоступен." />;
  if (
    ref &&
    (!repo.namespace ||
      ref.namespace_id !== repo.namespace.namespace_id ||
      ref.registry_instance_id !== repo.namespace.registry_instance_id)
  )
    return <ErrorState message="Репозиторий принадлежит другому проекту." />;
  const clone = repo.clone_url ?? repo.external_url;
  return (
    <div className="space-y-5">
      <TextLink asChild>
        <Link
          to={withNamespaceLocation(
            repo.namespace
              ? workspacePath(repo.namespace, "repositories")
              : "/repositories",
            repo.namespace ?? null,
          )}
        >
          Репозитории проекта
        </Link>
      </TextLink>
      {project.data && (
        <p className="flex items-center gap-2 text-sm">
          <ProjectAvatar projectKey={project.data.project_key} size="xs" />
          {project.data.name} · {project.data.project_key}
        </p>
      )}
      <PageHeader
        title={repo.public_name}
        actions={
          <>
            {canAdmin && repo.kind === "hosted" && (
              <TextLink asChild>
                <Link
                  to={withNamespaceLocation(
                    `/catalog/repositories/${id}/sync`,
                    repo.namespace ?? null,
                  )}
                >
                  Синхронизация с внешним репозиторием
                </Link>
              </TextLink>
            )}
          </>
        }
      />
      <SectionNavigation aria-label="Разделы репозитория">
        {[
          ["git", "Git / PR"],
          ...(repo.kind === "hosted" ? [["files", "Файлы и история"]] : []),
          ["pipelines", "Пайплайны"],
          ["deployments", "Деплои"],
          ...(canAdmin ? [["configurations", "CI-конфигурации"]] : []),
        ].map(([key, label]) => (
          <SectionNavigationItem key={key} asChild active={tab === key}>
            <Link
              to={withNamespaceLocation(
                tabLocation(key),
                repo.namespace ?? null,
              )}
            >
              {label}
            </Link>
          </SectionNavigationItem>
        ))}
      </SectionNavigation>
      {!["git", "files", "pipelines", "deployments", "configurations"].includes(
        tab,
      ) && <ErrorState message="Раздел репозитория не найден." />}
      {tab === "files" && repo.kind === "external" && (
        <Notice role="status" tone="neutral" appearance="inline">
          Файлы внешнего репозитория доступны у Git-провайдера.
        </Notice>
      )}
      {tab === "files" &&
        repo.kind === "hosted" &&
        repo.storage_name &&
        (repo.availability ? (
          <ErrorState message="Git-хранилище недоступно." />
        ) : (
          <RepositoryBrowserPage repositoryName={repo.storage_name} embedded />
        ))}
      {tab === "pipelines" && (
        <ExecutionList
          key={id + tab}
          namespace={repo.namespace ?? undefined}
          repositoryId={id}
        />
      )}
      {tab === "deployments" && (
        <ExecutionList
          key={id + tab}
          namespace={repo.namespace ?? undefined}
          repositoryId={id}
          deployment
        />
      )}
      {tab === "configurations" && (
        <RepositoryConfigurations
          key={id}
          repositoryId={id!}
          writable={
            canAdmin &&
            repo.state === "active" &&
            repo.ready &&
            !repo.availability
          }
        />
      )}
      <div hidden={tab !== "git"} className="space-y-5">
        {repo.availability && (
          <Notice role="alert" tone="warning" appearance="inline">
            Git-хранилище недоступно. История каталога сохранена; для push и
            merge требуется восстановить исходные данные.
          </Notice>
        )}
        <div className="space-y-2">
          <Label htmlFor="repository-clone-url">
            {repo.kind === "hosted" ? "Clone URL" : "URL внешнего Git"}
          </Label>
          <div className="flex min-w-0 gap-2">
            <Input
              id="repository-clone-url"
              readOnly
              value={clone ?? ""}
              className="font-mono"
            />
            <CopyButton
              variant="outline"
              value={clone ?? ""}
              copiedLabel="Скопировано"
              onCopied={() => setError(undefined)}
              onCopyError={() =>
                setError(
                  "Не удалось скопировать. Выделите URL и скопируйте вручную.",
                )
              }
            >
              Копировать
            </CopyButton>
          </div>
        </div>
        {repo.kind === "external" && (
          <p className="text-sm text-text-muted">
            Используется внешний Git и существующий CI. Список PR остаётся у
            провайдера.
          </p>
        )}
        {error && (
          <Notice role="alert" tone="danger" appearance="inline">
            {error}
          </Notice>
        )}
        {repo.kind === "hosted" && (
          <section className="space-y-3">
            <SectionHeader
              title={<>Pull requests</>}
              actions={
                <>
                  {repo.state === "active" && !repo.availability && (
                    <Button type="button" onClick={() => setShowPullForm(true)}>
                      Создать PR
                    </Button>
                  )}
                </>
              }
            />
            {pulls.isPending ? (
              <LoadingState message="Загружаем PR…" />
            ) : pulls.isError ? (
              <Notice role="alert" tone="danger" appearance="inline">
                PR недоступны.
              </Notice>
            ) : pulls.data?.length === 0 ? (
              <p className="text-text-muted">PR пока нет.</p>
            ) : (
              pulls.data?.map((pr) => (
                <TextLink asChild>
                  <Link
                    key={pr.id}
                    className="block"
                    to={withNamespaceLocation(
                      `/catalog/repositories/${id}/pulls/${pr.number}${intent}`,
                      ref,
                    )}
                  >
                    #{pr.number} {pr.title} ·{" "}
                    {{ open: "Открыт", closed: "Закрыт", merged: "Слит" }[
                      pr.status
                    ] ?? pr.status}
                  </Link>
                </TextLink>
              ))
            )}
          </section>
        )}
        {repo.state === "active" &&
          !repo.availability &&
          repo.kind === "hosted" && (
            <Dialog
              open={showPullForm}
              onOpenChange={(open) => {
                if (!busy) setShowPullForm(open);
              }}
            >
              <DialogContent aria-describedby={undefined}>
                <DialogTitle>Создать PR</DialogTitle>
                <Card asChild className="max-w-xl">
                  <form onSubmit={createPull}>
                    <CardContent className="space-y-3">
                      <Label htmlFor="pr-title">Название</Label>
                      <Input id="pr-title" name="title" required />
                      <Label htmlFor="pr-source">Исходная ветка</Label>
                      <Input id="pr-source" name="source_branch" required />
                      <Label htmlFor="pr-target">Целевая ветка</Label>
                      <Input
                        id="pr-target"
                        name="target_branch"
                        defaultValue="main"
                        required
                      />
                      <Button disabled={busy}>Создать PR</Button>
                    </CardContent>
                  </form>
                </Card>
              </DialogContent>
            </Dialog>
          )}
      </div>
    </div>
  );
}
