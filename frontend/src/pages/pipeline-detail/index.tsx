import { ResourceListTitle, ResourceListDescription,
  Notice,
  Badge,
  PageHeader,
  SectionHeader,
  CardContent,
  ConfirmDialog,
  SearchInput,
  LoadingState,
  EmptyState,
  TextLink,
} from "@pdlc/ui/ui";
import {
  DescriptionItem,
  CodeBlock,
  Disclosure,
  NamespaceLink as Link,
  ResourceList,
  ResourceListRow,
  Card,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
  Button,
  Input,
  toast,
} from "@pdlc/ui/ui";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type ComponentProps,
} from "react";

import { useParams } from "react-router";
import { useTranslation } from "react-i18next";
import { ApiError } from "@/api/client";
import { ConfigurationContext } from "@/widgets/configuration-context";
import { ForbiddenPage } from "@/pages/forbidden";
import {
  usePipeline,
  useUpdateJobStatus,
  useJobLogPages,
  useAppendLog,
  useCancelPipeline,
  useRetryPipeline,
  useTestReport,
  useJobAttempts,
} from "@/api/hooks";

import type { Job, TestReport } from "@/api/types";

import {
  ChevronRight,
  Terminal,
  ClipboardCheck,
  Play,
  CheckCircle2,
  XCircle,
  Square,
  Ban,
  RotateCcw,
  Package,
  FileCode2,
  ArrowDownToLine,
  History,
} from "lucide-react";

import type { PipelinePlan, Status } from "@/api/types";

const statusTones: Record<string, ComponentProps<typeof Badge>["tone"]> = {
  queued: "muted",
  running: "warning",
  success: "success",
  failed: "danger",
  canceled: "muted",
};

