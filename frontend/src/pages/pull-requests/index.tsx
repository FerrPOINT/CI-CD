import { AutocompleteInput, PageNavigation, FormField, TextLink } from "@pdlc/ui/ui";
import { useEffect, useState } from "react";
import {
  Select,
  NamespaceLink as Link,
  ResourceList,
  ResourceListRow,
  toast,
  InitialsAvatar as UserAvatar,
  Button,
  Input,
  Label,
  Textarea,
  SearchInput,
  Badge,
  Notice,
  PageHeader,
} from "@pdlc/ui/ui";
import { useParams, useSearchParams } from "react-router";
import { useTranslation } from "react-i18next";
import { ChevronRight, FileDiff, GitPullRequest, Plus } from "lucide-react";

import {
  useCreatePullRequest,
  usePullRequests,
  useRepositoryRefs,
} from "@/api/hooks";
import { currentSession } from "@/api/auth";
import type { PullRequest, PullRequestStatus } from "@/api/types";
import { formatDate } from "@/shared/lib/format";
import { QueryState } from "@/shared/ui/query-state";

import { useDebouncedValue } from "@pdlc/ui/lib";

import { PullRequestActions } from "./actions";

const pageSize = 20;
const pullRequestStatuses: PullRequestStatus[] = ["open", "closed", "merged"];

const statusTones = {
  open: "info",
  closed: "danger",
  merged: "success",
} as const;

function CreatePullRequestForm({
  repo,
  onClose,
}: {
  repo: string;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const createPullRequest = useCreatePullRequest(repo);
  const [form, setForm] = useState({
    title: "",
    description: "",
    source_branch: "",
    target_branch: "",
  });
  const deferredSource = useDebouncedValue(form.source_branch.trim());
  const deferredTarget = useDebouncedValue(form.target_branch.trim());
  const sourceRefs = useRepositoryRefs(repo, {
    kind: "branch",
    limit: 51,
    search: deferredSource,
  });
  const targetRefs = useRepositoryRefs(repo, {
    kind: "branch",
    limit: 51,
    search: deferredTarget,
  });
  const refsLoading = sourceRefs.isLoading || targetRefs.isLoading;
  const refsError = sourceRefs.error || targetRefs.error;
  const sameBranch = Boolean(
    form.source_branch.trim() &&
    form.source_branch.trim() === form.target_branch.trim(),
  );

  function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (
      sameBranch ||
      !form.title.trim() ||
      !form.source_branch.trim() ||
      !form.target_branch.trim()
    )
      return;
    createPullRequest.mutate(
      {
        repository_name: repo,
        title: form.title.trim(),
        description: form.description.trim() || undefined,
        source_branch: form.source_branch.trim(),
        target_branch: form.target_branch.trim(),
        author: currentSession()?.username,
      },
      {
        onSuccess: () => {
          toast.success(t("pulls.created"));
          onClose();
        },
        onError: (error) => toast.error(error.message),
      },
    );
  }

  return (
    <form
      aria-label={t("pulls.create")}
      onSubmit={handleSubmit}
      className="grid gap-4 border-y border-border py-4 sm:grid-cols-2"
    >
      <div className="sm:col-span-2">
        <FormField id="pr-title" label={t("pulls.titleField")} required>
          {(attrs) => (
            <Input
              {...attrs}
              value={form.title}
              onChange={(event) =>
                setForm({ ...form, title: event.target.value })
              }
            />
          )}
        </FormField>
      </div>
      <FormField id="pr-source" label={t("pulls.sourceBranch")} required>
        {(attrs) => (
          <AutocompleteInput
            {...attrs}
            options={
              sourceRefs.data
                ?.filter((ref) => ref.kind === "branch")
                .slice(0, 50)
                .map((ref) => ({ value: ref.name, label: ref.name })) ?? []
            }
            className="font-mono"
            value={form.source_branch}
            onValueChange={(nextValue) => setForm({ ...form, source_branch: nextValue })}
          />
        )}
      </FormField>
      <FormField id="pr-target" label={t("pulls.targetBranch")} required>
        {(attrs) => (
          <AutocompleteInput
            {...attrs}
            options={
              targetRefs.data
                ?.filter((ref) => ref.kind === "branch")
                .slice(0, 50)
                .map((ref) => ({ value: ref.name, label: ref.name })) ?? []
            }
            className="font-mono"
            value={form.target_branch}
            onValueChange={(nextValue) => setForm({ ...form, target_branch: nextValue })}
          />
        )}
      </FormField>
      {sameBranch && (
        <Notice
          role="alert"
          tone="danger"
          appearance="inline"
          className="sm:col-span-2"
        >
          {t("pulls.branchesMustDiffer")}
        </Notice>
      )}
      {refsLoading && (
        <Notice
          role="status"
          tone="neutral"
          appearance="inline"
          className="sm:col-span-2"
        >
          {t("pulls.loadingRefs")}
        </Notice>
      )}
      {((sourceRefs.data?.length ?? 0) > 50 ||
        (targetRefs.data?.length ?? 0) > 50) && (
        <Notice
          role="status"
          tone="neutral"
          appearance="inline"
          className="sm:col-span-2"
        >
          {t("pulls.refSuggestionsLimited")}
        </Notice>
      )}
      {Boolean(refsError) && (
        <Notice
          tone="warning"
          appearance="inline"
          role="alert"
          className="sm:col-span-2"
          actions={
            <Button
              type="button"
              size="sm"
              variant="outline"

              onClick={() => {
                if (sourceRefs.error) void sourceRefs.refetch();
                if (targetRefs.error) void targetRefs.refetch();
              }}
            >
              {t("common.retry")}
            </Button>
          }
        >
          {t("pulls.refsUnavailable")}
        </Notice>
      )}
      <div className="sm:col-span-2">
        <FormField id="pr-description" label={t("pulls.descriptionField")}>
          {(attrs) => (
            <Textarea
              {...attrs}
              rows={3}
              value={form.description}
              onChange={(event) =>
                setForm({ ...form, description: event.target.value })
              }
            />
          )}
        </FormField>
      </div>
      <div className="flex flex-wrap gap-2 sm:col-span-2">
        <Button
          type="submit"

          disabled={createPullRequest.isPending || sameBranch}
        >
          {t("pulls.create")}
        </Button>
        <Button
          type="button"
          variant="ghost"

          onClick={onClose}
        >
          {t("common.cancel")}
        </Button>
      </div>
    </form>
  );
}

