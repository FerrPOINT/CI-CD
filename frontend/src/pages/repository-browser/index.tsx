import {
  AutocompleteInput,
  ResourceListTitle,
  ResourceListDescription,
  CardContent,
  CardHeader,
  Card,
  Badge,
  PageHeader,
  CodeBlock,
  PageNavigation,
  Button,
  Checkbox,
  NamespaceLink as Link,
  ResourceList,
  ResourceListRow,
  toast,
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
  Input,
  Label,
  Textarea,
  ConfirmDialog,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  SearchInput,
  Notice,
  TextLink,
  FormField,
} from "@pdlc/ui/ui";
import { useEffect, useState } from "react";

import { useParams, useSearchParams } from "react-router";
import { useTranslation } from "react-i18next";
import {
  ArrowLeft,
  Check,
  ChevronRight,
  FileText,
  Folder,
  GitBranch,
  GitCompareArrows,
  GitPullRequest,
  Package,
  Pencil,
  Plus,
  Search,
  Tag,
  Trash2,
  X,
} from "lucide-react";

import {
  useRepositoryCommits,
  useRepositoryRefs,
  useRepositoryTree,
  useRepositoryBlob,
  useRepositoryTags,
  useReleases,
  useCreateRelease,
  useDeleteRelease,
} from "@/api/hooks";

import { QueryState } from "@/shared/ui/query-state";
import { useDebouncedValue } from "@pdlc/ui/lib";
import type { Release, RepositoryRef } from "@/api/types";

const browserTabs = [
  "code",
  "commits",
  "branches",
  "tags",
  "releases",
] as const;
const refPageSize = 100;
const refSuggestionLimit = 51;
const treePageSize = 100;
const commitPageSize = 25;
const maxCommitPage = Math.floor(0xffffffff / commitPageSize) + 1;

function parentPath(path: string): string {
  return path.slice(0, path.lastIndexOf("/"));
}

function formatDate(value: string, locale: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString(locale);
}

function parsePage(value: string | null): number {
  const page = Number(value);
  return Number.isSafeInteger(page) && page > 0 ? page : 1;
}

