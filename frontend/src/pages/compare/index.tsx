import { useEffect, useState, type ComponentProps } from "react";
import {
  Badge,
  PageHeader,
  SectionHeader,
  NamespaceLink as Link,
  ResourceList,
  ResourceListRow,
  DiffBlock,
  Button,
  Input,
  Label,
  EmptyState,
  Notice,
  TextLink,
} from "@pdlc/ui/ui";
import { useParams, useSearchParams } from "react-router";
import { useTranslation } from "react-i18next";
import { ChevronRight, FileDiff, GitCompareArrows } from "lucide-react";
import { useRepositoryComparison, useRepositoryRefs } from "@/api/hooks";
import type { ChangeStatus, Comparison } from "@/api/types";
import { QueryState } from "@/shared/ui/query-state";
import { useDebouncedValue } from "@pdlc/ui/lib";

const changeStatusTones: Record<
  ChangeStatus,
  ComponentProps<typeof Badge>["tone"]
> = {
  added: "success",
  modified: "warning",
  deleted: "danger",
};

function ComparisonResults({ comparison }: { comparison: Comparison }) {
  const { t } = useTranslation();
  const totalAdditions = comparison.files.reduce(
    (sum, file) => sum + file.additions,
    0,
  );
  const totalDeletions = comparison.files.reduce(
    (sum, file) => sum + file.deletions,
    0,
  );

  if (comparison.files.length === 0) {
    return (
      <div role="status">
        <EmptyState message={t("compare.noChanges")} />
      </div>
    );
  }

  return (
    <section className="space-y-4" aria-label={t("compare.filesChanged")}>
      <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 border-y border-border py-3 text-sm">
        <span className="text-text-muted">{t("compare.mergeBase")}:</span>
        <code className="break-all rounded bg-surface-raised px-1.5 py-0.5">
          {comparison.merge_base}
        </code>
        <span className="font-mono text-text-primary">+{totalAdditions}</span>
        <span className="font-mono text-text-primary">−{totalDeletions}</span>
      </div>
      <SectionHeader
        title={
          <>
            {t("compare.filesChanged")}{" "}
            <span className="font-normal text-text-muted">
              ({comparison.files.length})
            </span>
          </>
        }
      />
      <ResourceList asChild>
        <ul>
          {comparison.files.map((file) => (
            <ResourceListRow asChild key={file.path}>
              <li className="flex min-w-0 flex-col gap-1 sm:flex-row sm:items-center sm:gap-3">
                <div className="flex min-w-0 flex-1 items-start gap-2">
                  <FileDiff
                    className="mt-0.5 h-4 w-4 shrink-0 text-text-muted"
                    aria-hidden
                  />
                  <code className="min-w-0 break-all text-xs">{file.path}</code>
                </div>
                <div className="flex shrink-0 items-center gap-3 pl-6 text-xs sm:pl-0">
                  <Badge tone={changeStatusTones[file.status]}>
                    {t(`compare.status_${file.status}`)}
                  </Badge>
                  {file.binary ? (
                    <span className="text-text-muted">
                      {t("compare.binaryFile")}
                    </span>
                  ) : (
                    <span className="font-mono text-text-primary">
                      +{file.additions} −{file.deletions}
                    </span>
                  )}
                </div>
              </li>
            </ResourceListRow>
          ))}
        </ul>
      </ResourceList>
      {comparison.patch.trim() && (
        <div className="min-w-0 space-y-3">
          <SectionHeader title={<>{t("compare.patch")}</>} />
          <DiffBlock
            patch={comparison.patch}
            aria-label={t("compare.patch")}
            notice={
              comparison.patch_truncated
                ? t("compare.patchTruncated")
                : undefined
            }
          />
        </div>
      )}
    </section>
  );
}