export function PipelineDetailPage() {
  const { t } = useTranslation();
  const { pipelineId } = useParams<{ pipelineId: string }>();
  const { data, isLoading, error, refetch } = usePipeline(pipelineId);
  const updateStatus = useUpdateJobStatus();
  const cancelPipeline = useCancelPipeline();
  const retryPipeline = useRetryPipeline();
  const [selectedJobId, setSelectedJobId] = useState<string | null>(null);
  const [logMessage, setLogMessage] = useState("");
  const [confirmCancel, setConfirmCancel] = useState(false);
  const logPanelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (selectedJobId)
      logPanelRef.current?.scrollIntoView?.({ block: "start" });
  }, [selectedJobId]);

  if (isLoading) return <LoadingState message={t("common.loading")} />;
  if (error && !data) {
    if (error instanceof ApiError && error.status === 403)
      return <ForbiddenPage />;
    return (
      <Notice
        role="alert"
        tone="danger"
        appearance="inline"
        actions={
          <Button
            type="button"
            variant="outline"

            onClick={() => void refetch()}
          >
            {t("common.retry")}
          </Button>
        }
      >
        <span>{t("pipelines.loadError")}</span>
      </Notice>
    );
  }
  if (!data) return <EmptyState message={t("common.noData")} />;

  const { pipeline, stages, plan } = data;
  const selectedJob = stages
    .flatMap((s) => s.jobs)
    .find((j) => j.id === selectedJobId);

  function handleStatus(jobId: string, status: Status) {
    updateStatus.mutate(
      { jobId, status },
      {
        onSuccess: () => toast.success(t("jobs.statusUpdated")),
        onError: (e) => toast.error(e.message),
      },
    );
  }

  function handleCancel() {
    cancelPipeline.mutate(pipeline.id, {
      onSuccess: () => {
        setConfirmCancel(false);
        toast.success(t("pipelines.canceledNotice"));
      },
      onError: () => toast.error(t("pipelines.actionError")),
    });
  }

  function handleRetry() {
    retryPipeline.mutate(pipeline.id, {
      onSuccess: () => toast.success(t("pipelines.retryStarted")),
      onError: () => toast.error(t("pipelines.actionError")),
    });
  }

  return (
    <ConfigurationContext
      configurationId={pipeline.project_id}
      showSettings={false}
    >
      <div className="min-w-0 space-y-6">
        <div>
          <div className="flex min-w-0 flex-wrap items-center gap-2 text-sm text-text-muted">
            <TextLink asChild>
              <Link
                to={`/delivery-configs/${pipeline.project_id}/pipelines`}
                className="inline-flex min-h-10 items-center"
              >
                {t("navigation.pipelines")}
              </Link>
            </TextLink>
            <ChevronRight className="h-3 w-3" />
            <span>#{pipeline.id.slice(0, 8)}</span>
          </div>
          <div className="mt-2">
            <PageHeader
              title={`#${pipeline.id.slice(0, 8)}`}
              actions={
                <>
                  {(pipeline.status === "queued" ||
                    pipeline.status === "running") && (
                    <Button
                      size="sm"
                      variant="destructive-outline"
                      disabled={cancelPipeline.isPending}
                      onClick={() => setConfirmCancel(true)}
                    >
                      <Ban className="h-4 w-4" /> {t("pipelines.cancel")}
                    </Button>
                  )}
                  {(pipeline.status === "failed" ||
                    pipeline.status === "canceled") && (
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={retryPipeline.isPending}
                      onClick={handleRetry}
                    >
                      <RotateCcw className="h-4 w-4" /> {t("pipelines.retry")}
                    </Button>
                  )}
                </>
              }
              context={
                <div className="flex min-w-0 flex-wrap items-center gap-3">
                  <Badge tone="muted">
                    <code>{pipeline.git_ref}</code>
                  </Badge>
                  <Badge
                    role="status"
                    appearance="dot"
                    tone={statusTones[pipeline.status]}
                  >
                    {t(`pipelines.${pipeline.status}`)}
                  </Badge>
                </div>
              }
            />
          </div>
        </div>

        {error && (
          <Notice
            role="alert"
            tone="danger"
            appearance="inline"
            actions={
              <Button
                type="button"
                variant="outline"

                onClick={() => void refetch()}
              >
                {t("common.retry")}
              </Button>
            }
          >
            <span>{t("pipelines.loadError")}</span>
          </Notice>
        )}

        {plan && <PipelinePlanCard plan={plan} />}

        <div
          className="grid min-w-0 gap-4 lg:grid-cols-3"
          data-pipeline-layout="stage-grid"
        >
          {stages.map((stage) => (
            <Card key={stage.id} className="min-w-0">
              <CardContent>
                <SectionHeader
                  title={stage.name}
                  context={
                    <Badge appearance="dot" tone={statusTones[stage.status]}>
                      {t(`pipelines.${stage.status}`)}
                    </Badge>
                  }
                />
                <ResourceList className="mt-3">
                  {stage.jobs.map((job) => (
                    <JobCard
                      key={job.id}
                      job={job}
                      onStatus={handleStatus}
                      onShowLogs={() => setSelectedJobId(job.id)}
                      pending={updateStatus.isPending}
                    />
                  ))}
                </ResourceList>
              </CardContent>
            </Card>
          ))}
        </div>

        {selectedJob && (
          <div ref={logPanelRef} className="scroll-mt-16">
            <Card>
              <CardContent>
                <SectionHeader
                  title={
                    <>
                      <Terminal className="h-4 w-4 text-accent" />
                      {t("jobs.logs")} — {selectedJob.name}
                    </>
                  }
                  actions={
                    <Button
                      size="sm"
                      variant="outline"

                      onClick={() => setSelectedJobId(null)}
                    >
                      {t("common.close")}
                    </Button>
                  }
                />
                <JobLogPanel
                  jobId={selectedJob.id}
                  live={
                    selectedJob.status === "queued" ||
                    selectedJob.status === "running"
                  }
                  logMessage={logMessage}
                  setLogMessage={setLogMessage}
                />
              </CardContent>
            </Card>
          </div>
        )}
        {selectedJob && <JobTestReportPanel jobId={selectedJob.id} />}
        <ConfirmDialog
          open={confirmCancel}
          onOpenChange={setConfirmCancel}
          title={t("pipelines.cancelTitle")}
          description={t("pipelines.cancelDescription")}
          cancelLabel={t("pipelines.keepRunning")}
          confirmLabel={t("pipelines.confirmCancel")}
          isPending={cancelPipeline.isPending}
          onConfirm={handleCancel}
        />
      </div>
    </ConfigurationContext>
  );
}

