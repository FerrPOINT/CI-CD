import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useParams } from "react-router";
import { Copy, Download, Package } from "lucide-react";
import {
  PageHeader,
  CopyButton,
  toast,
  Select,
  Loader,
  Button,
  ResourceList,
  ResourceListRow,
  SearchInput,
  EmptyState,
  Notice,
} from "@pdlc/ui/ui";
import { downloadArtifact, saveDownloadedArtifact } from "@/api/client";
import { useArtifacts } from "@/api/hooks";
import type { Artifact } from "@/api/types";

import { QueryState } from "@/shared/ui/query-state";

const pageSize = 20;
type ArtifactStatus = "available" | "expired" | "purged";
type StatusFilter = "all" | ArtifactStatus;

export function ArtifactsPage() {
  const { t } = useTranslation();
  const { jobId } = useParams();
  const {
    data: artifacts = [],
    isLoading,
    error: listError,
    refetch,
  } = useArtifacts(jobId);
  const [search, setSearch] = useState("");
  const [status, setStatus] = useState<StatusFilter>("all");
  const [page, setPage] = useState(1);
  const [downloadingId, setDownloadingId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<{
    id: string;
    message: string;
  } | null>(null);

  const normalizedSearch = search.trim().toLocaleLowerCase();
  const filtered = artifacts.filter(
    (artifact) =>
      (status === "all" || artifactState(artifact) === status) &&
      (artifact.name.toLocaleLowerCase().includes(normalizedSearch) ||
        artifact.content_type.toLocaleLowerCase().includes(normalizedSearch)),
  );
  const totalPages = Math.max(1, Math.ceil(filtered.length / pageSize));
  const currentPage = Math.min(page, totalPages);
  const visible = filtered.slice(
    (currentPage - 1) * pageSize,
    currentPage * pageSize,
  );

  async function handleDownload(artifact: Artifact) {
    if (downloadingId) return;
    setActionError(null);
    setDownloadingId(artifact.id);
    try {
      saveDownloadedArtifact(await downloadArtifact(artifact.id));
    } catch {
      setActionError({
        id: artifact.id,
        message: t("artifacts.downloadFailed"),
      });
      void refetch();
    } finally {
      setDownloadingId(null);
    }
  }

  return (
    <div className="min-w-0 space-y-5">
      <PageHeader
        title={t("artifacts.title")}
        leading={
          <>
            <Package className="h-5 w-5 text-accent" aria-hidden />
          </>
        }
      />

      <QueryState
        data={artifacts}
        isLoading={isLoading}
        error={listError}
        errorMessage={t("artifacts.loadError")}
        isEmpty={(list) => list.length === 0}
        empty={{ title: t("artifacts.empty") }}
        onRetry={() => void refetch()}
      >
        {() => (
          <section aria-label={t("artifacts.title")} className="space-y-3">
            <div className="flex flex-wrap gap-2">
              <div className="min-w-0 flex-1 sm:max-w-xl">
                <SearchInput
                  aria-label={t("artifacts.search")}
                  placeholder={t("artifacts.search")}
                  value={search}
                  onChange={(event) => {
                    setSearch(event.target.value);
                    setPage(1);
                  }}
                />
              </div>
              <Select
                aria-label={t("artifacts.status")}
                className="w-auto"
                value={status}
                onValueChange={(nextValue) => {
                  setStatus(nextValue as StatusFilter);
                  setPage(1);
                }}
                options={[
                  { value: "all", label: String(t("artifacts.all")) },
                  {
                    value: "available",
                    label: String(t("artifacts.available")),
                  },
                  { value: "expired", label: String(t("artifacts.expired")) },
                  { value: "purged", label: String(t("artifacts.purged")) },
                ]}
              />
            </div>
            <p className="text-xs text-text-muted">
              {t("artifacts.shown", {
                count: visible.length,
                total: filtered.length,
              })}
            </p>
            {filtered.length === 0 ? (
              <div role="status">
                <EmptyState message={t("artifacts.noMatches")} />
              </div>
            ) : (
              <ResourceList asChild>
                <ul>
                  {visible.map((artifact) => {
                    const state = artifactState(artifact);
                    return (
                      <ResourceListRow asChild key={artifact.id}>
                        <li className="min-w-0">
                          <div className="flex min-w-0 items-start gap-2">
                            <Package
                              className="mt-1 h-4 w-4 shrink-0 text-accent"
                              aria-hidden
                            />
                            <div className="min-w-0 flex-1">
                              <p className="break-all text-sm font-medium">
                                {artifact.name}
                              </p>
                              <p className="break-all text-xs text-text-secondary">
                                {formatBytes(artifact.size_bytes)} /{" "}
                                {artifact.content_type}
                              </p>
                              {state !== "available" && (
                                <p className="text-xs text-text-muted">
                                  {t("artifacts." + state)}
                                </p>
                              )}
                            </div>
                            {artifact.sha256 && (
                              <CopyButton
                                variant="ghost"
                                aria-label={t("artifacts.copyDigestFor", {
                                  name: artifact.name,
                                })}
                                title={t("artifacts.copyDigestFor", {
                                  name: artifact.name,
                                })}
                                value={artifact.sha256}
                                size="icon"
                                onClick={() => setActionError(null)}
                                onCopied={() =>
                                  toast.success(t("artifacts.copied"))
                                }
                                onCopyError={() =>
                                  setActionError({
                                    id: artifact.id,
                                    message: t("artifacts.copyFailed"),
                                  })
                                }
                              >
                                <Copy className="h-4 w-4" aria-hidden />
                              </CopyButton>
                            )}
                            {state === "available" && (
                              <Button
                                variant="ghost"
                                type="button"
                                aria-label={
                                  downloadingId === artifact.id
                                    ? t("artifacts.downloading", {
                                        name: artifact.name,
                                      })
                                    : t("artifacts.downloadFor", {
                                        name: artifact.name,
                                      })
                                }
                                title={t("artifacts.downloadFor", {
                                  name: artifact.name,
                                })}
                                aria-busy={downloadingId === artifact.id}
                                disabled={downloadingId !== null}
                                size="icon" className="shrink-0"
                                onClick={() => void handleDownload(artifact)}
                              >
                                {downloadingId === artifact.id ? (
                                  <Loader
                                    size="sm"
                                    className="h-4 w-4"
                                    aria-hidden
                                  />
                                ) : (
                                  <Download className="h-4 w-4" aria-hidden />
                                )}
                              </Button>
                            )}
                          </div>
                          <div className="ml-6 flex flex-wrap gap-x-4 gap-y-1 text-xs text-text-muted">
                            <time
                              dateTime={artifact.created_at}
                              title={new Date(
                                artifact.created_at,
                              ).toLocaleString()}
                            >
                              {t("artifacts.created")}:{" "}
                              {formatDate(artifact.created_at)}
                            </time>
                            <time
                              dateTime={artifact.expires_at}
                              title={new Date(
                                artifact.expires_at,
                              ).toLocaleString()}
                            >
                              {t("artifacts.expires")}:{" "}
                              {formatDate(artifact.expires_at)}
                            </time>
                          </div>
                          {artifact.sha256 && (
                            <p className="ml-6 min-w-0 text-xs text-text-muted">
                              SHA-256:{" "}
                              <code
                                title={artifact.sha256}
                                className="font-mono"
                              >
                                {artifact.sha256.slice(0, 12)}...
                              </code>
                            </p>
                          )}
                          {actionError?.id === artifact.id && (
                            <Notice
                              role="alert"
                              className="ml-6 break-words"
                              tone="danger"
                              appearance="inline"
                            >
                              {actionError.message}
                            </Notice>
                          )}
                        </li>
                      </ResourceListRow>
                    );
                  })}
                </ul>
              </ResourceList>
            )}
            {filtered.length > pageSize && (
              <nav
                aria-label={t("artifacts.pages")}
                className="flex items-center justify-end gap-2"
              >
                <Button
                  type="button"
                  size="sm"
                  variant="outline"

                  disabled={currentPage === 1}
                  onClick={() => setPage(currentPage - 1)}
                >
                  {t("artifacts.previous")}
                </Button>
                <span className="text-sm text-text-muted">
                  {currentPage} / {totalPages}
                </span>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"

                  disabled={currentPage === totalPages}
                  onClick={() => setPage(currentPage + 1)}
                >
                  {t("artifacts.next")}
                </Button>
              </nav>
            )}
          </section>
        )}
      </QueryState>
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return bytes + " B";
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + " KiB";
  return (bytes / (1024 * 1024)).toFixed(1) + " MiB";
}

function formatDate(value: string): string {
  return new Date(value).toLocaleString(undefined, {
    dateStyle: "short",
    timeStyle: "short",
  });
}

function artifactState(artifact: Artifact): ArtifactStatus {
  if (artifact.purged_at) return "purged";
  return new Date(artifact.expires_at).getTime() <= Date.now()
    ? "expired"
    : "available";
}