export function ComparePage() {
  const { t } = useTranslation();
  const { repo } = useParams<{ repo: string }>();
  const [searchParams, setSearchParams] = useSearchParams();
  const fromParam = searchParams.get("from")?.trim() ?? "";
  const toParam = searchParams.get("to")?.trim() ?? "";
  const [from, setFrom] = useState(fromParam);
  const [to, setTo] = useState(toParam);
  const deferredFrom = useDebouncedValue(from.trim());
  const deferredTo = useDebouncedValue(to.trim());
  const sameRef = Boolean(from.trim() && from.trim() === to.trim());
  const canCompare = Boolean(fromParam && toParam && fromParam !== toParam);
  const fromRefs = useRepositoryRefs(repo, { limit: 51, search: deferredFrom });
  const toRefs = useRepositoryRefs(repo, { limit: 51, search: deferredTo });
  const {
    data: comparison,
    isLoading,
    error,
    refetch,
  } = useRepositoryComparison(repo, fromParam, canCompare ? toParam : "");

  useEffect(() => {
    setFrom(fromParam);
    setTo(toParam);
  }, [fromParam, toParam]);

  if (!repo)
    return (
      <p className="text-sm text-text-muted">{t("repositories.notFound")}</p>
    );

  function applyComparison(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const base = from.trim();
    const head = to.trim();
    if (!base || !head || base === head) return;
    setSearchParams(new URLSearchParams({ from: base, to: head }), {
      replace: true,
    });
  }

  return (
    <div className="space-y-5">
      <div>
        <div className="flex min-w-0 flex-wrap items-center gap-2 text-sm text-text-muted">
          <TextLink asChild>
            <Link
              to="/repositories"
              className="inline-flex min-h-10 items-center"
            >
              {t("navigation.repositories")}
            </Link>
          </TextLink>
          <ChevronRight className="h-3 w-3" aria-hidden />
          <TextLink asChild>
            <Link
              to={`/repositories/${encodeURIComponent(repo)}`}
              className="inline-flex min-h-10 items-center break-all"
            >
              {repo}
            </Link>
          </TextLink>
          <ChevronRight className="h-3 w-3" aria-hidden />
          <span>{t("repositoryBrowser.compare")}</span>
        </div>
        <div className="mt-2">
          <PageHeader
            title={t("compare.title")}
            leading={
              <>
                <GitCompareArrows className="h-6 w-6 text-accent" aria-hidden />
              </>
            }
          />
        </div>
      </div>

      <form
        aria-label={t("compare.title")}
        onSubmit={applyComparison}
        className="grid gap-3 border-y border-border py-4 sm:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)_auto] sm:items-end"
      >
        <div className="min-w-0 space-y-1.5">
          <Label htmlFor="compare-from">{t("compare.baseRef")}</Label>
          <Input
            id="compare-from"
            required
            list="compare-refs-from"
            className="font-mono"
            value={from}
            onChange={(event) => setFrom(event.target.value)}
          />
          <datalist id="compare-refs-from">
            {fromRefs.data?.slice(0, 50).map((ref) => (
              <option key={`${ref.kind}-${ref.name}`} value={ref.name} />
            ))}
          </datalist>
        </div>
        <GitCompareArrows
          className="mb-3 hidden h-4 w-4 text-text-muted sm:block"
          aria-hidden
        />
        <div className="min-w-0 space-y-1.5">
          <Label htmlFor="compare-to">{t("compare.headRef")}</Label>
          <Input
            id="compare-to"
            required
            list="compare-refs-to"
            className="font-mono"
            value={to}
            onChange={(event) => setTo(event.target.value)}
          />
          <datalist id="compare-refs-to">
            {toRefs.data?.slice(0, 50).map((ref) => (
              <option key={`${ref.kind}-${ref.name}`} value={ref.name} />
            ))}
          </datalist>
        </div>
        <Button type="submit" disabled={sameRef}>
          {t("compare.compareAction")}
        </Button>
        {sameRef && (
          <Notice
            role="alert"
            className="sm:col-span-4"
            tone="danger"
            appearance="inline"
          >
            {t("compare.refsMustDiffer")}
          </Notice>
        )}
        {(fromRefs.isLoading || toRefs.isLoading) && (
          <Notice
            role="status"
            className="sm:col-span-4"
            tone="neutral"
            appearance="inline"
          >
            {t("compare.loadingRefs")}
          </Notice>
        )}
        {((fromRefs.data?.length ?? 0) > 50 ||
          (toRefs.data?.length ?? 0) > 50) && (
          <Notice
            role="status"
            className="sm:col-span-4"
            tone="warning"
            appearance="inline"
          >
            {t("compare.refSuggestionsLimited")}
          </Notice>
        )}
        {Boolean(fromRefs.error || toRefs.error) && (
          <Notice
            role="alert"
            className="sm:col-span-4"
            tone="warning"
            appearance="panel"
            actions={
              <Button
                type="button"
                size="sm"
                variant="outline"

                onClick={() => {
                  if (fromRefs.error) void fromRefs.refetch();
                  if (toRefs.error) void toRefs.refetch();
                }}
              >
                {t("common.retry")}
              </Button>
            }
          >
            <span>{t("compare.refsUnavailable")}</span>
          </Notice>
        )}
      </form>

      {!canCompare ? (
        <Notice role="status" tone="neutral" appearance="inline">
          {fromParam && toParam
            ? t("compare.refsMustDiffer")
            : t("compare.chooseRefs")}
        </Notice>
      ) : (
        <QueryState
          data={comparison}
          isLoading={isLoading}
          error={error}
          onRetry={() => refetch()}
        >
          {(result) => <ComparisonResults comparison={result} />}
        </QueryState>
      )}
    </div>
  );
}
