import { useQuery } from "@tanstack/react-query";
import { api } from "@/api/client";
import type { Configuration } from "@/api/workspaces";
import { FormEvent, useEffect, useState, type ComponentProps } from "react";
import { Badge, ResourceListDescription,
  PageNavigation,
  PageHeader,
  NamespaceLink as Link,
  ResourceList,
  ResourceListRow,
  useNamespaceNavigate as useNavigate,
  toast,
  Button,
  Input,
  Label,
  TextLink,
} from "@pdlc/ui/ui";

import { useParams } from "react-router";
import { useTranslation } from "react-i18next";
import { ChevronRight, Clock, Play } from "lucide-react";

import { usePipelines, useTriggerPipeline } from "@/api/hooks";
import { QueryState } from "@/shared/ui/query-state";

const pageSize = 20;
const statusTones: Record<string, ComponentProps<typeof Badge>["tone"]> = {
  queued: "muted",
  running: "warning",
  success: "success",
  failed: "danger",
  canceled: "muted",
};

export function PipelinesPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { projectId } = useParams<{ projectId: string }>();
  const configQuery = useQuery({
    queryKey: ["delivery-config", projectId],
    queryFn: ({ signal }) =>
      api<Configuration>(`/delivery-configurations/${projectId}`, { signal }),
  });
  const project = configQuery.data;
  const [page, setPage] = useState(0);
  useEffect(() => setPage(0), [projectId]);
  const pipelinesQuery = usePipelines(projectId, page, pageSize);
  const pipelines = (pipelinesQuery.data ?? []).slice(0, pageSize);
  const hasNextPage = (pipelinesQuery.data?.length ?? 0) > pageSize;
  const trigger = useTriggerPipeline(projectId);
  const [showForm, setShowForm] = useState(false);
  const [gitRef, setGitRef] = useState("");

  function openForm() {
    setGitRef(project?.default_branch ?? "main");
    setShowForm(true);
  }

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!gitRef.trim() || trigger.isPending) return;
    trigger.mutate(gitRef.trim(), {
      onSuccess: (detail) => {
        setShowForm(false);
        toast.success(t("pipelines.started"));
        navigate(`/pipelines/${detail.pipeline.id}`);
      },
      onError: (error) => toast.error(error.message),
    });
  }

  return (
    <div className="space-y-5">
      <>
        <nav
          aria-label={t("pipelines.title")}
          className="flex min-w-0 items-center gap-2 text-sm text-text-muted"
        >
          <TextLink asChild>
            <Link to="/delivery-configs" className="min-h-10 content-center">
              CI-конфигурации
            </Link>
          </TextLink>
          <ChevronRight className="h-4 w-4 shrink-0" aria-hidden />
          <span className="truncate">{project?.name ?? projectId}</span>
        </nav>
        <PageHeader
          title={t("pipelines.title")}
          actions={
            <>
              <Button
                type="button"
                size="sm"

                aria-expanded={showForm}
                aria-controls="pipeline-trigger-form"
                onClick={openForm}
                disabled={trigger.isPending}
              >
                <Play className="h-4 w-4" aria-hidden />
                {t("pipelines.run")}
              </Button>
            </>
          }
        />
      </>

      {showForm && (
        <form
          id="pipeline-trigger-form"
          aria-label={t("pipelines.run")}
          onSubmit={handleSubmit}
          className="flex flex-wrap items-end gap-3 border-y border-border py-3"
        >
          <div className="min-w-48 flex-1 space-y-1.5 sm:max-w-xs">
            <Label htmlFor="git-ref">{t("pipelines.gitRef")}</Label>
            <Input
              id="git-ref"

              required
              placeholder="main"
              value={gitRef}
              onChange={(event) => setGitRef(event.target.value)}
              disabled={trigger.isPending}
            />
          </div>
          <Button
            type="submit"

            disabled={trigger.isPending || !gitRef.trim()}
          >
            {t("pipelines.run")}
          </Button>
          <Button
            type="button"
            variant="outline"

            disabled={trigger.isPending}
            onClick={() => setShowForm(false)}
          >
            {t("common.cancel")}
          </Button>
        </form>
      )}

      <QueryState
        data={pipelines}
        isLoading={pipelinesQuery.isLoading}
        error={pipelinesQuery.error}
        onRetry={() => void pipelinesQuery.refetch()}
        isEmpty={(items) => items.length === 0}
        empty={{ title: t("pipelines.empty") }}
      >
        {() => (
          <ResourceList asChild>
            <section aria-label={t("pipelines.title")}>
              {pipelines.map((pipeline) => (
                <ResourceListRow asChild interactive key={pipeline.id}>
                  <Link
                    to={`/pipelines/${pipeline.id}`}
                    className="flex min-w-0 flex-wrap items-center justify-between gap-x-4 gap-y-1"
                  >
                    <span className="flex min-w-0 flex-wrap items-center gap-2">
                      <code className="text-sm font-medium text-text-primary">
                        #{pipeline.id.slice(0, 8)}
                      </code>
                      <code className="min-w-0 break-all text-xs text-text-secondary">
                        {pipeline.git_ref}
                      </code>
                      <Badge appearance="inline" tone={statusTones[pipeline.status] ?? "neutral"}>
                        {t(`pipelines.${pipeline.status}`)}
                      </Badge>
                    </span>
                    <ResourceListDescription asChild className="flex items-center gap-2"><span >
                      <Clock className="h-4 w-4" aria-hidden />
                      <time dateTime={pipeline.created_at}>
                        {new Date(pipeline.created_at).toLocaleString()}
                      </time>
                      <ChevronRight className="h-4 w-4" aria-hidden />
                    </span></ResourceListDescription>
                  </Link>
                </ResourceListRow>
              ))}
            </section>
          </ResourceList>
        )}
      </QueryState>
      {(page > 0 || hasNextPage) && !pipelinesQuery.isError && (
        <PageNavigation
          page={page + 1}
          hasNext={hasNextPage && !pipelinesQuery.isFetching}
          onPageChange={(nextPage) => setPage(nextPage - 1)}
          label={t("pipelines.title")}
          previousLabel={t("projects.previous")}
          nextLabel={t("projects.next")}
          pending={pipelinesQuery.isFetching}
          summary={t("pipelines.page", { page: page + 1 })}
        />
      )}
    </div>
  );
}
