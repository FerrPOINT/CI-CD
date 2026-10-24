import {
  PageNavigation,
  PageHeader,
  Disclosure,
  toast,
  Button,
  Input,
  Label,
  ResourceList,
  ResourceListRow,
  Checkbox,
  ConfirmDialog,
  SearchInput,
  Notice,
  EmptyState,
} from "@pdlc/ui/ui";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { useParams } from "react-router";
import { Clock, Pencil, Plus, Trash2 } from "lucide-react";

import {
  useCreateSchedule,
  useDeleteSchedule,
  useSchedules,
  useUpdateSchedule,
} from "@/api/hooks";
import type { Schedule } from "@/api/types";

import { QueryState } from "@/shared/ui/query-state";

const pageSize = 20;
const emptyForm = { cron: "", git_ref: "main", enabled: true };

export function SchedulesPage() {
  const { t } = useTranslation();
  const { projectId } = useParams();
  const {
    data: schedules = [],
    isLoading,
    error: listError,
    refetch,
  } = useSchedules(projectId);
  const createSchedule = useCreateSchedule(projectId);
  const updateSchedule = useUpdateSchedule();
  const deleteSchedule = useDeleteSchedule();
  const [showForm, setShowForm] = useState(false);
  const [editing, setEditing] = useState<Schedule | null>(null);
  const [form, setForm] = useState(emptyForm);
  const [saveError, setSaveError] = useState("");
  const [pendingDelete, setPendingDelete] = useState<Schedule | null>(null);
  const [deleteError, setDeleteError] = useState("");
  const [search, setSearch] = useState("");
  const [page, setPage] = useState(1);
  const cronRef = useRef<HTMLInputElement>(null);
  const saving = createSchedule.isPending || updateSchedule.isPending;

  const normalizedSearch = search.trim().toLocaleLowerCase();
  const filteredSchedules = schedules.filter((schedule) =>
    (schedule.cron + " " + schedule.git_ref)
      .toLocaleLowerCase()
      .includes(normalizedSearch),
  );
  const totalPages = Math.max(
    1,
    Math.ceil(filteredSchedules.length / pageSize),
  );
  const currentPage = Math.min(page, totalPages);
  const visibleSchedules = filteredSchedules.slice(
    (currentPage - 1) * pageSize,
    currentPage * pageSize,
  );

  useEffect(() => {
    if (showForm) cronRef.current?.focus();
  }, [showForm, editing]);

  function closeForm() {
    setShowForm(false);
    setEditing(null);
    setForm(emptyForm);
    setSaveError("");
  }

  function openCreateForm() {
    if (showForm && editing === null) {
      closeForm();
      return;
    }
    setEditing(null);
    setForm(emptyForm);
    setSaveError("");
    setShowForm(true);
  }

  function openEditForm(schedule: Schedule) {
    setEditing(schedule);
    setForm({
      cron: schedule.cron,
      git_ref: schedule.git_ref,
      enabled: schedule.enabled,
    });
    setSaveError("");
    setShowForm(true);
  }

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (saving) return;
    const input = {
      cron: form.cron.trim(),
      git_ref: form.git_ref.trim(),
      enabled: form.enabled,
    };
    if (!input.cron || !input.git_ref) return;
    setSaveError("");
    const callbacks = {
      onSuccess: () => {
        closeForm();
        toast.success(t(editing ? "schedules.updated" : "schedules.created"));
      },
      onError: () => setSaveError(t("schedules.saveError")),
    };
    if (editing) updateSchedule.mutate({ id: editing.id, ...input }, callbacks);
    else createSchedule.mutate(input, callbacks);
  }

  return (
    <div className="min-w-0 space-y-5">
      <PageHeader
        title={t("schedules.title")}
        actions={
          <>
            <Button
              type="button"
              size="sm"

              aria-expanded={showForm && editing === null}
              disabled={saving}
              onClick={openCreateForm}
            >
              <Plus className="h-4 w-4" aria-hidden />
              {t("schedules.create")}
            </Button>
          </>
        }
      />

      <p className="border-y border-border py-3 text-xs text-text-secondary">
        {t("schedules.utcNotice")}
      </p>

      {showForm && (
        <form
          onSubmit={handleSubmit}
          aria-label={t(editing ? "schedules.edit" : "schedules.create")}
          aria-busy={saving}
          className="grid gap-3 border-b border-border pb-4 sm:grid-cols-2"
        >
          <div className="min-w-0 space-y-1.5">
            <Label htmlFor="schedule-cron">{t("schedules.cron")}</Label>
            <Input
              id="schedule-cron"
              ref={cronRef}
              className="min-w-0 font-mono"
              required
              disabled={saving}
              placeholder="0 4 * * 1"
              value={form.cron}
              onChange={(event) => {
                setForm({ ...form, cron: event.target.value });
                setSaveError("");
              }}
            />
          </div>
          <div className="min-w-0 space-y-1.5">
            <Label htmlFor="schedule-ref">{t("schedules.gitRef")}</Label>
            <Input
              id="schedule-ref"
              className="min-w-0"
              required
              disabled={saving}
              value={form.git_ref}
              onChange={(event) => {
                setForm({ ...form, git_ref: event.target.value });
                setSaveError("");
              }}
            />
          </div>
          <Checkbox
            className="sm:col-span-2"
            checked={form.enabled}
            disabled={saving}
            onCheckedChange={(checked) => {
              setForm({ ...form, enabled: checked === true });
              setSaveError("");
            }}
          >
            {t("schedules.enabled")}
          </Checkbox>
          {saveError && (
            <Notice
              role="alert"
              className="sm:col-span-2"
              tone="danger"
              appearance="inline"
            >
              {saveError}
            </Notice>
          )}
          <div className="flex flex-wrap justify-end gap-2 sm:col-span-2">
            <Button
              type="button"
              variant="outline"

              disabled={saving}
              onClick={closeForm}
            >
              {t("common.cancel")}
            </Button>
            <Button
              type="submit"

              disabled={saving || !form.cron.trim() || !form.git_ref.trim()}
            >
              {t(editing ? "schedules.save" : "schedules.create")}
            </Button>
          </div>
        </form>
      )}

      <QueryState
        data={schedules}
        isLoading={isLoading}
        error={listError}
        errorMessage={t("schedules.loadError")}
        isEmpty={(list) => list.length === 0}
        empty={{ title: t("schedules.empty") }}
        onRetry={() => void refetch()}
      >
        {() => (
          <section aria-label={t("schedules.title")} className="space-y-3">
            <div className="max-w-xl">
              <SearchInput
                aria-label={t("schedules.search")}
                placeholder={t("schedules.search")}
                value={search}
                onChange={(event) => {
                  setSearch(event.target.value);
                  setPage(1);
                }}
              />
            </div>
            <p className="text-xs text-text-muted">
              {t("schedules.shown", {
                count: visibleSchedules.length,
                total: filteredSchedules.length,
              })}
            </p>
            {filteredSchedules.length === 0 ? (
              <div role="status">
                <EmptyState message={t("schedules.noMatches")} />
              </div>
            ) : (
              <ResourceList asChild>
                <ul>
                  {visibleSchedules.map((schedule) => (
                    <ResourceListRow asChild key={schedule.id}>
                      <li className="min-w-0">
                        <div className="flex min-w-0 items-center gap-2">
                          <Clock
                            className="h-4 w-4 shrink-0 text-accent"
                            aria-hidden
                          />
                          <div className="min-w-0 flex-1">
                            <p className="break-all font-mono text-sm font-medium">
                              {schedule.cron}
                            </p>
                            <p className="break-all text-xs text-text-secondary">
                              {schedule.git_ref}
                            </p>
                          </div>
                          <span
                            className={
                              schedule.enabled
                                ? "shrink-0 text-xs text-success"
                                : "shrink-0 text-xs text-text-muted"
                            }
                          >
                            {t(
                              schedule.enabled
                                ? "schedules.enabledOn"
                                : "schedules.enabledOff",
                            )}
                          </span>
                          <Button
                            variant="ghost"
                            type="button"
                            aria-label={t("schedules.editFor", {
                              name: schedule.cron,
                            })}
                            title={t("schedules.editFor", {
                              name: schedule.cron,
                            })}
                            disabled={saving}
                            size="icon"
                            className="shrink-0"
                            onClick={() => openEditForm(schedule)}
                          >
                            <Pencil className="h-4 w-4" aria-hidden />
                          </Button>
                          <Button
                            size="icon"
                            variant="destructive-outline"
                            type="button"
                            aria-label={t("schedules.deleteFor", {
                              name: schedule.cron,
                            })}
                            title={t("schedules.deleteFor", {
                              name: schedule.cron,
                            })}
                            disabled={deleteSchedule.isPending}
                            className="shrink-0"
                            onClick={() => {
                              setPendingDelete(schedule);
                              setDeleteError("");
                            }}
                          >
                            <Trash2 className="h-4 w-4" aria-hidden />
                          </Button>
                        </div>
                        <div className="ml-6 flex flex-wrap gap-x-4 gap-y-1 text-xs text-text-muted">
                          <span>
                            {t("schedules.nextFire")}:{" "}
                            {formatOptionalDate(schedule.next_fire_at)}
                          </span>
                          <span>
                            {t("schedules.lastFire")}:{" "}
                            {formatOptionalDate(schedule.last_fired_at)}
                          </span>
                        </div>
                        {schedule.last_fire_error && (
                          <Disclosure
                            className="ml-6 min-w-0 text-xs text-danger-strong"
                            summary={<>{t("schedules.errorPaused")}</>}
                          >
                            <p className="break-all pb-1">
                              {schedule.last_fire_error}
                            </p>
                          </Disclosure>
                        )}
                      </li>
                    </ResourceListRow>
                  ))}
                </ul>
              </ResourceList>
            )}
            {filteredSchedules.length > pageSize && (
              <PageNavigation
                page={currentPage}
                pageCount={totalPages}
                onPageChange={setPage}
                label={t("schedules.pages")}
                previousLabel={t("schedules.previous")}
                nextLabel={t("schedules.next")}
              />
            )}
          </section>
        )}
      </QueryState>

      <ConfirmDialog
        open={pendingDelete !== null}
        title={
          pendingDelete
            ? t("schedules.deleteConfirm") + ' "' + pendingDelete.cron + '"?'
            : ""
        }
        description={t("schedules.deleteWarning")}
        error={deleteError || undefined}
        isPending={deleteSchedule.isPending}
        closeOnConfirm={false}
        onOpenChange={(open) => {
          if (open) return;
          setPendingDelete(null);
          setDeleteError("");
        }}
        onConfirm={() => {
          if (!pendingDelete || deleteSchedule.isPending) return;
          setDeleteError("");
          deleteSchedule.mutate(pendingDelete.id, {
            onSuccess: () => {
              setPendingDelete(null);
              toast.success(t("schedules.deleted"));
            },
            onError: () => setDeleteError(t("schedules.deleteError")),
          });
        }}
        confirmLabel={t("common.delete")}
        pendingLabel={t("common.delete")}
      />
    </div>
  );
}

function formatOptionalDate(value: string | null): string {
  return value
    ? new Date(value).toLocaleString(undefined, {
        dateStyle: "short",
        timeStyle: "short",
        timeZone: "UTC",
      }) + " UTC"
    : "-";
}
