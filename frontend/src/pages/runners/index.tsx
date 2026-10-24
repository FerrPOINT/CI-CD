import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Server, Trash2 } from "lucide-react";
import {
  PageNavigation,
  Badge,
  PageHeader,
  toast,
  Select,
  Button,
  Input,
  Label,
  ResourceList,
  ResourceListRow,
  ConfirmDialog,
  SearchInput,
  EmptyState,
  FormField,
} from "@pdlc/ui/ui";
import { useDeleteRunner, useRegisterRunner, useRunners } from "@/api/hooks";
import type { Runner, RunnerStatus } from "@/api/types";

import { QueryState } from "@/shared/ui/query-state";

const pageSize = 20;

function runnerTone(status: RunnerStatus) {
  if (status === "online") return "success";
  if (status === "paused") return "warning";
  return "muted";
}

export function RunnersPage() {
  const { t } = useTranslation();
  const {
    data: runners = [],
    isLoading,
    error: listError,
    refetch,
  } = useRunners();
  const registerRunner = useRegisterRunner();
  const deleteRunner = useDeleteRunner();
  const [showForm, setShowForm] = useState(false);
  const [form, setForm] = useState({ name: "", tags: "" });
  const [pendingDelete, setPendingDelete] = useState<Runner | null>(null);
  const [search, setSearch] = useState("");
  const [statusFilter, setStatusFilter] = useState<RunnerStatus | "all">("all");
  const [page, setPage] = useState(1);

  const normalizedSearch = search.trim().toLocaleLowerCase();
  const filteredRunners = runners
    .filter(
      (runner) =>
        (statusFilter === "all" || runner.status === statusFilter) &&
        `${runner.name} ${runner.tags.join(" ")}`
          .toLocaleLowerCase()
          .includes(normalizedSearch),
    )
    .sort((a, b) => a.name.localeCompare(b.name));
  const totalPages = Math.max(1, Math.ceil(filteredRunners.length / pageSize));
  const currentPage = Math.min(page, totalPages);
  const visibleRunners = filteredRunners.slice(
    (currentPage - 1) * pageSize,
    currentPage * pageSize,
  );

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const name = form.name.trim();
    if (!name) return;
    registerRunner.mutate(
      {
        name,
        tags: form.tags
          .split(",")
          .map((tag) => tag.trim())
          .filter(Boolean),
      },
      {
        onSuccess: () => {
          setShowForm(false);
          setForm({ name: "", tags: "" });
          toast.success(t("runners.registered"));
        },
        onError: () => toast.error(t("runners.registerError")),
      },
    );
  }

  return (
    <div className="space-y-5">
      <PageHeader
        title={t("runners.title")}
        actions={
          <>
            <Button
              size="sm"

              aria-expanded={showForm}
              disabled={registerRunner.isPending}
              onClick={() => {
                if (showForm) setForm({ name: "", tags: "" });
                setShowForm(!showForm);
              }}
            >
              <Plus className="h-4 w-4" aria-hidden />
              {t("runners.register")}
            </Button>
          </>
        }
      />

      {showForm && (
        <form
          onSubmit={handleSubmit}
          aria-label={t("runners.register")}
          className="grid gap-3 border-y border-border py-4 sm:grid-cols-2"
        >
          <FormField id="runner-name" label={t("runners.name")}>
            {(baseFieldAttrs) => (
              <Input
                {...baseFieldAttrs}
                required
                autoFocus
                value={form.name}
                onChange={(event) =>
                  setForm({ ...form, name: event.target.value })
                }
              />
            )}
          </FormField>
          <FormField id="runner-tags" label={t("runners.tags")}>
            {(baseFieldAttrs) => (
              <Input
                {...baseFieldAttrs}
                placeholder="linux, docker"
                value={form.tags}
                onChange={(event) =>
                  setForm({ ...form, tags: event.target.value })
                }
              />
            )}
          </FormField>
          <p className="text-xs text-text-secondary sm:col-span-2">
            {t("runners.manualNotice")}
          </p>
          <div className="flex justify-end gap-2 sm:col-span-2">
            <Button
              type="button"
              variant="outline"

              disabled={registerRunner.isPending}
              onClick={() => {
                setShowForm(false);
                setForm({ name: "", tags: "" });
              }}
            >
              {t("common.cancel")}
            </Button>
            <Button
              type="submit"

              disabled={registerRunner.isPending || !form.name.trim()}
            >
              {t("runners.register")}
            </Button>
          </div>
        </form>
      )}

      <QueryState
        data={runners}
        isLoading={isLoading}
        error={listError}
        errorMessage={t("runners.loadError")}
        isEmpty={(list) => list.length === 0}
        empty={{ title: t("runners.empty") }}
        onRetry={() => void refetch()}
      >
        {() => (
          <section aria-label={t("runners.title")} className="space-y-3">
            <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-end">
              <div>
                <SearchInput
                  aria-label={t("runners.search")}
                  placeholder={t("runners.search")}
                  value={search}
                  onChange={(event) => {
                    setSearch(event.target.value);
                    setPage(1);
                  }}
                />
              </div>
              <div className="flex min-w-0 items-center gap-2">
                <Label htmlFor="runner-status" className="shrink-0">
                  {t("runners.status")}
                </Label>
                <Select
                  id="runner-status"
                  value={statusFilter}
                  onValueChange={(nextValue) => {
                    setStatusFilter(nextValue as RunnerStatus | "all");
                    setPage(1);
                  }}
                  className="w-auto min-w-0 flex-1 sm:w-40"
                  options={[
                    { value: "all", label: String(t("runners.allStatuses")) },
                    {
                      value: "online",
                      label: String(t("runners.status_online")),
                    },
                    {
                      value: "offline",
                      label: String(t("runners.status_offline")),
                    },
                    {
                      value: "paused",
                      label: String(t("runners.status_paused")),
                    },
                  ]}
                />
              </div>
            </div>
            <p className="text-xs text-text-muted">
              {t("runners.shown", {
                count: visibleRunners.length,
                total: filteredRunners.length,
              })}
            </p>
            {filteredRunners.length === 0 ? (
              <div role="status">
                <EmptyState message={t("runners.noMatches")} />
              </div>
            ) : (
              <ResourceList asChild>
                <ul>
                  {visibleRunners.map((runner) => {
                    const tags = runner.tags.join(", ");
                    return (
                      <ResourceListRow asChild key={runner.id}>
                        <li className="flex min-w-0 items-center gap-2">
                          <Server
                            className="h-4 w-4 shrink-0 text-accent"
                            aria-hidden
                          />
                          <div className="min-w-0 flex-1">
                            <p
                              className="truncate text-sm font-medium"
                              title={runner.name}
                            >
                              {runner.name}
                            </p>
                            <p
                              className="truncate text-xs text-text-muted"
                              title={tags || undefined}
                            >
                              {tags || t("runners.noTags")}
                            </p>
                            <p className="truncate text-xs text-text-muted">
                              {t("runners.lastSeen")}:{" "}
                              {runner.last_seen_at
                                ? new Date(runner.last_seen_at).toLocaleString(
                                    undefined,
                                    { dateStyle: "short", timeStyle: "short" },
                                  )
                                : t("runners.neverSeen")}
                            </p>
                          </div>
                          <Badge
                            appearance="dot"
                            tone={runnerTone(runner.status)}
                            className="shrink-0"
                          >
                            {t(`runners.status_${runner.status}`)}
                          </Badge>
                          <Button
                            size="icon"
                            variant="destructive-outline"
                            type="button"
                            aria-label={t("runners.deleteRunner", {
                              name: runner.name,
                            })}
                            title={t("runners.deleteRunner", {
                              name: runner.name,
                            })}
                            disabled={deleteRunner.isPending}
                            className="shrink-0"
                            onClick={() => setPendingDelete(runner)}
                          >
                            <Trash2 className="h-4 w-4" aria-hidden />
                          </Button>
                        </li>
                      </ResourceListRow>
                    );
                  })}
                </ul>
              </ResourceList>
            )}
            {filteredRunners.length > pageSize && (
              <PageNavigation
                page={currentPage}
                pageCount={totalPages}
                onPageChange={setPage}
                label={t("runners.pages")}
                previousLabel={t("runners.previous")}
                nextLabel={t("runners.next")}
              />
            )}
          </section>
        )}
      </QueryState>

      <ConfirmDialog
        open={pendingDelete !== null}
        title={
          pendingDelete
            ? `${t("runners.deleteConfirm")} "${pendingDelete.name}"?`
            : ""
        }
        description={t("runners.deleteWarning")}
        isPending={deleteRunner.isPending}
        closeOnConfirm={false}
        onOpenChange={(open) => {
          if (open) return;
          setPendingDelete(null);
        }}
        onConfirm={() => {
          if (!pendingDelete) return;
          deleteRunner.mutate(pendingDelete.id, {
            onSuccess: () => {
              setPendingDelete(null);
              toast.success(t("runners.deleted"));
            },
            onError: () => toast.error(t("runners.deleteError")),
          });
        }}
        confirmLabel={t("common.delete")}
        pendingLabel={t("common.delete")}
      />
    </div>
  );
}