function JobCard({
  job,
  onStatus,
  onShowLogs,
  pending,
}: {
  job: Job;
  onStatus: (jobId: string, status: Status) => void;
  onShowLogs: () => void;
  pending: boolean;
}) {
  const { t } = useTranslation();
  const terminalJobId =
    job.status === "failed" || job.status === "canceled" ? job.id : undefined;
  const { data: attempts = [] } = useJobAttempts(terminalJobId);
  const latestDiagnostic = attempts.find(
    (attempt) => attempt.error_tail,
  )?.error_tail;

  return (
    <ResourceListRow>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <ResourceListTitle asChild className="break-words"><span >{job.name}</span></ResourceListTitle>
        </div>
        <Badge appearance="dot" tone={statusTones[job.status]}>
          {t(`pipelines.${job.status}`)}
        </Badge>
      </div>
      <ResourceListDescription asChild className="mt-2 min-w-0 break-words"><p >
        <code className="rounded bg-surface-raised px-1 py-0.5">
          {job.image}
        </code>
        <span className="mx-1">·</span>
        <code className="break-all">{job.command}</code>
      </p></ResourceListDescription>
      {job.required_tags.length > 0 && (
        <div className="mt-2 flex flex-wrap items-center gap-1 text-xs text-text-muted">
          <span>{t("jobs.runnerTags")}:</span>
          {job.required_tags.map((tag) => (
            <code
              key={tag}
              className="rounded bg-surface-raised px-1.5 py-0.5 text-text-primary"
            >
              {tag}
            </code>
          ))}
        </div>
      )}
      {job.required_secrets.length > 0 && (
        <div className="mt-2 flex flex-wrap items-center gap-1 text-xs text-text-muted">
          <span>{t("jobs.secrets")}:</span>
          {job.required_secrets.map((secret) => (
            <code
              key={secret}
              className="rounded bg-surface-raised px-1.5 py-0.5 text-text-primary"
            >
              {secret}
            </code>
          ))}
        </div>
      )}
      {job.artifact_paths.length > 0 && (
        <div className="mt-2 flex flex-wrap items-center gap-1 text-xs text-text-muted">
          <span>{t("jobs.artifacts")}:</span>
          {job.artifact_paths.map((path) => (
            <code
              key={path}
              className="rounded bg-surface-raised px-1.5 py-0.5 text-text-primary"
            >
              {path}
            </code>
          ))}
        </div>
      )}
      {latestDiagnostic && (
        <Notice tone="danger" appearance="inline">
          {latestDiagnostic}
        </Notice>
      )}
      <div className="mt-3 flex flex-wrap items-center gap-1.5">
        <Button
          size="sm"
          variant="outline"

          onClick={onShowLogs}
        >
          <Terminal className="h-3 w-3" /> {t("jobs.logs")}
        </Button>
        <Button asChild size="sm" variant="outline">
          <Link to={`/jobs/${job.id}/artifacts`}>
            <Package className="h-3 w-3" /> {t("artifacts.title")}
          </Link>
        </Button>
      </div>
      {(job.status === "queued" || job.status === "running") && (
        <Disclosure
          className="group mt-2 text-sm text-text-muted"
          summary={<>{t("jobs.manualStatus")}</>}
        >
          <div className="flex flex-wrap gap-2 pb-1">
            {job.status === "queued" && (
              <Button
                size="sm"
                variant="outline"

                disabled={pending}
                onClick={() => onStatus(job.id, "running")}
              >
                <Play className="h-3 w-3" /> {t("jobs.start")}
              </Button>
            )}
            {job.status === "running" && (
              <>
                <Button
                  size="sm"
                  variant="outline"

                  disabled={pending}
                  onClick={() => onStatus(job.id, "success")}
                >
                  <CheckCircle2 className="h-3 w-3" /> {t("jobs.pass")}
                </Button>
                <Button
                  size="sm"
                  variant="outline"

                  disabled={pending}
                  onClick={() => onStatus(job.id, "failed")}
                >
                  <XCircle className="h-3 w-3" /> {t("jobs.fail")}
                </Button>
                <Button
                  size="sm"
                  variant="outline"

                  disabled={pending}
                  onClick={() => onStatus(job.id, "canceled")}
                >
                  <Square className="h-3 w-3" /> {t("jobs.cancel")}
                </Button>
              </>
            )}
          </div>
        </Disclosure>
      )}
    </ResourceListRow>
  );
}