function PullRequestRow({
  repo,
  pullRequest,
  locale,
}: {
  repo: string;
  pullRequest: PullRequest;
  locale: string;
}) {
  const { t } = useTranslation();
  const detailHref = `/repositories/${encodeURIComponent(repo)}/pulls/${pullRequest.number}`;

  return (
    <ResourceListRow asChild>
      <li className="flex min-w-0 flex-col gap-2 sm:flex-row sm:items-center sm:gap-3">
        <div className="flex min-w-0 flex-1 items-start gap-2">
          <GitPullRequest
            className="mt-1 h-4 w-4 shrink-0 text-accent"
            aria-hidden
          />
          <div className="min-w-0 flex-1">
            <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
              <TextLink asChild>
                <Link
                  to={detailHref}
                  className="min-w-0 break-words text-sm font-medium"
                >
                  <span className="text-text-muted">#{pullRequest.number}</span>{" "}
                  {pullRequest.title}
                </Link>
              </TextLink>
              <Badge tone={statusTones[pullRequest.status]}>
                {t(`pulls.status_${pullRequest.status}`)}
              </Badge>
            </div>
            <div className="mt-1 flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-xs text-text-muted">
              <code className="break-all">{pullRequest.source_branch}</code>
              <span aria-hidden>→</span>
              <code className="break-all">{pullRequest.target_branch}</code>
              <span className="inline-flex items-center gap-1">
                <UserAvatar name={pullRequest.created_by} size="xs" />
                {pullRequest.created_by || t("pulls.unknownAuthor")}
              </span>
              <time dateTime={pullRequest.created_at}>
                {formatDate(pullRequest.created_at, locale)}
              </time>
            </div>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1 pl-6 sm:pl-0">
          <Button asChild variant="ghost" size="icon">
            <Link
              to={`${detailHref}?view=diff`}
              aria-label={`${t("pulls.viewDiff")} #${pullRequest.number}`}
              title={`${t("pulls.viewDiff")} #${pullRequest.number}`}
            >
              <FileDiff className="h-4 w-4" aria-hidden />
            </Link>
          </Button>
          <PullRequestActions repo={repo} pullRequest={pullRequest} compact />
        </div>
      </li>
    </ResourceListRow>
  );
}

