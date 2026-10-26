import {
  ActionGroup,
  ResourceListDescription,
  Prose,
  ResourceStateView,
  NamespaceLink as Link,
  ResourceList,
  ResourceListRow,
  DiffBlock,
  Button,
  InitialsAvatar as UserAvatar,
  TextLink,
} from "@pdlc/ui/ui";
import {
  PageHeader,
  SectionHeader,
  Badge,
  Card,
  CardContent,
  DescriptionItem,
  LoadingState,
  EmptyState,
  Notice,
} from "@pdlc/ui/ui";
import { useParams, useSearchParams } from "react-router";
import { useTranslation } from "react-i18next";
import { usePullRequest, useRepositoryComparison } from "@/api/hooks";
import { ApiError } from "@/api/client";

import {
  ChevronRight,
  GitPullRequest,
  FileDiff,
  ArrowLeft,
} from "lucide-react";
import type { PullRequest, PullRequestStatus } from "@/api/types";
import { formatDate } from "@/shared/lib/format";
import { QueryState } from "@/shared/ui/query-state";
import { PullRequestActions } from "@/pages/pull-requests/actions";

function branchRef(branch: string): string {
  return branch.startsWith("refs/heads/") ? branch : `refs/heads/${branch}`;
}

export function buildCompareHref(
  pr: Pick<PullRequest, "repository_name" | "source_branch" | "target_branch">,
): string {
  return `/repositories/${encodeURIComponent(pr.repository_name)}/compare?from=${encodeURIComponent(branchRef(pr.target_branch))}&to=${encodeURIComponent(branchRef(pr.source_branch))}`;
}

export function buildPrDiffHref(
  pr: Pick<PullRequest, "repository_name" | "number">,
): string {
  return `/repositories/${encodeURIComponent(pr.repository_name)}/pulls/${pr.number}?view=diff`;
}

const statusTones = {
  open: "info",
  closed: "danger",
  merged: "success",
} as const satisfies Record<PullRequestStatus, string>;