function PipelinePlanCard({ plan }: { plan: PipelinePlan }) {
  const { t } = useTranslation();
  return (
    <Disclosure
      className="min-w-0"
      summary={
        <>
          <span className="flex min-w-0 items-center gap-2">
            <FileCode2 className="h-4 w-4 shrink-0 text-accent" aria-hidden />
            <span className="text-sm font-semibold">
              {t("pipelines.planTitle")}
            </span>
          </span>
          <span className="flex min-w-0 items-center gap-2 text-xs text-text-muted">
            <span className="break-all">{plan.config_source}</span>
          </span>
        </>
      }
    >
      <dl className="grid min-w-0 gap-3 text-xs sm:grid-cols-2 lg:grid-cols-3">
        <DescriptionItem label={t("pipelines.planParser")}>
          {plan.parser_version}
        </DescriptionItem>
        <DescriptionItem label={t("pipelines.planCommit")} technical>
          {plan.resolved_commit_sha ?? "-"}
        </DescriptionItem>
        <DescriptionItem label={t("pipelines.planEdges")}>
          {String(planDependencyCount(plan.plan))}
        </DescriptionItem>
        <DescriptionItem label={t("pipelines.planConfigHash")} technical>
          {plan.config_sha256}
        </DescriptionItem>
        <DescriptionItem label={t("pipelines.planHash")} technical>
          {plan.plan_sha256}
        </DescriptionItem>
      </dl>
    </Disclosure>
  );
}

function planDependencyCount(plan: unknown): number {
  if (!plan || typeof plan !== "object") return 0;
  const dependencies = (plan as { dependencies?: unknown }).dependencies;
  return Array.isArray(dependencies) ? dependencies.length : 0;
}

