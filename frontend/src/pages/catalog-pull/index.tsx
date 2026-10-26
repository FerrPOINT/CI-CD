import {
  ActionGroup,
  ResourceListDescription,
  EmptyState,
  CardContent,
  Card,
  PageHeader,
  SectionHeader,
  LoadingState,
  NamespaceLink as Link,
  Button,
  usePlatformServices,
  ErrorState,
  Notice,
  TextLink,
} from "@pdlc/ui/ui";
import { useState } from "react";

import { useLocation, useParams, useSearchParams } from "react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { withNamespaceLocation } from "@pdlc/ui/lib";
import { api } from "@/api/client";
import type { components } from "@/api/schema";
import { useNamespaceContext } from "@/widgets/namespace-context";
export function CatalogPullPage() {
  const { search } = useLocation();
  const { id, number } = useParams();
  const { ref, malformed } = useNamespaceContext();
  const cache = useQueryClient();
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [params] = useSearchParams();
  const { services } = usePlatformServices();
  const task = params.get("task_id"),
    tracker = params.get("tracker_instance_id");
  const linksKey = [
    "pull-task-links",
    ref?.registry_instance_id,
    ref?.namespace_id,
    id,
    number,
  ];
  const links = useQuery({
    queryKey: linksKey,
    queryFn: ({ signal }) =>
      api<components["schemas"]["PullTaskLink"][]>(
        `/catalog/repositories/${id}/pulls/${number}/tasks`,
        { signal },
      ),
  });
  const linked =
    links.data?.some(
      (link) =>
        link.task.task_id === task && link.task.tracker_instance_id === tracker,
    ) ?? false;
  async function linkTask() {
    if (!task || !tracker) return;
    setBusy(true);
    setError(undefined);
    try {
      await api(`/catalog/repositories/${id}/pulls/${number}/tasks`, {
        method: "POST",
        body: JSON.stringify({ tracker_instance_id: tracker, task_id: task }),
      });
      await cache.invalidateQueries({ queryKey: linksKey });
    } catch (error) {
      setError(
        error instanceof Error ? error.message : "Не удалось подтвердить связь",
      );
    } finally {
      setBusy(false);
    }
  }
  const key = [
    "catalog-pull",
    ref?.registry_instance_id,
    ref?.namespace_id,
    id,
    number,
  ];
  const query = useQuery({
    queryKey: key,
    queryFn: ({ signal }) =>
      api<components["schemas"]["PullRequest"]>(
        `/catalog/repositories/${id}/pulls/${number}`,
        {
          signal,
        },
      ),
  });
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
  async function act(action: string) {
    setBusy(true);
    setError(undefined);
    try {
      await api(`/catalog/repositories/${id}/pulls/${number}/action`, {
        method: "POST",
        body: JSON.stringify({ action }),
      });
      await cache.invalidateQueries({ queryKey: key });
    } catch (e) {
      setError(e instanceof Error ? e.message : "Операция отклонена");
    } finally {
      setBusy(false);
    }
  }
  if (query.isPending || repository.isPending)
    return <LoadingState message="Загружаем PR…" />;
  if (
    malformed ||
    query.isError ||
    !query.data ||
    repository.isError ||
    !repository.data ||
    (ref &&
      (repository.data.namespace?.namespace_id !== ref.namespace_id ||
        repository.data.namespace?.registry_instance_id !==
          ref.registry_instance_id))
  )
    return <ErrorState message="PR недоступен." />;
  const pr = query.data;
  const trackerUrl = services.find(
    (service) => service.key === "task-tracker",
  )?.ui_url;
  return (
    <div className="space-y-4">
      <TextLink asChild>
        <Link to={withNamespaceLocation(`/catalog/repositories/${id}`, ref)}>
          Репозиторий
        </Link>
      </TextLink>
      <PageHeader title={`#${pr.number} ${pr.title}`} />
      <p>
        {pr.source_branch} → {pr.target_branch} · {pr.status}
      </p>
      <p>{pr.description}</p>
      {error && (
        <Notice role="alert" tone="danger" appearance="inline">
          {error}
        </Notice>
      )}
      <TextLink asChild>
        <Link
          className="block"
          to={withNamespaceLocation(
            `/repositories/${encodeURIComponent(pr.repository_name)}/pulls/${pr.number}`,
            ref,
          )}
        >
          Diff и проверки
        </Link>
      </TextLink>
      <section className="space-y-2">
        <SectionHeader title={<>Связанные задачи</>} />
        {links.isPending ? (
          <LoadingState message="Загружаем связи…" />
        ) : links.isError ? (
          <Notice role="alert" tone="danger" appearance="inline">
            Связи недоступны.
          </Notice>
        ) : links.data?.length === 0 ? (
          <EmptyState message="Задач пока нет." />
        ) : (
          links.data?.map((link) => (
            <Card key={`${link.task.tracker_instance_id}:${link.task.task_id}`}>
              <CardContent>
                {trackerUrl ? (
                  <TextLink
                    href={withNamespaceLocation(
                      `${trackerUrl}/issues/${link.task.task_id}`,
                      link.namespace,
                      search,
                    )}
                  >
                    {link.task_key}
                  </TextLink>
                ) : (
                  <span>{link.task_key}</span>
                )}
                <ResourceListDescription asChild technical>
                  <p className="break-all">
                  Commit: {link.source_commit_sha}
                </p>
                </ResourceListDescription>
              </CardContent>
            </Card>
          ))
        )}
      </section>
      {task && tracker && repository.data.state === "active" && (
        <div className="space-y-2">
          <Button
            variant="outline"
            disabled={busy || linked || links.isPending || links.isError}
            onClick={() => void linkTask()}
          >
            {linked ? "PR связан с задачей" : "Связать с задачей из Tracker"}
          </Button>
          <p className="text-sm text-text-muted">
            Связь закрепляет исходный commit. Проверки в задаче относятся к
            этому commit и merge commit.
          </p>
        </div>
      )}
      {pr.status === "open" &&
        repository.data.state === "active" &&
        !repository.data.availability && (
          <ActionGroup align="start">
            <Button disabled={busy} onClick={() => act("merge")}>
              Merge
            </Button>
            <Button
              variant="outline"
              disabled={busy}
              onClick={() => act("close")}
            >
              Закрыть
            </Button>
          </ActionGroup>
        )}
    </div>
  );
}