export function PullRequestsPage() {
  const { t, i18n } = useTranslation();
  const { repo } = useParams<{ repo: string }>();
  const [searchParams, setSearchParams] = useSearchParams();
  const [showForm, setShowForm] = useState(false);
  const urlSearch = searchParams.get("q")?.slice(0, 200) ?? "";
  const rawStatus = searchParams.get("status");
  const status: PullRequestStatus | "all" = pullRequestStatuses.includes(
    rawStatus as PullRequestStatus,
  )
    ? (rawStatus as PullRequestStatus)
    : "all";
  const rawPage = Number(searchParams.get("page"));
  const page = Number.isSafeInteger(rawPage) && rawPage > 0 ? rawPage : 1;
  const [searchDraft, setSearchDraft] = useState(urlSearch);
  const { data, isLoading, isFetching, isPlaceholderData, error, refetch } =
    usePullRequests(repo, {
      limit: pageSize,
      offset: (page - 1) * pageSize,
      status: status === "all" ? undefined : status,
      search: urlSearch || undefined,
    });
  const hasFilters = Boolean(urlSearch || status !== "all");

  useEffect(() => setSearchDraft(urlSearch), [urlSearch]);

  useEffect(() => {
    const timeout = window.setTimeout(() => {
      const nextQuery = searchDraft.trim();
      if (nextQuery === urlSearch) return;
      setSearchParams(
        (current) => {
          const next = new URLSearchParams(current);
          if (nextQuery) next.set("q", nextQuery);
          else next.delete("q");
          next.delete("page");
          return next;
        },
        { replace: true },
      );
    }, 300);
    return () => window.clearTimeout(timeout);
  }, [searchDraft, setSearchParams, urlSearch]);

  useEffect(() => {
    if (!data || isPlaceholderData) return;
    const lastPage = Math.max(1, Math.ceil(data.total / pageSize));
    if (page <= lastPage) return;
    setSearchParams(
      (current) => {
        const next = new URLSearchParams(current);
        if (lastPage === 1) next.delete("page");
        else next.set("page", String(lastPage));
        return next;
      },
      { replace: true },
    );
  }, [data, isPlaceholderData, page, setSearchParams]);

  function changePage(nextPage: number) {
    setSearchParams((current) => {
      const next = new URLSearchParams(current);
      if (nextPage <= 1) next.delete("page");
      else next.set("page", String(nextPage));
      return next;
    });
  }

  function changeStatus(nextStatus: PullRequestStatus | "all") {
    setSearchParams(
      (current) => {
        const next = new URLSearchParams(current);
        if (nextStatus === "all") next.delete("status");
        else next.set("status", nextStatus);
        next.delete("page");
        return next;
      },
      { replace: true },
    );
  }

  if (!repo)
    return (
      <p className="text-sm text-text-muted">{t("repositories.notFound")}</p>
    );

  return (
    <div className="space-y-5">
      <div>
        <div className="flex min-w-0 flex-wrap items-center gap-2 text-sm text-text-muted">
          <TextLink asChild>
            <Link to="/repositories">{t("navigation.repositories")}</Link>
          </TextLink>
          <ChevronRight className="h-3 w-3" aria-hidden />
          <TextLink asChild>
            <Link
              to={`/repositories/${encodeURIComponent(repo)}`}
              className="break-all"
            >
              {repo}
            </Link>
          </TextLink>
          <ChevronRight className="h-3 w-3" aria-hidden />
          <span>{t("repositoryBrowser.pullRequests")}</span>
        </div>
        <div className="mt-2">
          <PageHeader
            title={t("pulls.title")}
            leading={
              <GitPullRequest className="h-6 w-6 text-accent" aria-hidden />
            }
            actions={
              <Button
                type="button"
                onClick={() => setShowForm((value) => !value)}
              >
                <Plus className="h-4 w-4" aria-hidden />
                {t("pulls.create")}
              </Button>
            }
          />
        </div>
      </div>

      {showForm && (
        <CreatePullRequestForm repo={repo} onClose={() => setShowForm(false)} />
      )}

      <QueryState
        data={data}
        isLoading={isLoading}
        error={error}
        onRetry={() => refetch()}
        isEmpty={(result) => result.total === 0 && !hasFilters}
        empty={{ title: t("pulls.empty") }}
      >
        {(result) => {
          const pageCount = Math.max(1, Math.ceil(result.total / pageSize));
          return (
            <section
              aria-label={t("pulls.title")}
              aria-busy={isFetching}
              className="space-y-3"
            >
              <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
                <div className="min-w-0 flex-1">
                  <SearchInput
                    aria-label={t("pulls.search")}
                    placeholder={t("pulls.search")}
                    maxLength={200}
                    value={searchDraft}
                    onChange={(event) => setSearchDraft(event.target.value)}
                  />
                </div>
                <Label htmlFor="pr-status" className="sr-only">
                  {t("pulls.statusFilter")}
                </Label>
                <Select
                  id="pr-status"
                  value={status}
                  onValueChange={(nextValue) =>
                    changeStatus(nextValue as PullRequestStatus | "all")
                  }
                  className="w-auto sm:w-40"
                  options={[
                    { value: "all", label: String(t("pulls.allStatuses")) },
                    { value: "open", label: String(t("pulls.status_open")) },
                    {
                      value: "closed",
                      label: String(t("pulls.status_closed")),
                    },
                    {
                      value: "merged",
                      label: String(t("pulls.status_merged")),
                    },
                  ]}
                />
              </div>
              <p
                className="flex flex-wrap gap-2 text-xs text-text-muted"
                aria-live="polite"
              >
                <span>
                  {t("pulls.shown", {
                    count: result.items.length,
                    total: result.total,
                  })}
                </span>
                {isFetching && <span>{t("common.loading")}</span>}
              </p>
              {result.total === 0 ? (
                <Notice tone="neutral" appearance="inline" role="status">
                  {t("pulls.noMatches")}
                </Notice>
              ) : (
                <ResourceList asChild>
                  <ul>
                    {result.items.map((pullRequest) => (
                      <PullRequestRow
                        key={pullRequest.id}
                        repo={repo}
                        pullRequest={pullRequest}
                        locale={i18n.language}
                      />
                    ))}
                  </ul>
                </ResourceList>
              )}
              {result.total > pageSize && (
                <PageNavigation
                  page={page}
                  pageCount={pageCount}
                  onPageChange={changePage}
                  label={t("pulls.pages")}
                  previousLabel={t("pulls.previous")}
                  nextLabel={t("pulls.next")}
                  pending={isFetching}
                />
              )}
            </section>
          );
        }}
      </QueryState>
    </div>
  );
}