function JobLogPanel({
  jobId,
  live,
  logMessage,
  setLogMessage,
}: {
  jobId: string;
  live: boolean;
  logMessage: string;
  setLogMessage: (v: string) => void;
}) {
  const { t } = useTranslation();
  const attemptsQuery = useJobAttempts(jobId, live);
  const attempts = useMemo(
    () => attemptsQuery.data ?? [],
    [attemptsQuery.data],
  );
  const [selectedAttemptId, setSelectedAttemptId] = useState<string | null>(
    null,
  );
  const [logSearch, setLogSearch] = useState("");
  const [appliedLogSearch, setAppliedLogSearch] = useState("");
  const selectedAttempt =
    attempts.find((a) => a.id === selectedAttemptId) ?? attempts[0];
  const logPages = useJobLogPages(
    jobId,
    selectedAttempt?.id,
    appliedLogSearch,
    live,
  );
  const tailMode = appliedLogSearch.length === 0;
  const logs = useMemo(() => {
    const pages = logPages.data?.pages ?? [];
    const orderedPages = tailMode ? [...pages].reverse() : pages;
    return orderedPages.flatMap((page) => page.items);
  }, [logPages.data, tailMode]);
  const logViewportRef = useRef<HTMLPreElement>(null);
  const followTailRef = useRef(true);
  const preserveScrollHeightRef = useRef<number | null>(null);
  const previousLastSequenceRef = useRef<number | null>(null);
  const [showLatestLogs, setShowLatestLogs] = useState(false);
  const initialLogsError = logPages.isError && logPages.data == null;
  const nextLogsError = logPages.isFetchNextPageError;
  const refreshLogsError = logPages.isRefetchError && !nextLogsError;
  const appendLog = useAppendLog();
  const activeAttemptId = attempts[0]?.id;
  const canAppend = !!selectedAttempt && selectedAttempt.id === activeAttemptId;

  useEffect(() => {
    const timeout = window.setTimeout(
      () => setAppliedLogSearch(logSearch.trim()),
      300,
    );
    return () => window.clearTimeout(timeout);
  }, [logSearch]);

  useEffect(() => {
    if (attempts.length === 0) {
      setSelectedAttemptId(null);
      return;
    }
    if (
      !selectedAttemptId ||
      !attempts.some((a) => a.id === selectedAttemptId)
    ) {
      setSelectedAttemptId(attempts[0].id);
    }
  }, [attempts, selectedAttemptId]);

  useEffect(() => {
    followTailRef.current = tailMode;
    preserveScrollHeightRef.current = null;
    previousLastSequenceRef.current = null;
    setShowLatestLogs(false);
  }, [appliedLogSearch, selectedAttempt?.id, tailMode]);

  useEffect(() => {
    const lastSequence = logs[logs.length - 1]?.sequence ?? null;
    if (!tailMode) {
      previousLastSequenceRef.current = lastSequence;
      return;
    }

    const viewport = logViewportRef.current;
    const preservedHeight = preserveScrollHeightRef.current;
    if (viewport && preservedHeight != null) {
      viewport.scrollTop += viewport.scrollHeight - preservedHeight;
      preserveScrollHeightRef.current = null;
      previousLastSequenceRef.current = lastSequence;
      return;
    }

    const previousLastSequence = previousLastSequenceRef.current;
    if (viewport && followTailRef.current && logs.length > 0) {
      viewport.scrollTop = viewport.scrollHeight;
    } else if (
      previousLastSequence != null &&
      lastSequence != null &&
      lastSequence > previousLastSequence
    ) {
      setShowLatestLogs(true);
    }
    previousLastSequenceRef.current = lastSequence;
  }, [logs, tailMode]);

  function loadNextLogPage() {
    if (tailMode) {
      preserveScrollHeightRef.current =
        logViewportRef.current?.scrollHeight ?? null;
      followTailRef.current = false;
      setShowLatestLogs(true);
    }
    void logPages.fetchNextPage();
  }

  function scrollToLatestLog() {
    const viewport = logViewportRef.current;
    if (viewport) viewport.scrollTop = viewport.scrollHeight;
    followTailRef.current = true;
    setShowLatestLogs(false);
  }

  return (
    <div className="mt-3 space-y-3">
      {attemptsQuery.isError && (
        <Notice
          role="alert"
          tone="danger"
          appearance="inline"
          actions={
            <Button
              type="button"
              variant="outline"

              onClick={() => void attemptsQuery.refetch()}
            >
              {t("common.retry")}
            </Button>
          }
        >
          <span>{t("jobs.attemptsError")}</span>
        </Notice>
      )}
      {attempts.length > 0 && (
        <div className="space-y-2">
          <div className="flex flex-wrap gap-2">
            {attempts.map((attempt) => (
              <Button
                key={attempt.id}
                type="button"
                size="sm"
                variant={
                  attempt.id === selectedAttempt?.id ? "default" : "outline"
                }

                onClick={() => setSelectedAttemptId(attempt.id)}
              >
                {t("jobs.attempt")} #{attempt.attempt_no} ·{" "}
                {t(`pipelines.${attempt.status}`)}
              </Button>
            ))}
          </div>
          {selectedAttempt && (
            <div className="grid gap-2 text-xs text-text-muted sm:grid-cols-2 lg:grid-cols-4">
              <span>
                {t("jobs.trigger")}: {selectedAttempt.trigger}
              </span>
              <span>
                {t("jobs.exitCode")}: {selectedAttempt.exit_code ?? "-"}
              </span>
              <span>
                {t("jobs.startedAt")}:{" "}
                {formatAttemptTime(selectedAttempt.started_at)}
              </span>
              <span>
                {t("jobs.finishedAt")}:{" "}
                {formatAttemptTime(selectedAttempt.finished_at)}
              </span>
            </div>
          )}
          {selectedAttempt?.error_tail && (
            <Notice tone="danger" appearance="inline">
              {selectedAttempt.error_tail}
            </Notice>
          )}
        </div>
      )}
      {selectedAttempt && (
        <>
          <div className="flex flex-wrap gap-2">
            <SearchInput
              aria-label={t("jobs.searchLogs")}
              value={logSearch}
              onChange={(e) => setLogSearch(e.target.value)}
              placeholder={t("jobs.searchLogs")}
              containerClassName="min-w-40 flex-1"
              className="font-mono"
            />
            {logSearch && (
              <Button
                type="button"
                size="sm"
                variant="outline"

                onClick={() => setLogSearch("")}
              >
                {t("common.clear")}
              </Button>
            )}
            <Button
              type="button"
              size="sm"
              variant="outline"

              onClick={() => void logPages.refetch()}
            >
              <RotateCcw className="h-4 w-4" aria-hidden />{" "}
              {t("jobs.refreshLogs")}
            </Button>
          </div>
          {initialLogsError && (
            <Notice
              role="alert"
              tone="danger"
              appearance="inline"
              actions={
                <Button
                  type="button"
                  variant="outline"

                  onClick={() => void logPages.refetch()}
                >
                  {t("common.retry")}
                </Button>
              }
            >
              <span>{t("jobs.logsError")}</span>
            </Notice>
          )}
          {nextLogsError && (
            <Notice
              role="alert"
              tone="danger"
              appearance="inline"
              actions={
                <Button
                  type="button"
                  variant="outline"

                  onClick={loadNextLogPage}
                >
                  {t("common.retry")}
                </Button>
              }
            >
              <span>{t("jobs.logsPageError")}</span>
            </Notice>
          )}
          {refreshLogsError && (
            <Notice
              role="alert"
              tone="danger"
              appearance="inline"
              actions={
                <Button
                  type="button"
                  variant="outline"

                  onClick={() => void logPages.refetch()}
                >
                  {t("common.retry")}
                </Button>
              }
            >
              <span>{t("jobs.logsRefreshError")}</span>
            </Notice>
          )}
          {tailMode &&
            logPages.hasNextPage &&
            !initialLogsError &&
            !nextLogsError && (
              <Button
                type="button"
                size="sm"
                variant="outline"

                disabled={logPages.isFetchingNextPage}
                onClick={loadNextLogPage}
              >
                <History className="h-4 w-4" aria-hidden />
                {logPages.isFetchingNextPage
                  ? t("common.loading")
                  : t("jobs.loadEarlierLogs")}
              </Button>
            )}
          {!initialLogsError && (
            <CodeBlock
              ref={logViewportRef}
              role="log"
              aria-label={t("jobs.logs")}
              tabIndex={0}
              onScroll={(event) => {
                if (!tailMode) return;
                const viewport = event.currentTarget;
                const nearBottom =
                  viewport.scrollHeight -
                    viewport.scrollTop -
                    viewport.clientHeight <=
                  24;
                followTailRef.current = nearBottom;
                setShowLatestLogs(!nearBottom);
              }}
              className="max-h-80"
              wrap={false}
            >
              {logPages.isLoading
                ? t("common.loading")
                : logs.length === 0
                  ? t("jobs.noLogs")
                  : logs
                      .map(
                        (l) =>
                          `${String(l.sequence).padStart(3, "0")}  ${l.message}`,
                      )
                      .join("\n")}
            </CodeBlock>
          )}
          {tailMode &&
            showLatestLogs &&
            logs.length > 0 &&
            !initialLogsError && (
              <Button
                type="button"
                size="sm"
                variant="outline"

                onClick={scrollToLatestLog}
              >
                <ArrowDownToLine className="h-4 w-4" aria-hidden />{" "}
                {t("jobs.latestLogs")}
              </Button>
            )}
          {!tailMode &&
            logPages.hasNextPage &&
            !initialLogsError &&
            !nextLogsError && (
              <Button
                type="button"
                size="sm"
                variant="outline"

                disabled={logPages.isFetchingNextPage}
                onClick={loadNextLogPage}
              >
                {logPages.isFetchingNextPage
                  ? t("common.loading")
                  : t("jobs.loadMoreLogs")}
              </Button>
            )}
        </>
      )}
      {!selectedAttempt &&
        !attemptsQuery.isLoading &&
        !attemptsQuery.isError && (
          <p className="text-sm text-text-muted">{t("jobs.noLogs")}</p>
        )}
      {canAppend && (
        <Disclosure
          className="group text-sm text-text-muted"
          summary={<>{t("jobs.manualLog")}</>}
        >
          <form
            className="flex flex-wrap gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              if (!logMessage.trim() || appendLog.isPending) return;
              appendLog.mutate(
                { jobId, message: logMessage.trim() },
                {
                  onSuccess: () => setLogMessage(""),
                  onError: (err) => toast.error(err.message),
                },
              );
            }}
          >
            <Input
              aria-label={t("jobs.logMessage")}
              value={logMessage}
              onChange={(e) => setLogMessage(e.target.value)}
              placeholder={t("jobs.logMessage")}
              className="min-w-40 flex-1 font-mono text-sm"
              disabled={appendLog.isPending}
            />
            <Button
              type="submit"
              size="sm"

              disabled={appendLog.isPending || !logMessage.trim()}
            >
              {t("jobs.append")}
            </Button>
          </form>
        </Disclosure>
      )}
    </div>
  );
}

