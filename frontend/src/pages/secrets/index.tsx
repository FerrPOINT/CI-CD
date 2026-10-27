import { useEffect, useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { useParams } from "react-router";
import {
  Eye,
  EyeOff,
  KeyRound,
  Plus,
  RotateCw,
  ShieldCheck,
  Trash2,
} from "lucide-react";
import {
  ResourceListTitle,
  ActionGroup,
  PageNavigation,
  PageHeader,
  toast,
  Button,
  Input,
  InputWithAction,
  Label,
  ResourceList,
  ResourceListRow,
  ConfirmDialog,
  SearchInput,
  Notice,
  EmptyState,
} from "@pdlc/ui/ui";
import { useDeleteSecret, useSecrets, useUpsertSecret } from "@/api/hooks";
import type { SecretMetadata } from "@/api/types";

import { QueryState } from "@/shared/ui/query-state";

const pageSize = 20;
const emptyForm = { key: "", value: "" };

export function SecretsPage() {
  const { t } = useTranslation();
  const { projectId } = useParams();
  const {
    data: secrets = [],
    isLoading,
    error: listError,
    refetch,
  } = useSecrets(projectId);
  const upsert = useUpsertSecret(projectId);
  const remove = useDeleteSecret();
  const [showForm, setShowForm] = useState(false);
  const [editingKey, setEditingKey] = useState<string | null>(null);
  const [form, setForm] = useState(emptyForm);
  const [showValue, setShowValue] = useState(false);
  const [saveError, setSaveError] = useState("");
  const [pendingDelete, setPendingDelete] = useState<SecretMetadata | null>(
    null,
  );
  const [deleteError, setDeleteError] = useState("");
  const [search, setSearch] = useState("");
  const [page, setPage] = useState(1);
  const keyRef = useRef<HTMLInputElement>(null);
  const valueRef = useRef<HTMLInputElement>(null);

  const normalizedSearch = search.trim().toLocaleLowerCase();
  const filteredSecrets = secrets.filter((secret) =>
    secret.key.toLocaleLowerCase().includes(normalizedSearch),
  );
  const totalPages = Math.max(1, Math.ceil(filteredSecrets.length / pageSize));
  const currentPage = Math.min(page, totalPages);
  const visibleSecrets = filteredSecrets.slice(
    (currentPage - 1) * pageSize,
    currentPage * pageSize,
  );
  const isReplacing =
    editingKey !== null ||
    secrets.some((secret) => secret.key === form.key.trim());

  useEffect(() => {
    if (showForm) (editingKey === null ? keyRef : valueRef).current?.focus();
  }, [showForm, editingKey]);

  function closeForm() {
    setShowForm(false);
    setEditingKey(null);
    setForm(emptyForm);
    setShowValue(false);
    setSaveError("");
  }

  function openNewForm() {
    if (showForm && editingKey === null) {
      closeForm();
      return;
    }
    setForm(emptyForm);
    setEditingKey(null);
    setShowValue(false);
    setSaveError("");
    setShowForm(true);
  }

  function openReplaceForm(secret: SecretMetadata) {
    setForm({ key: secret.key, value: "" });
    setEditingKey(secret.key);
    setShowValue(false);
    setSaveError("");
    setShowForm(true);
  }

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const key = form.key.trim();
    if (!key || !form.value || upsert.isPending) return;
    setSaveError("");
    upsert.mutate(
      { key, value: form.value },
      {
        onSuccess: () => {
          closeForm();
          setSearch(key);
          setPage(1);
          toast.success(t(isReplacing ? "secrets.replaced" : "secrets.saved"));
        },
        onError: () => setSaveError(t("secrets.saveError")),
      },
    );
  }

  return (
    <div className="min-w-0 space-y-5">
      <PageHeader
        title={t("secrets.title")}
        actions={
          <>
            <Button
              type="button"
              size="sm"

              aria-expanded={showForm && editingKey === null}
              disabled={upsert.isPending}
              onClick={openNewForm}
            >
              <Plus className="h-4 w-4" aria-hidden />
              {t("secrets.add")}
            </Button>
          </>
        }
      />

      <Notice appearance="inline" separated icon={<ShieldCheck aria-hidden />}>
        <p>{t("secrets.encryptedNotice")}</p>
      </Notice>

      {showForm && (
        <form
          onSubmit={handleSubmit}
          aria-label={t(
            editingKey !== null ? "secrets.replace" : "secrets.add",
          )}
          aria-busy={upsert.isPending}
          className="grid gap-3 border-b border-border pb-4 sm:grid-cols-2"
        >
          <div className="min-w-0 space-y-1.5">
            {editingKey !== null ? (
              <>
                <span className="text-sm font-medium">{t("secrets.key")}</span>
                <p className="flex min-h-10 min-w-0 items-center break-all border-b border-border py-2 font-mono text-sm">
                  {form.key}
                </p>
              </>
            ) : (
              <>
                <Label htmlFor="secret-key">{t("secrets.key")}</Label>
                <Input
                  id="secret-key"
                  ref={keyRef}
                  className="min-w-0"
                  required
                  disabled={upsert.isPending}
                  autoComplete="off"
                  placeholder="DATABASE_PASSWORD"
                  value={form.key}
                  onChange={(event) => {
                    setForm({ ...form, key: event.target.value });
                    setSaveError("");
                  }}
                />
              </>
            )}
          </div>
          <div className="min-w-0 space-y-1.5">
            <Label htmlFor="secret-value">{t("secrets.value")}</Label>
            <InputWithAction
              id="secret-value"
              ref={valueRef}
              type={showValue ? "text" : "password"}
              className="min-w-0"
              required
              disabled={upsert.isPending}
              autoComplete="new-password"
              value={form.value}
              onChange={(event) => {
                setForm({ ...form, value: event.target.value });
                setSaveError("");
              }}
              action={{
                label: t(showValue ? "secrets.hideValue" : "secrets.showValue"),
                pressed: showValue,
                onAction: () => setShowValue(!showValue),
                icon: showValue ? (
                  <EyeOff className="h-4 w-4" aria-hidden />
                ) : (
                  <Eye className="h-4 w-4" aria-hidden />
                ),
              }}
            />
          </div>
          {isReplacing && (
            <p className="text-xs text-text-secondary sm:col-span-2">
              {t("secrets.replaceWarning")}
            </p>
          )}
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
          <ActionGroup align="end" className="sm:col-span-2">
            <Button
              type="button"
              variant="outline"

              disabled={upsert.isPending}
              onClick={closeForm}
            >
              {t("common.cancel")}
            </Button>
            <Button
              type="submit"

              disabled={upsert.isPending || !form.key.trim() || !form.value}
            >
              {t(isReplacing ? "secrets.replace" : "secrets.save")}
            </Button>
          </ActionGroup>
        </form>
      )}

      <QueryState
        data={secrets}
        isLoading={isLoading}
        error={listError}
        errorMessage={t("secrets.loadError")}
        isEmpty={(list) => list.length === 0}
        empty={{ title: t("secrets.empty") }}
        onRetry={() => void refetch()}
      >
        {() => (
          <section aria-label={t("secrets.title")} className="space-y-3">
            <div className="max-w-xl">
              <SearchInput
                aria-label={t("secrets.search")}
                placeholder={t("secrets.search")}
                value={search}
                onChange={(event) => {
                  setSearch(event.target.value);
                  setPage(1);
                }}
              />
            </div>
            <p className="text-xs text-text-muted">
              {t("secrets.shown", {
                count: visibleSecrets.length,
                total: filteredSecrets.length,
              })}
            </p>
            {filteredSecrets.length === 0 ? (
              <div role="status">
                <EmptyState message={t("secrets.noMatches")} />
              </div>
            ) : (
              <ResourceList asChild>
                <ul>
                  {visibleSecrets.map((secret) => (
                    <ResourceListRow asChild key={secret.id}>
                      <li className="flex min-w-0 items-center gap-2">
                        <KeyRound
                          className="h-4 w-4 shrink-0 text-accent"
                          aria-hidden
                        />
                        <div className="min-w-0 flex-1">
                          <ResourceListTitle asChild technical>
                            <p className="break-all">{secret.key}</p>
                          </ResourceListTitle>
                          <time
                            dateTime={secret.updated_at}
                            title={new Date(secret.updated_at).toLocaleString()}
                            className="text-xs text-text-muted"
                          >
                            <span className="sr-only sm:not-sr-only">
                              {t("secrets.updated")}:{" "}
                            </span>
                            {new Date(secret.updated_at).toLocaleString(
                              undefined,
                              { dateStyle: "short", timeStyle: "short" },
                            )}
                          </time>
                        </div>
                        <Button
                          variant="ghost"
                          type="button"
                          aria-label={t("secrets.replaceFor", {
                            name: secret.key,
                          })}
                          title={t("secrets.replaceFor", { name: secret.key })}
                          disabled={upsert.isPending}
                          size="icon"
                          className="shrink-0"
                          onClick={() => openReplaceForm(secret)}
                        >
                          <RotateCw className="h-4 w-4" aria-hidden />
                        </Button>
                        <Button
                          size="icon"
                          variant="destructive-outline"
                          type="button"
                          aria-label={t("secrets.deleteFor", {
                            name: secret.key,
                          })}
                          title={t("secrets.deleteFor", { name: secret.key })}
                          disabled={remove.isPending}
                          className="shrink-0"
                          onClick={() => {
                            setPendingDelete(secret);
                            setDeleteError("");
                          }}
                        >
                          <Trash2 className="h-4 w-4" aria-hidden />
                        </Button>
                      </li>
                    </ResourceListRow>
                  ))}
                </ul>
              </ResourceList>
            )}
            {filteredSecrets.length > pageSize && (
              <PageNavigation
                page={currentPage}
                pageCount={totalPages}
                onPageChange={setPage}
                label={t("secrets.pages")}
                previousLabel={t("secrets.previous")}
                nextLabel={t("secrets.next")}
              />
            )}
          </section>
        )}
      </QueryState>

      <ConfirmDialog
        open={pendingDelete !== null}
        title={
          pendingDelete
            ? `${t("secrets.deleteConfirm")} "${pendingDelete.key}"?`
            : ""
        }
        description={t("secrets.deleteWarning")}
        error={deleteError || undefined}
        isPending={remove.isPending}
        closeOnConfirm={false}
        onOpenChange={(open) => {
          if (open) return;
          setPendingDelete(null);
          setDeleteError("");
        }}
        onConfirm={() => {
          if (!pendingDelete || remove.isPending) return;
          setDeleteError("");
          remove.mutate(pendingDelete.id, {
            onSuccess: () => {
              setPendingDelete(null);
              toast.success(t("secrets.deleted"));
            },
            onError: () => setDeleteError(t("secrets.deleteError")),
          });
        }}
        confirmLabel={t("common.delete")}
        pendingLabel={t("common.delete")}
      />
    </div>
  );
}