function formatBytes(value: number, locale: string): string {
  const units = ["B", "KiB", "MiB", "GiB"];
  let amount = Math.max(0, value);
  let unit = 0;
  while (amount >= 1024 && unit < units.length - 1) {
    amount /= 1024;
    unit += 1;
  }
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: unit === 0 ? 0 : 1 }).format(amount)} ${units[unit]}`;
}

export function RepositoryBrowserPage({
  repositoryName,
  embedded = false,
}: { repositoryName?: string; embedded?: boolean } = {}) {
  const { t, i18n } = useTranslation();
  const { repo: routeRepository } = useParams<{ repo: string }>();
  const repo = repositoryName ?? routeRepository;
  const [searchParams, setSearchParams] = useSearchParams();
  const tabParam = searchParams.get(embedded ? "git_tab" : "tab");
  const tab = browserTabs.find((value) => value === tabParam) ?? "code";
  const gitRef = searchParams.get("ref") || "HEAD";
  const rawCommitPage = Number(searchParams.get("commitPage"));
  const commitPage =
    Number.isSafeInteger(rawCommitPage) &&
    rawCommitPage > 0 &&
    rawCommitPage <= maxCommitPage
      ? rawCommitPage
      : 1;
  const filePath = searchParams.get("file") || null;
  const dirPath =
    searchParams.get("dir") ?? (filePath ? parentPath(filePath) : "");
  const refPage = parsePage(searchParams.get("refPage"));
  const refSearch = searchParams.get("refSearch")?.trim() ?? "";
  const treePage = parsePage(searchParams.get("treePage"));
  const treeSearch = searchParams.get("treeSearch")?.trim() ?? "";

  if (!repo)
    return (
      <p className="text-sm text-text-muted">{t("repositories.notFound")}</p>
    );

  function updateParams(
    changes: Record<string, string | null>,
    options?: { replace?: boolean },
  ) {
    const next = new URLSearchParams(searchParams);
    for (const [key, value] of Object.entries(changes)) {
      const target = embedded && key === "tab" ? "git_tab" : key;
      if (value) next.set(target, value);
      else next.delete(target);
    }
    setSearchParams(next, options);
  }

  return (
    <div className="space-y-6">
      {!embedded && (
        <>
          <div className="flex items-center gap-2 text-sm text-text-muted">
            <TextLink asChild>
              <Link
                to="/repositories"
                className="inline-flex min-h-10 items-center"
              >
                {t("navigation.repositories")}
              </Link>
            </TextLink>
            <ChevronRight className="h-3 w-3" />
            <span className="min-w-0 break-all">{repo}</span>
          </div>
          <PageHeader
            title={repo}
            leading={
              <>
                <GitBranch className="h-6 w-6 text-accent" />
              </>
            }
            actions={
              <>
                <div className="flex flex-wrap gap-2">
                  <Button asChild variant="outline" size="sm">
                    <Link
                      to={`/repositories/${encodeURIComponent(repo)}/compare`}
                    >
                      <GitCompareArrows className="h-4 w-4" />{" "}
                      {t("repositoryBrowser.compareChanges")}
                    </Link>
                  </Button>
                  <Button asChild size="sm">
                    <Link
                      to={`/repositories/${encodeURIComponent(repo)}/pulls`}
                    >
                      <GitPullRequest className="h-4 w-4" />{" "}
                      {t("repositoryBrowser.createPullRequest")}
                    </Link>
                  </Button>
                </div>
              </>
            }
          />
        </>
      )}

      <Tabs
        value={tab}
        onValueChange={(value) =>
          updateParams({
            tab: value === "code" ? null : value,
            refPage: null,
            refSearch: null,
          })
        }
      >
        <TabsList className="grid h-auto w-full grid-cols-3 gap-1 sm:inline-flex sm:w-auto">
          <TabsTrigger value="code" className="min-w-0">
            {t("repositoryBrowser.code", "Код")}
          </TabsTrigger>
          <TabsTrigger value="commits" className="min-w-0">
            {t("repositoryBrowser.commits")}
          </TabsTrigger>
          <TabsTrigger value="branches" className="min-w-0">
            {t("repositoryBrowser.branches")}
          </TabsTrigger>
          <TabsTrigger value="tags" className="min-w-0">
            {t("repositoryBrowser.tags", "Теги")}
          </TabsTrigger>
          <TabsTrigger value="releases" className="min-w-0">
            {t("repositoryBrowser.releases", "Релизы")}
          </TabsTrigger>
        </TabsList>

        <TabsContent value="code" className="mt-4">
          <CodeBrowser
            repo={repo}
            gitRef={gitRef}
            dirPath={dirPath}
            filePath={filePath}
            treePage={treePage}
            treeSearch={treeSearch}
            onNavigate={updateParams}
          />
        </TabsContent>
        <TabsContent value="commits" className="mt-4">
          <CommitsList
            repo={repo}
            gitRef={gitRef}
            locale={i18n.language}
            page={commitPage}
            onPageChange={(page, replace) =>
              updateParams(
                { commitPage: page === 1 ? null : String(page) },
                { replace },
              )
            }
          />
        </TabsContent>
        <TabsContent value="branches" className="mt-4">
          <BranchesList
            repo={repo}
            page={refPage}
            search={refSearch}
            onNavigate={updateParams}
            onSelect={(ref) =>
              updateParams({
                tab: null,
                ref: `refs/heads/${ref.name}`,
                dir: null,
                file: null,
                refPage: null,
                refSearch: null,
                treePage: null,
                treeSearch: null,
              })
            }
          />
        </TabsContent>
        <TabsContent value="tags" className="mt-4">
          <TagsList
            repo={repo}
            page={refPage}
            search={refSearch}
            onNavigate={updateParams}
            onSelect={(tag) =>
              updateParams({
                tab: null,
                ref: `refs/tags/${tag.name}`,
                dir: null,
                file: null,
                refPage: null,
                refSearch: null,
                treePage: null,
                treeSearch: null,
              })
            }
          />
        </TabsContent>
        <TabsContent value="releases" className="mt-4">
          <ReleasesList repo={repo} />
        </TabsContent>
      </Tabs>
    </div>
  );
}

function CommitsList({
  repo,
  gitRef,
  locale,
  page,
  onPageChange,
}: {
  repo: string;
  gitRef: string;
  locale: string;
  page: number;
  onPageChange: (page: number, replace?: boolean) => void;
}) {
  const { t } = useTranslation();
  const query = useRepositoryCommits(repo, gitRef, page, commitPageSize);

  useEffect(() => {
    if (page > 1 && query.data?.items.length === 0) onPageChange(1, true);
  }, [onPageChange, page, query.data]);

  if (query.error && page > 1) {
    return (
      <Notice
        role="alert"
        tone="warning"
        appearance="panel"
        actions={
          <>
            <Button
              type="button"
              size="sm"
              variant="outline"

              onClick={() => void query.refetch()}
            >
              {t("common.retry")}
            </Button>
            <Button
              type="button"
              size="sm"
              variant="ghost"

              onClick={() => onPageChange(page - 1)}
            >
              {t("repositoryBrowser.previousCommits")}
            </Button>
          </>
        }
      >
        <span>{t("repositoryBrowser.commitsLoadError")}</span>
      </Notice>
    );
  }

  return (
    <QueryState
      data={query.data}
      isLoading={query.isLoading}
      error={query.error}
      errorMessage={t("repositoryBrowser.commitsLoadError")}
      onRetry={() => void query.refetch()}
      isEmpty={(result) => result.items.length === 0}
      empty={{ title: t("repositoryBrowser.noCommits") }}
    >
      {(result) => (
        <section className="space-y-3" aria-busy={query.isFetching}>
          <ResourceList asChild variant="bordered">
            <ul>
              {result.items.map((commit) => (
                <ResourceListRow asChild key={commit.sha}>
                  <li className="flex min-w-0 flex-col gap-1 text-sm sm:flex-row sm:items-center sm:gap-4">
                    <code className="shrink-0 text-xs text-accent">
                      {commit.short_sha}
                    </code>
                    <ResourceListTitle asChild className="min-w-0 flex-1 break-words"><span >
                      {commit.message}
                    </span></ResourceListTitle>
                    <ResourceListDescription asChild className="min-w-0 break-words"><span >
                      {commit.author}
                    </span></ResourceListDescription>
                    <time
                      className="shrink-0 text-xs text-text-muted"
                      dateTime={commit.date}
                    >
                      {formatDate(commit.date, locale)}
                    </time>
                  </li>
                </ResourceListRow>
              ))}
            </ul>
          </ResourceList>
          {(page > 1 || result.hasMore) && (
            <PageNavigation
              page={page}
              hasNext={result.hasMore && !query.isFetching}
              onPageChange={onPageChange}
              label={t("repositoryBrowser.commitPages")}
              previousLabel={t("repositoryBrowser.previousCommits")}
              nextLabel={t("repositoryBrowser.nextCommits")}
              pending={query.isFetching}
              summary={t("repositoryBrowser.commitPage", { page })}
            />
          )}
        </section>
      )}
    </QueryState>
  );
}

type NavigateParams = (changes: Record<string, string | null>) => void;

function RefListSearch({
  search,
  onSearch,
}: {
  search: string;
  onSearch: (search: string) => void;
}) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState(search);

  useEffect(() => setDraft(search), [search]);

  return (
    <form
      className="flex min-w-0 items-center gap-2"
      role="search"
      onSubmit={(event) => {
        event.preventDefault();
        onSearch(draft.trim());
      }}
    >
      <div className="min-w-0 flex-1 sm:max-w-sm">
        <SearchInput
          aria-label={t("repositoryBrowser.searchRefs")}
          placeholder={t("repositoryBrowser.searchRefs")}
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
        />
      </div>
      <Button
        type="submit"
        variant="outline"
        size="icon"

        aria-label={t("repositoryBrowser.applyRefSearch")}
        title={t("repositoryBrowser.applyRefSearch")}
      >
        <Search className="h-4 w-4" aria-hidden />
      </Button>
      {(draft || search) && (
        <Button
          type="button"
          variant="ghost"
          size="icon"

          aria-label={t("repositoryBrowser.clearRefSearch")}
          title={t("repositoryBrowser.clearRefSearch")}
          onClick={() => {
            setDraft("");
            onSearch("");
          }}
        >
          <X className="h-4 w-4" aria-hidden />
        </Button>
      )}
    </form>
  );
}

function BranchesList({
  repo,
  page,
  search,
  onNavigate,
  onSelect,
}: {
  repo: string;
  page: number;
  search: string;
  onNavigate: NavigateParams;
  onSelect: (ref: RepositoryRef) => void;
}) {
  const { t } = useTranslation();
  const query = useRepositoryRefs(repo, {
    kind: "branch",
    limit: refPageSize + 1,
    offset: (page - 1) * refPageSize,
    search,
  });
  const allBranches = query.data?.filter((ref) => ref.kind === "branch");
  const branches = allBranches?.slice(0, refPageSize);
  const hasNext = (allBranches?.length ?? 0) > refPageSize;
  return (
    <div className="space-y-2">
      <RefListSearch
        search={search}
        onSearch={(value) =>
          onNavigate({ refSearch: value || null, refPage: null })
        }
      />
      <QueryState
        data={branches}
        isLoading={query.isLoading}
        error={query.error}
        errorMessage={t("repositoryBrowser.branchesLoadError")}
        onRetry={() => void query.refetch()}
        isEmpty={(items) => items.length === 0}
        empty={{
          title: search
            ? t("repositoryBrowser.noBranchMatches")
            : t("repositoryBrowser.noBranches"),
        }}
      >
        {(items) => (
          <ResourceList asChild variant="bordered">
            <ul>
              {items.map((ref) => (
                <ResourceListRow asChild key={ref.name}>
                  <li className="flex min-w-0 flex-wrap items-center gap-x-4 gap-y-1 text-sm">
                    <Button
                      variant="ghost"
                      type="button"
                      className="min-w-0 flex-1 text-left"
                      onClick={() => onSelect(ref)}
                    >
                      <GitBranch
                        className="h-4 w-4 shrink-0 text-accent"
                        aria-hidden
                      />
                      <span className="break-all">{ref.name}</span>
                    </Button>
                    <code className="text-xs text-text-muted">
                      {ref.sha.slice(0, 7)}
                    </code>
                    {ref.target && (
                      <ResourceListDescription asChild className="w-full break-words sm:w-auto sm:max-w-[40%] sm:truncate"><span >
                        {ref.target}
                      </span></ResourceListDescription>
                    )}
                  </li>
                </ResourceListRow>
              ))}
            </ul>
          </ResourceList>
        )}
      </QueryState>
      <PageNavigation
        page={page}
        hasNext={hasNext}
        onPageChange={(nextPage) =>
          onNavigate({ refPage: nextPage > 1 ? String(nextPage) : null })
        }
        compact
        hideSinglePage
        label={t("repositoryBrowser.refPagination")}
        previousLabel={t("repositoryBrowser.previousRefPage")}
        nextLabel={t("repositoryBrowser.nextRefPage")}
        className="justify-center border-t border-border pt-2"
      />
    </div>
  );
}

function CodeBrowser({
  repo,
  gitRef,
  dirPath,
  filePath,
  treePage,
  treeSearch,
  onNavigate,
}: {
  repo: string;
  gitRef: string;
  dirPath: string;
  filePath: string | null;
  treePage: number;
  treeSearch: string;
  onNavigate: NavigateParams;
}) {
  const { t } = useTranslation();
  const [draftRef, setDraftRef] = useState(gitRef === "HEAD" ? "" : gitRef);
  const deferredRef = useDebouncedValue(
    draftRef.trim().replace(/^refs\/(?:heads|tags)\//, ""),
  );
  const refsQuery = useRepositoryRefs(repo, {
    limit: refSuggestionLimit,
    search: deferredRef,
  });
  const refs =
    refsQuery.data
      ?.filter((ref) => ref.kind === "branch" || ref.kind === "tag")
      .slice(0, refSuggestionLimit - 1) ?? [];
  const refOptions = refs.map((ref) => ({
    value: `refs/${ref.kind === "branch" ? "heads" : "tags"}/${ref.name}`,
    label: `${ref.kind === "branch" ? t("repositoryBrowser.branches") : t("repositoryBrowser.tags")} / ${ref.name}`,
  }));

  useEffect(() => setDraftRef(gitRef === "HEAD" ? "" : gitRef), [gitRef]);

  function applyRef(event: React.FormEvent) {
    event.preventDefault();
    const value = draftRef.trim();
    onNavigate({
      ref: value && value !== "HEAD" ? value : null,
      dir: null,
      file: null,
      treePage: null,
      treeSearch: null,
    });
  }

  return (
    <div className="space-y-3">
      <form
        className="flex min-w-0 flex-wrap items-end gap-2"
        role="search"
        aria-label={t("repositoryBrowser.refPicker")}
        onSubmit={applyRef}
      >
        <div className="min-w-0 flex-1 space-y-1.5 sm:max-w-md">
          <Label htmlFor="repository-ref">
            {t("repositoryBrowser.gitRef")}
          </Label>
          <AutocompleteInput
            id="repository-ref"
            type="search"
            options={[
              { value: "HEAD", label: t("repositoryBrowser.defaultRef") },
              ...(refOptions.map((option) => ({ value: option.value, label: option.label })) ??
                []),
            ]}
            className="font-mono"
            placeholder={t("repositoryBrowser.defaultRef")}
            value={draftRef}
            onValueChange={(nextValue) => setDraftRef(nextValue)}
          />

        </div>
        <Button
          type="submit"
          variant="outline"
          size="icon"

          aria-label={t("repositoryBrowser.applyRef")}
          title={t("repositoryBrowser.applyRef")}
        >
          <Check className="h-4 w-4" aria-hidden />
        </Button>
      </form>
      {refsQuery.isLoading && (
        <Notice role="status" tone="neutral" appearance="inline">
          {t("repositoryBrowser.loadingRefs")}
        </Notice>
      )}
      {(refsQuery.data?.length ?? 0) > refSuggestionLimit - 1 && (
        <Notice role="status" tone="warning" appearance="inline">
          {t("repositoryBrowser.refSuggestionsLimited")}
        </Notice>
      )}
      {Boolean(refsQuery.error) && (
        <Notice
          role="alert"

          tone="warning"
          appearance="panel"
          actions={
            <Button
              type="button"
              size="sm"
              variant="outline"

              onClick={() => void refsQuery.refetch()}
            >
              {t("common.retry")}
            </Button>
          }
        >
          <span>{t("repositoryBrowser.refsUnavailable")}</span>
        </Notice>
      )}
      {filePath ? (
        <FilePreview
          repo={repo}
          gitRef={gitRef}
          filePath={filePath}
          onBack={() => onNavigate({ file: null, dir: parentPath(filePath) })}
        />
      ) : (
        <TreeView
          repo={repo}
          gitRef={gitRef}
          dirPath={dirPath}
          page={treePage}
          search={treeSearch}
          onNavigate={onNavigate}
        />
      )}
    </div>
  );
}

function FilePreview({
  repo,
  gitRef,
  filePath,
  onBack,
}: {
  repo: string;
  gitRef: string;
  filePath: string;
  onBack: () => void;
}) {
  const { t, i18n } = useTranslation();
  const query = useRepositoryBlob(repo, gitRef, filePath);
  return (
    <Card>
      <CardHeader className="min-w-0 flex-row flex-wrap items-center gap-2 text-sm">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="min-w-10"
          aria-label={t("repositoryBrowser.backToTree")}
          title={t("repositoryBrowser.backToTree")}
          onClick={onBack}
        >
          <ArrowLeft className="h-4 w-4" aria-hidden />
        </Button>
        <FileText className="h-4 w-4 shrink-0 text-accent" aria-hidden />
        <span className="min-w-0 flex-1 break-all font-medium">{filePath}</span>
        {query.data && (
          <span className="text-xs text-text-muted">
            {formatBytes(query.data.size, i18n.language)} ·{" "}
            {query.data.sha.slice(0, 7)}
          </span>
        )}
      </CardHeader>
      <CardContent>
        <QueryState
          data={query.data}
          isLoading={query.isLoading}
          error={query.error}
          errorMessage={t("repositoryBrowser.fileLoadError")}
          onRetry={() => void query.refetch()}
        >
          {(blob) =>
            blob.binary ? (
              <p className="text-sm text-text-muted">
                {t("repositoryBrowser.binaryFile")}
              </p>
            ) : blob.content.length === 0 ? (
              <p className="text-sm text-text-muted">
                {t("repositoryBrowser.emptyFile")}
              </p>
            ) : (
              <div>
                {blob.truncated && (
                  <Notice
                    role="status"
                    className="mb-3"
                    tone="warning"
                    appearance="inline"
                  >
                    {t("repositoryBrowser.truncatedFile")}
                  </Notice>
                )}
                <CodeBlock
                  tabIndex={0}
                  aria-label={t("repositoryBrowser.filePreview", {
                    path: filePath,
                  })}
                  className="max-h-[70vh]"
                  wrap={false}
                >
                  <code>{blob.content}</code>
                </CodeBlock>
              </div>
            )
          }
        </QueryState>
      </CardContent>
    </Card>
  );
}

function TreeView({
  repo,
  gitRef,
  dirPath,
  page,
  search,
  onNavigate,
}: {
  repo: string;
  gitRef: string;
  dirPath: string;
  page: number;
  search: string;
  onNavigate: NavigateParams;
}) {
  const { t } = useTranslation();
  const [draftSearch, setDraftSearch] = useState(search);
  const query = useRepositoryTree(repo, gitRef, dirPath || undefined, {
    limit: treePageSize + 1,
    offset: (page - 1) * treePageSize,
    search,
  });
  const crumbs = dirPath ? dirPath.split("/") : [];
  const entries = query.data?.slice(0, treePageSize);
  const hasNext = (query.data?.length ?? 0) > treePageSize;

  useEffect(() => setDraftSearch(search), [search]);

  function submitSearch(event: React.FormEvent) {
    event.preventDefault();
    const value = draftSearch.trim();
    onNavigate({ treeSearch: value || null, treePage: null });
  }

  function clearSearch() {
    setDraftSearch("");
    onNavigate({ treeSearch: null, treePage: null });
  }

  return (
    <Card>
      <CardHeader>
        <nav
          aria-label={t("repositoryBrowser.code")}
          className="flex min-w-0 flex-wrap items-center gap-1 text-sm"
        >
          <Folder className="h-4 w-4 shrink-0 text-accent" aria-hidden />
          <Button
            variant="ghost"
            type="button"
            size="icon"
            onClick={() =>
              onNavigate({
                dir: null,
                file: null,
                treePage: null,
                treeSearch: null,
              })
            }
          >
            /
          </Button>
          {crumbs.map((part, index) => (
            <span
              key={`${index}-${part}`}
              className="flex min-w-0 items-center gap-1"
            >
              <span className="text-text-muted">/</span>
              <Button
                variant="ghost"
                type="button"
                className="min-w-0 break-all text-left"
                onClick={() =>
                  onNavigate({
                    dir: crumbs.slice(0, index + 1).join("/"),
                    file: null,
                    treePage: null,
                    treeSearch: null,
                  })
                }
              >
                {part}
              </Button>
            </span>
          ))}
        </nav>
      </CardHeader>
      <CardContent className="space-y-2">
        <form
          className="flex min-w-0 items-center gap-2"
          role="search"
          onSubmit={submitSearch}
        >
          <div className="min-w-0 flex-1 sm:max-w-sm">
            <SearchInput
              aria-label={t("repositoryBrowser.searchTree")}
              placeholder={t("repositoryBrowser.searchTree")}
              value={draftSearch}
              onChange={(event) => setDraftSearch(event.target.value)}
            />
          </div>
          <Button
            type="submit"
            variant="outline"
            size="icon"

            aria-label={t("repositoryBrowser.applyTreeSearch")}
            title={t("repositoryBrowser.applyTreeSearch")}
          >
            <Search className="h-4 w-4" aria-hidden />
          </Button>
          {(draftSearch || search) && (
            <Button
              type="button"
              variant="ghost"
              size="icon"

              aria-label={t("repositoryBrowser.clearTreeSearch")}
              title={t("repositoryBrowser.clearTreeSearch")}
              onClick={clearSearch}
            >
              <X className="h-4 w-4" aria-hidden />
            </Button>
          )}
        </form>
        <QueryState
          data={entries}
          isLoading={query.isLoading}
          error={query.error}
          errorMessage={t("repositoryBrowser.codeLoadError")}
          onRetry={() => void query.refetch()}
          isEmpty={(items) => items.length === 0}
          empty={{
            title: search
              ? t("repositoryBrowser.noTreeMatches")
              : t("repositoryBrowser.emptyTree"),
          }}
        >
          {(items) => (
            <ul className="divide-y divide-border">
              {items.map((entry) => (
                <li
                  key={entry.path}
                  className="flex min-w-0 items-center gap-2 text-sm hover:bg-surface-raised"
                >
                  <Button
                    variant="ghost"
                    type="button"
                    className="flex min-w-0 flex-1 items-center gap-3 text-left"
                    onClick={() =>
                      onNavigate(
                        entry.kind === "tree"
                          ? {
                              dir: entry.path,
                              file: null,
                              treePage: null,
                              treeSearch: null,
                            }
                          : { file: entry.path },
                      )
                    }
                  >
                    {entry.kind === "tree" ? (
                      <Folder
                        className="h-4 w-4 shrink-0 text-accent"
                        aria-hidden
                      />
                    ) : (
                      <FileText
                        className="h-4 w-4 shrink-0 text-text-muted"
                        aria-hidden
                      />
                    )}
                    <span className="min-w-0 break-all">{entry.name}</span>
                  </Button>
                  <code className="hidden shrink-0 pr-2 text-xs text-text-muted sm:inline">
                    {entry.sha.slice(0, 7)}
                  </code>
                  {entry.size != null && (
                    <span className="hidden w-20 shrink-0 pr-2 text-right text-xs text-text-muted md:inline">
                      {entry.size} B
                    </span>
                  )}
                </li>
              ))}
            </ul>
          )}
        </QueryState>
        {(page > 1 || hasNext) && (
          <PageNavigation
            page={page}
            hasNext={hasNext}
            onPageChange={(nextPage) =>
              onNavigate({ treePage: nextPage === 1 ? null : String(nextPage) })
            }
            label={t("repositoryBrowser.treePagination")}
            previousLabel={t("repositoryBrowser.previousTreePage")}
            nextLabel={t("repositoryBrowser.nextTreePage")}
            compact
            summary={t("repositoryBrowser.treePage", { page })}
            className="border-t border-border pt-2"
          />
        )}
      </CardContent>
    </Card>
  );
}

function TagsList({
  repo,
  page,
  search,
  onNavigate,
  onSelect,
}: {
  repo: string;
  page: number;
  search: string;
  onNavigate: NavigateParams;
  onSelect: (tag: { name: string }) => void;
}) {
  const { t } = useTranslation();
  const query = useRepositoryTags(repo, {
    limit: refPageSize + 1,
    offset: (page - 1) * refPageSize,
    search,
  });
  const tags = query.data?.slice(0, refPageSize);
  const hasNext = (query.data?.length ?? 0) > refPageSize;
  return (
    <div className="space-y-2">
      <RefListSearch
        search={search}
        onSearch={(value) =>
          onNavigate({ refSearch: value || null, refPage: null })
        }
      />
      <QueryState
        data={tags}
        isLoading={query.isLoading}
        error={query.error}
        errorMessage={t("repositoryBrowser.tagsLoadError")}
        onRetry={() => void query.refetch()}
        isEmpty={(items) => items.length === 0}
        empty={{
          title: search
            ? t("repositoryBrowser.noTagMatches")
            : t("repositoryBrowser.noTags"),
        }}
      >
        {(items) => (
          <ResourceList asChild variant="bordered">
            <ul>
              {items.map((tag) => (
                <ResourceListRow asChild key={tag.name}>
                  <li className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 text-sm">
                    <Button
                      variant="ghost"
                      type="button"
                      className="min-w-0 flex-1 text-left"
                      onClick={() => onSelect(tag)}
                    >
                      <Tag
                        className="h-4 w-4 shrink-0 text-accent"
                        aria-hidden
                      />
                      <span className="break-all">{tag.name}</span>
                    </Button>
                    <code className="text-xs text-text-muted">
                      {tag.sha.slice(0, 7)}
                    </code>
                    {tag.message && (
                      <ResourceListDescription asChild className="w-full min-w-0 break-words sm:w-auto sm:max-w-[40%] sm:truncate"><span >
                        {tag.message}
                      </span></ResourceListDescription>
                    )}
                  </li>
                </ResourceListRow>
              ))}
            </ul>
          </ResourceList>
        )}
      </QueryState>
      <PageNavigation
        page={page}
        hasNext={hasNext}
        onPageChange={(nextPage) =>
          onNavigate({ refPage: nextPage > 1 ? String(nextPage) : null })
        }
        compact
        hideSinglePage
        label={t("repositoryBrowser.refPagination")}
        previousLabel={t("repositoryBrowser.previousRefPage")}
        nextLabel={t("repositoryBrowser.nextRefPage")}
        className="justify-center border-t border-border pt-2"
      />
    </div>
  );
}

function ReleasesList({ repo }: { repo: string }) {
  const { t, i18n } = useTranslation();
  const releasesQuery = useReleases(repo);
  const createRelease = useCreateRelease(repo);
  const deleteRelease = useDeleteRelease(repo);
  const [open, setOpen] = useState(false);
  const [editingTag, setEditingTag] = useState<string | null>(null);
  const [pendingDelete, setPendingDelete] = useState<string | null>(null);
  const [tagError, setTagError] = useState<string | null>(null);
  const [form, setForm] = useState({
    tag_name: "",
    name: "",
    description: "",
    prerelease: false,
  });
  const deferredTag = useDebouncedValue(form.tag_name.trim());
  const tagsQuery = useRepositoryTags(repo, {
    limit: refSuggestionLimit,
    search: deferredTag,
  });

  function openCreate() {
    setEditingTag(null);
    setTagError(null);
    setForm({ tag_name: "", name: "", description: "", prerelease: false });
    setOpen(true);
  }

  function openEdit(release: Release) {
    setEditingTag(release.tag_name);
    setTagError(null);
    setForm({
      tag_name: release.tag_name,
      name: release.name,
      description: release.description,
      prerelease: release.prerelease,
    });
    setOpen(true);
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!editingTag && (!releasesQuery.data || releasesQuery.error)) {
      setTagError(t("releases.listUnavailable"));
      return;
    }
    const tagName = form.tag_name.trim();
    if (!tagName) {
      setTagError(t("releases.tagRequired"));
      return;
    }
    if (
      !editingTag &&
      releasesQuery.data?.some((release) => release.tag_name === tagName)
    ) {
      setTagError(t("releases.alreadyExists"));
      return;
    }
    setTagError(null);
    createRelease.mutate(
      {
        tag_name: tagName,
        name: form.name.trim() || tagName,
        description: form.description.trim(),
        prerelease: form.prerelease,
      },
      {
        onSuccess: () => {
          toast.success(
            t(editingTag ? "releases.updated" : "releases.created"),
          );
          setOpen(false);
        },
        onError: (err) => toast.error(err.message),
      },
    );
  }

  return (
    <div className="space-y-3">
      <div className="flex justify-end">
        <Button
          type="button"

          disabled={!releasesQuery.data || Boolean(releasesQuery.error)}
          onClick={openCreate}
        >
          <Plus className="h-4 w-4" aria-hidden />
          {t("releases.create")}
        </Button>
      </div>
      <QueryState
        data={releasesQuery.data}
        isLoading={releasesQuery.isLoading}
        error={releasesQuery.error}
        errorMessage={t("repositoryBrowser.releasesLoadError")}
        onRetry={() => void releasesQuery.refetch()}
        isEmpty={(releases) => releases.length === 0}
        empty={{ title: t("releases.none") }}
      >
        {(releases) => (
          <ResourceList asChild variant="bordered">
            <ul>
              {releases.map((rel) => (
                <ResourceListRow asChild key={rel.id}>
                  <li className="flex min-w-0 flex-wrap items-start gap-x-3 gap-y-1 text-sm sm:flex-nowrap">
                    <Package
                      className="mt-1 h-4 w-4 shrink-0 text-accent"
                      aria-hidden
                    />
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
                        <ResourceListTitle asChild className="min-w-0 break-words"><span >
                          {rel.name}
                        </span></ResourceListTitle>
                        {rel.prerelease && (
                          <Badge tone="warning">
                            {t("releases.prerelease")}
                          </Badge>
                        )}
                      </div>
                      <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-text-muted">
                        <span className="flex min-w-0 items-center gap-1 break-all">
                          <Tag className="h-3 w-3 shrink-0" aria-hidden />
                          {rel.tag_name}
                        </span>
                        <time dateTime={rel.created_at}>
                          {formatDate(rel.created_at, i18n.language)}
                        </time>
                      </div>
                      {rel.description && (
                        <p className="mt-2 whitespace-pre-wrap break-words text-sm text-text-secondary">
                          {rel.description}
                        </p>
                      )}
                    </div>
                    <div className="flex w-full shrink-0 items-center justify-end gap-1 sm:w-auto">
                      <Button
                        variant="ghost"
                        type="button"
                        size="icon"
                        aria-label={`${t("releases.edit")} ${rel.tag_name}`}
                        title={`${t("releases.edit")} ${rel.tag_name}`}
                        onClick={() => openEdit(rel)}
                      >
                        <Pencil className="h-4 w-4" aria-hidden />
                      </Button>
                      <Button
                        size="icon"
                        variant="destructive-outline"
                        type="button"

                        aria-label={`${t("releases.deleteAction")} ${rel.tag_name}`}
                        title={`${t("releases.deleteAction")} ${rel.tag_name}`}
                        disabled={deleteRelease.isPending}
                        onClick={() => setPendingDelete(rel.tag_name)}
                      >
                        <Trash2 className="h-4 w-4" aria-hidden />
                      </Button>
                    </div>
                  </li>
                </ResourceListRow>
              ))}
            </ul>
          </ResourceList>
        )}
      </QueryState>

      <Dialog
        open={open}
        onOpenChange={(next) => {
          if (!createRelease.isPending) setOpen(next);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {t(editingTag ? "releases.edit" : "releases.create")}
            </DialogTitle>
          </DialogHeader>
          <form
            onSubmit={handleSubmit}
            aria-label={t(editingTag ? "releases.edit" : "releases.create")}
            className="grid gap-3"
          >
            <div className="space-y-1.5">
              <Label htmlFor="rel-tag">{t("releases.tag")}</Label>
              <AutocompleteInput
                id="rel-tag"
                value={form.tag_name}
                onValueChange={(nextValue) => {
                  setForm({ ...form, tag_name: nextValue });
                  setTagError(null);
                }}
                options={
                  tagsQuery.data
                    ?.slice(0, refSuggestionLimit - 1)
                    .map((tag) => ({ value: tag.name, label: tag.name })) ?? []
                }
                readOnly={!!editingTag}
                aria-invalid={!!tagError}
                aria-describedby={tagError ? "rel-tag-error" : undefined}
                placeholder="v1.0.0"
                required
              />

              {tagError && (
                <Notice
                  id="rel-tag-error"
                  role="alert"

                  tone="danger"
                  appearance="inline"
                >
                  {tagError}
                </Notice>
              )}
              {tagsQuery.isLoading && (
                <Notice role="status" tone="neutral" appearance="inline">
                  {t("repositoryBrowser.loadingRefs")}
                </Notice>
              )}
              {(tagsQuery.data?.length ?? 0) > refSuggestionLimit - 1 && (
                <Notice role="status" tone="warning" appearance="inline">
                  {t("repositoryBrowser.refSuggestionsLimited")}
                </Notice>
              )}
              {Boolean(tagsQuery.error) && (
                <Notice
                  role="alert"

                  tone="warning"
                  appearance="panel"
                  actions={
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"

                      onClick={() => void tagsQuery.refetch()}
                    >
                      {t("common.retry")}
                    </Button>
                  }
                >
                  <span>{t("repositoryBrowser.refsUnavailable")}</span>
                </Notice>
              )}
            </div>
            <FormField id="rel-name" label={t("releases.name")}>
              {(baseFieldAttrs) => (
                <Input
                  {...baseFieldAttrs}
                  value={form.name}
                  onChange={(e) => setForm({ ...form, name: e.target.value })}
                />
              )}
            </FormField>
            <FormField id="rel-desc" label={t("releases.description")}>
              {(baseFieldAttrs) => (
                <Textarea
                  {...baseFieldAttrs}
                  rows={4}
                  value={form.description}
                  onChange={(e) =>
                    setForm({ ...form, description: e.target.value })
                  }
                />
              )}
            </FormField>
            <div className="flex items-center gap-2">
              <Checkbox
                id="rel-pre"
                checked={form.prerelease}
                onCheckedChange={(checked) =>
                  setForm({ ...form, prerelease: checked === true })
                }
              />
              <Label htmlFor="rel-pre">{t("releases.prerelease")}</Label>
            </div>
            <DialogFooter>
              <Button
                type="button"
                variant="outline"
                disabled={createRelease.isPending}
                onClick={() => setOpen(false)}
              >
                {t("common.cancel")}
              </Button>
              <Button type="submit" disabled={createRelease.isPending}>
                {createRelease.isPending
                  ? t("common.saving")
                  : t(editingTag ? "common.save" : "common.create")}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={pendingDelete !== null}
        title={
          pendingDelete
            ? `${t("releases.deleteTitle")} «${pendingDelete}»?`
            : ""
        }
        description={t("releases.deleteConfirm")}
        isPending={deleteRelease.isPending}
        closeOnConfirm={false}
        onOpenChange={(open) => {
          if (open) return;
          setPendingDelete(null);
        }}
        onConfirm={() => {
          if (!pendingDelete) return;
          deleteRelease.mutate(pendingDelete, {
            onSuccess: () => {
              setPendingDelete(null);
              toast.success(t("releases.deleted"));
            },
            onError: (error) => toast.error(error.message),
          });
        }}
        confirmLabel={t("common.delete")}
        pendingLabel={t("common.delete")}
      />
    </div>
  );
}