function formatAttemptTime(value: string | null): string {
  return value ? new Date(value).toLocaleString() : "-";
}

function JobTestReportPanel({ jobId }: { jobId: string }) {
  const { t } = useTranslation();
  const {
    data: reports = [],
    isLoading,
    isError,
    refetch,
  } = useTestReport(jobId);
  if (isLoading) return null;
  if (isError)
    return (
      <Notice
        role="alert"
        tone="danger"
        appearance="inline"
        actions={
          <Button
            type="button"
            variant="outline"

            onClick={() => void refetch()}
          >
            {t("common.retry")}
          </Button>
        }
      >
        <span>{t("jobs.reportError")}</span>
      </Notice>
    );
  if (reports.length === 0) return null;
  const total = reports.reduce<{
    total: number;
    passed: number;
    failed: number;
    skipped: number;
  }>(
    (acc, r) => ({
      total: acc.total + r.tests_total,
      passed: acc.passed + r.tests_passed,
      failed: acc.failed + r.tests_failed,
      skipped: acc.skipped + r.tests_skipped,
    }),
    { total: 0, passed: 0, failed: 0, skipped: 0 },
  );
  return (
    <Card>
      <CardContent>
        <SectionHeader
          title={
            <>
              <ClipboardCheck className="h-4 w-4 text-accent" />
              {t("jobs.testReport", "Тест-отчёты")} — {selectedLabel(total)}
            </>
          }
        />
        <ResourceList asChild>
          <div className="mt-3 md:hidden">
            {reports.map((r: TestReport) => (
              <ResourceListRow asChild key={r.id}>
                <div className="text-sm">
                  <ResourceListTitle asChild><p >{r.suite_name}</p></ResourceListTitle>
                  <ResourceListDescription asChild className="mt-1"><p >
                    ✓{r.tests_passed} ✗{r.tests_failed} ⇒{r.tests_skipped} /{" "}
                    {r.tests_total}
                    {r.duration_ms != null && ` · ${r.duration_ms}ms`}
                  </p></ResourceListDescription>
                </div>
              </ResourceListRow>
            ))}
          </div>
        </ResourceList>
        <div className="mt-3 hidden md:block">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t("jobs.suite", "Набор")}</TableHead>
                <TableHead>✓</TableHead>
                <TableHead>✗</TableHead>
                <TableHead>⇢</TableHead>
                <TableHead>{t("jobs.total", "Всего")}</TableHead>
                <TableHead>{t("jobs.duration", "Время")}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {reports.map((r: TestReport) => (
                <TableRow key={r.id}>
                  <TableCell className="font-medium">{r.suite_name}</TableCell>
                  <TableCell className="text-success">
                    {r.tests_passed}
                  </TableCell>
                  <TableCell
                    className={r.tests_failed > 0 ? "text-destructive" : ""}
                  >
                    {r.tests_failed}
                  </TableCell>
                  <TableCell className="text-text-muted">
                    {r.tests_skipped}
                  </TableCell>
                  <TableCell>{r.tests_total}</TableCell>
                  <TableCell className="text-text-muted">
                    {r.duration_ms != null ? `${r.duration_ms}ms` : "-"}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      </CardContent>
    </Card>
  );

  function selectedLabel(sum: {
    total: number;
    passed: number;
    failed: number;
    skipped: number;
  }): string {
    return `${sum.passed}/${sum.total}`;
  }
}
