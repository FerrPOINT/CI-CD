import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router";
import { History, Search } from "lucide-react";

import { useAuditLog } from "@/api/hooks";
import { QueryState } from "@/shared/ui/query-state";
import { ResourceListTitle,
  DescriptionItem,
  PageNavigation,
  PageHeader,
  Select,
  InitialsAvatar as UserAvatar,
  Button,
  Card,
  Label,
  ResourceList,
  ResourceListRow,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
  SearchInput,
  Notice,
  EmptyState,
} from "@pdlc/ui/ui";

const pageSize = 20;
const maxAuditPage = Math.floor(2_147_483_647 / pageSize) + 1;

export function AuditLogPage() {
  const { t, i18n } = useTranslation();
  const [searchParams, setSearchParams] = useSearchParams();
  const pageParam = searchParams.get("page");
  const rawPage = Number(pageParam);
  const validPage =
    pageParam === null ||
    (Number.isSafeInteger(rawPage) && rawPage > 0 && rawPage <= maxAuditPage);
  const page = pageParam !== null && validPage ? rawPage : 1;
  const action = searchParams.get("action")?.trim() ?? "";
  const query = searchParams.get("q")?.trim() ?? "";
  const [searchDraft, setSearchDraft] = useState(query);
  const auditQuery = useAuditLog({
    limit: pageSize,
    offset: (page - 1) * pageSize,
    action: action || undefined,
    q: query || undefined,
  });
  const hasFilters = Boolean(action || query);
  const timeFormatter = new Intl.DateTimeFormat(i18n.language, {
    dateStyle: "short",
    timeStyle: "short",
  });

  useEffect(() => setSearchDraft(query), [query]);

  useEffect(() => {
    if (validPage) return;
    setSearchParams(
      (current) => {
        const next = new URLSearchParams(current);
        next.delete("page");
        return next;
      },
      { replace: true },
    );
  }, [setSearchParams, validPage]);

  useEffect(() => {
    if (!auditQuery.data || auditQuery.isPlaceholderData) return;
    const lastPage = Math.max(1, Math.ceil(auditQuery.data.total / pageSize));
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
  }, [auditQuery.data, auditQuery.isPlaceholderData, page, setSearchParams]);

  function changePage(nextPage: number) {
    setSearchParams((current) => {
      const next = new URLSearchParams(current);
      if (nextPage <= 1) next.delete("page");
      else next.set("page", String(nextPage));
      return next;
    });
  }

  function changeAction(nextAction: string) {
    setSearchParams(
      (current) => {
        const next = new URLSearchParams(current);
        if (nextAction) next.set("action", nextAction);
        else next.delete("action");
        next.delete("page");
        return next;
      },
      { replace: true },
    );
  }

  function submitSearch(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const nextQuery = searchDraft.trim();
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
  }

  function clearSearch() {
    setSearchDraft("");
    setSearchParams(
      (current) => {
        const next = new URLSearchParams(current);
        next.delete("q");
        next.delete("page");
        return next;
      },
      { replace: true },
    );
  }

  return (
    <div className="space-y-4">
      <PageHeader
        title={t("auditLog.title")}
        leading={
          <>
            <History className="h-5 w-5 text-accent" aria-hidden />
          </>
        }
      />

      {auditQuery.error && auditQuery.data && (
        <Notice
          role="alert"

          tone="warning"
          appearance="panel"
          actions={
            <Button
              type="button"
              size="sm"
              variant="outline"

              onClick={() => void auditQuery.refetch()}
            >
              {t("common.retry")}
            </Button>
          }
        >
          <span>{t("auditLog.refreshFailed")}</span>
        </Notice>
      )}

      <QueryState
        data={auditQuery.data}
        isLoading={auditQuery.isLoading}
        error={auditQuery.data ? null : auditQuery.error}
        errorMessage={t("auditLog.loadFailed")}
        onRetry={() => void auditQuery.refetch()}
        isEmpty={(result) => result.total === 0 && !hasFilters}
        empty={{ title: t("auditLog.empty") }}
      >
        {(result) => {
          const pageCount = Math.max(1, Math.ceil(result.total / pageSize));
          const firstItem = result.total === 0 ? 0 : result.offset + 1;
          const lastItem = result.offset + result.items.length;
          const actionOptions =
            action && !result.actions.includes(action)
              ? [action, ...result.actions]
              : result.actions;

          return (
            <section
              aria-label={t("auditLog.title")}
              aria-busy={auditQuery.isFetching}
              className="space-y-3"
            >
              <div className="grid gap-2 lg:grid-cols-[minmax(18rem,1fr)_minmax(12rem,18rem)]">
                <form
                  onSubmit={submitSearch}
                  role="search"
                  className="flex min-w-0 gap-2"
                >
                  <div className="min-w-0 flex-1">
                    <SearchInput
                      aria-label={t("auditLog.search")}
                      placeholder={t("auditLog.search")}
                      maxLength={128}
                      value={searchDraft}
                      onChange={(event) => setSearchDraft(event.target.value)}
                      clear={
                        searchDraft || query
                          ? {
                              label: t("auditLog.clearSearch"),
                              onClear: clearSearch,
                            }
                          : undefined
                      }
                    />
                  </div>
                  <Button
                    type="submit"
                    size="sm"

                    disabled={searchDraft.trim() === query}
                  >
                    <Search className="h-4 w-4" aria-hidden />
                    {t("auditLog.searchButton")}
                  </Button>
                </form>
                <div>
                  <Label htmlFor="audit-action" className="sr-only">
                    {t("auditLog.actionFilter")}
                  </Label>
                  <Select
                    id="audit-action"
                    value={action}
                    onValueChange={(nextValue) => changeAction(nextValue)}
                    className="w-full min-w-0"
                    options={[
                      { value: "", label: String(t("auditLog.allActions")) },
                      ...(actionOptions.map((value) => ({
                        value: String(value),
                        label: String(
                          t(`auditLog.actions.${value}`, {
                            defaultValue: value,
                          }),
                        ),
                      })) ?? []),
                    ]}
                  />
                </div>
              </div>

              <p
                className="flex flex-wrap gap-2 text-xs text-text-muted"
                aria-live="polite"
              >
                <span>
                  {t("auditLog.shown", {
                    from: firstItem,
                    to: lastItem,
                    total: result.total,
                  })}
                </span>
                {auditQuery.isFetching && <span>{t("common.loading")}</span>}
              </p>

              {result.total === 0 ? (
                <div role="status">
                  <EmptyState message={t("auditLog.noMatches")} />
                </div>
              ) : (
                <>
                  <ResourceList asChild>
                    <ul className="md:hidden">
                      {result.items.map((event) => (
                        <ResourceListRow asChild key={event.id}>
                          <li className="min-w-0 space-y-2 text-sm">
                            <div className="flex min-w-0 items-start justify-between gap-3">
                              <ResourceListTitle asChild className="min-w-0 break-words"><span >
                                {t(`auditLog.actions.${event.action}`, {
                                  defaultValue: event.action,
                                })}
                              </span></ResourceListTitle>
                              <time
                                className="shrink-0 text-xs text-text-muted"
                                dateTime={event.created_at}
                                title={event.created_at}
                              >
                                {timeFormatter.format(
                                  new Date(event.created_at),
                                )}
                              </time>
                            </div>
                            <dl className="min-w-0 space-y-1">
                              <DescriptionItem
                                label={<>{t("auditLog.resource")}</>}
                                layout="inline"
                              >
                                {t(
                                  `auditLog.resources.${event.resource_type}`,
                                  { defaultValue: event.resource_type },
                                )}
                                {event.resource_id && (
                                  <span className="ml-2 font-mono">
                                    {event.resource_id.slice(0, 8)}
                                  </span>
                                )}
                              </DescriptionItem>
                              <DescriptionItem
                                label={<>{t("auditLog.actor")}</>}
                                layout="inline"
                              >
                                {event.actor || "—"}
                              </DescriptionItem>
                            </dl>
                          </li>
                        </ResourceListRow>
                      ))}
                    </ul>
                  </ResourceList>
                  <Card className="hidden overflow-hidden md:block">
                    <Table>
                      <TableHeader>
                        <TableRow>
                          <TableHead>{t("auditLog.action")}</TableHead>
                          <TableHead>{t("auditLog.resource")}</TableHead>
                          <TableHead>{t("auditLog.actor")}</TableHead>
                          <TableHead>{t("auditLog.time")}</TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {result.items.map((event) => (
                          <TableRow key={event.id}>
                            <TableCell className="text-xs">
                              {t(`auditLog.actions.${event.action}`, {
                                defaultValue: event.action,
                              })}
                            </TableCell>
                            <TableCell className="text-xs">
                              <span className="text-text-muted">
                                {t(
                                  `auditLog.resources.${event.resource_type}`,
                                  { defaultValue: event.resource_type },
                                )}
                              </span>
                              {event.resource_id && (
                                <span className="ml-2 font-mono text-text-muted">
                                  {event.resource_id.slice(0, 8)}
                                </span>
                              )}
                            </TableCell>
                            <TableCell className="text-xs text-text-muted">
                              {event.actor ? (
                                <span className="inline-flex min-w-0 items-center gap-1.5">
                                  <UserAvatar name={event.actor} size="xs" />
                                  <span className="min-w-0 break-all">
                                    {event.actor}
                                  </span>
                                </span>
                              ) : (
                                "—"
                              )}
                            </TableCell>
                            <TableCell className="text-xs text-text-muted">
                              <time
                                dateTime={event.created_at}
                                title={event.created_at}
                              >
                                {timeFormatter.format(
                                  new Date(event.created_at),
                                )}
                              </time>
                            </TableCell>
                          </TableRow>
                        ))}
                      </TableBody>
                    </Table>
                  </Card>
                </>
              )}

              {result.total > pageSize && (
                <PageNavigation
                  page={page}
                  pageCount={pageCount}
                  onPageChange={changePage}
                  label={t("auditLog.pages")}
                  previousLabel={t("auditLog.previous")}
                  nextLabel={t("auditLog.next")}
                  pending={auditQuery.isFetching}
                />
              )}
            </section>
          );
        }}
      </QueryState>
    </div>
  );
}