export function PullRequestDetailPage() {
  const { t, i18n } = useTranslation();
  const { repo, number } = useParams<{ repo: string; number: string }>();
  const [searchParams, setSearchParams] = useSearchParams();
  const parsedNumber = Number(number);
  const pullRequestNumber =
    Number.isSafeInteger(parsedNumber) && parsedNumber > 0
      ? parsedNumber
      : undefined;
  const {
    data: pr,
    isLoading,
    error,
    refetch,
  } = usePullRequest(repo, pullRequestNumber);

  const showDiff = searchParams.get("view") === "diff";
  const {
    data: comparison,
    isLoading: diffLoading,
    isError: diffError,
    error: diffErrorValue,
    refetch: refetchDiff,
  } = useRepositoryComparison(
    repo,
    showDiff && pr ? branchRef(pr.target_branch) : "",
    showDiff && pr ? branchRef(pr.source_branch) : "",
  );

  if (!repo)
    return (
      <ResourceStateView
        scope="local"
        state={{
          kind: "not-found",
          title: t("repositories.notFound"),
          headingLevel: 1,
        }}
      />
    );
  if (
    !pullRequestNumber ||
    (error instanceof ApiError && error.status === 404)
  ) {
    return (
        <ResourceStateView
          scope="local"
          state={{ kind: "not-found", title: t("pulls.notFound"), headingLevel: 1 }}
        />
      );
  }
  if (isLoading || error)
    return (
      <QueryState
        data={pr}
        isLoading={isLoading}
        error={error}
        onRetry={() => refetch()}
      >
        {() => null}
      </QueryState>
    );
  if (!pr)
    return (
        <ResourceStateView
          scope="local"
          state={{ kind: "not-found", title: t("pulls.notFound"), headingLevel: 1 }}
        />
      );

  if (showDiff) {
    return (
      <div className="space-y-6">
        <div>
          <div className="flex flex-wrap items-center gap-2 text-sm text-text-muted">
            <TextLink asChild>
              <Link
                to="/repositories"
                className="inline-flex min-h-10 items-center"
              >
                {t("navigation.repositories")}
              </Link>
            </TextLink>
            <ChevronRight className="h-3 w-3" />
            <TextLink asChild>
              <Link
                to={`/repositories/${encodeURIComponent(repo)}/pulls`}
                className="inline-flex min-h-10 items-center"
              >
                {t("repositoryBrowser.pullRequests")}
              </Link>
            </TextLink>
            <ChevronRight className="h-3 w-3" />
            <TextLink asChild>
              <Link
                to={`/repositories/${encodeURIComponent(repo)}/pulls/${pr.number}`}
                className="inline-flex min-h-10 items-center"
              >
                #{pr.number}
              </Link>
            </TextLink>
            <ChevronRight className="h-3 w-3" />
            <span>{t("pulls.viewDiff")}</span>
          </div>
          <div className="mt-2">
            <PageHeader
              leading={<FileDiff className="h-6 w-6 text-accent" aria-hidden />}
              title={`${t("pulls.viewDiff")} #${pr.number}`}
            />
          </div>
          <p className="mt-2 break-all font-mono text-sm text-text-secondary">
            {pr.target_branch} → {pr.source_branch}
          </p>
          <p className="mt-2 text-sm text-text-muted">{pr.title}</p>
        </div>
        <Button
          variant="outline"
          onClick={() => setSearchParams({}, { replace: true })}
        >
          <ArrowLeft className="h-4 w-4" /> {t("common.back")}
        </Button>
        {diffLoading ? (
          <LoadingState message={t("common.loading")} />
        ) : diffError ? (
          <Notice
            tone="danger"
            appearance="inline"
            role="alert"
            actions={
              <Button
                type="button"
                variant="outline"
                onClick={() => refetchDiff()}
              >
                {t("common.retry")}
              </Button>
            }
          >
            {t("common.error")}:{" "}
            {diffErrorValue instanceof Error
              ? diffErrorValue.message
              : String(diffErrorValue)}
          </Notice>
        ) : !comparison || comparison.files.length === 0 ? (
          <EmptyState message={t("compare.noChanges")} />
        ) : (
          <>
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-y border-border py-3 text-sm">
              <span className="text-text-muted">
                {t("compare.mergeBase")}:{" "}
              </span>
              <code className="break-all rounded bg-surface-raised px-1.5 py-0.5">
                {comparison.merge_base}
              </code>
              {comparison.files.some((file) => !file.binary) ? (
                <>
                  <span className="text-success">
                    +
                    {comparison.files.reduce(
                      (sum, file) => sum + file.additions,
                      0,
                    )}
                  </span>
                  <span className="text-danger">
                    −
                    {comparison.files.reduce(
                      (sum, file) => sum + file.deletions,
                      0,
                    )}
                  </span>
                </>
              ) : null}
            </div>
            <div>
              <SectionHeader title={t("compare.filesChanged")} />
              <ResourceList asChild variant="bordered">
                <ul className="mt-3 overflow-hidden">
                  {comparison.files.map((file) => (
                    <ResourceListRow asChild key={file.path}>
                      <li className="flex items-center gap-3 text-sm">
                        <FileDiff
                          className="h-4 w-4 text-text-muted"
                          aria-hidden
                        />
                        <code className="flex-1 truncate">{file.path}</code>
                        {file.binary ? (
                          <ResourceListDescription asChild>
                            <span>
                            {t("compare.binaryFile")}
                          </span>
                          </ResourceListDescription>
                        ) : (
                          <>
                            <span className="text-success">
                              +{file.additions}
                            </span>
                            <span className="text-danger">
                              −{file.deletions}
                            </span>
                          </>
                        )}
                      </li>
                    </ResourceListRow>
                  ))}
                </ul>
              </ResourceList>
            </div>
            {comparison.patch.trim() && (
              <>
                <SectionHeader title={t("compare.patch")} />
                <DiffBlock
                  patch={comparison.patch}
                  aria-label={t("compare.patch")}
                  notice={
                    comparison.patch_truncated
                      ? t("compare.patchTruncated")
                      : undefined
                  }
                />
              </>
            )}
          </>
        )}
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div>
        <div className="flex flex-wrap items-center gap-2 text-sm text-text-muted">
          <TextLink asChild>
            <Link
              to="/repositories"
              className="inline-flex min-h-10 items-center"
            >
              {t("navigation.repositories")}
            </Link>
          </TextLink>
          <ChevronRight className="h-3 w-3" />
          <TextLink asChild>
            <Link
              to={`/repositories/${encodeURIComponent(repo)}`}
              className="inline-flex min-h-10 items-center break-all"
            >
              {repo}
            </Link>
          </TextLink>
          <ChevronRight className="h-3 w-3" />
          <TextLink asChild>
            <Link
              to={`/repositories/${encodeURIComponent(repo)}/pulls`}
              className="inline-flex min-h-10 items-center"
            >
              {t("repositoryBrowser.pullRequests")}
            </Link>
          </TextLink>
          <ChevronRight className="h-3 w-3" />
          <span>#{pr.number}</span>
        </div>
        <div className="mt-2">
          <PageHeader
            leading={
              <GitPullRequest className="h-6 w-6 text-accent" aria-hidden />
            }
            title={`#${pr.number} ${pr.title}`}
            context={
              <Badge tone={statusTones[pr.status]}>
                {t(`pulls.status_${pr.status}`)}
              </Badge>
            }
          />
        </div>
      </div>

      <div className="page-split items-start">
        <Card asChild className="min-w-0 flex-1">
          <section>
            <CardContent>
              {pr.description && (
                <Prose asChild>
                  <p className="whitespace-pre-wrap">
                  {pr.description}
                </p>
                </Prose>
              )}
              <dl className="mt-4">
                <DescriptionItem
                  layout="inline"
                  label={t("pulls.sourceBranch")}
                >
                  <code className="break-all rounded bg-surface-raised px-1.5 py-0.5">
                    {pr.source_branch}
                  </code>
                </DescriptionItem>
                <DescriptionItem
                  layout="inline"
                  label={t("pulls.targetBranch")}
                >
                  <code className="break-all rounded bg-surface-raised px-1.5 py-0.5">
                    {pr.target_branch}
                  </code>
                </DescriptionItem>
                <DescriptionItem layout="inline" label={t("pulls.createdBy")}>
                  <UserAvatar name={pr.created_by} size="sm" withName />
                </DescriptionItem>
                <DescriptionItem layout="inline" label={t("pulls.createdAt")}>
                  {formatDate(pr.created_at, i18n.language)}
                </DescriptionItem>
                {pr.merged_at && (
                  <DescriptionItem layout="inline" label={t("pulls.mergedAt")}>
                    {formatDate(pr.merged_at, i18n.language)}
                  </DescriptionItem>
                )}
                {pr.merge_commit_sha && (
                  <DescriptionItem
                    layout="inline"
                    label={t("pulls.mergeCommit")}
                  >
                    <code className="rounded bg-surface-raised px-1.5 py-0.5">
                      {pr.merge_commit_sha.slice(0, 7)}
                    </code>
                  </DescriptionItem>
                )}
              </dl>
            </CardContent>
          </section>
        </Card>

        <aside className="min-w-0" aria-labelledby="pull-actions-title">
          <SectionHeader
            headingId="pull-actions-title"
            title={t("pulls.actions")}
          />
          <ActionGroup orientation="vertical" className="mt-3">
            <Button asChild variant="outline" className="justify-start">
              <Link to={buildPrDiffHref(pr)}>
                <FileDiff className="h-4 w-4" /> {t("pulls.viewDiff")}
              </Link>
            </Button>
            <PullRequestActions repo={repo} pullRequest={pr} />
          </ActionGroup>
        </aside>
      </div>
    </div>
  );
}
