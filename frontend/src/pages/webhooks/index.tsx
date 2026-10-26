import {
  EmptyState,
  ResourceListDescription,
  ResourceListTitle,
  PageNavigation,
  CardContent,
} from "@pdlc/ui/ui";
import { FormField } from "@pdlc/ui/ui";
import { Notice, Badge, PageHeader, SectionHeader } from "@pdlc/ui/ui";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useParams, useSearchParams } from "react-router";
import {
  useWebhooks,
  useCreateWebhook,
  useDeleteWebhook,
  useOutboxDeliveries,
  useOutboxDelivery,
  useRequeueOutboxDelivery,
  useNotifications,
  useNotificationEvents,
  useSaveNotifications,
} from "@/api/hooks";
import {
  Select,
  Card,
  ResourceList,
  ResourceListRow,
  Button,
  Input,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
  toast,
  ConfirmDialog,
} from "@pdlc/ui/ui";

import { RotateCcw, Webhook, Plus, Trash2, X } from "lucide-react";

import { QueryState } from "@/shared/ui/query-state";
import type {
  NotificationConfig,
  NotificationEvent,
  NotificationInput,
  OutboxDelivery,
  Webhook as WebhookType,
} from "@/api/types";

const deliveryPageSize = 20;
const maxDeliveryPage = Math.floor(0xffff_ffff / deliveryPageSize) + 1;
const deliveryStatuses = [
  "pending",
  "retry_scheduled",
  "delivered",
  "failed",
] as const;
const deliveryChannels = ["webhook", "notification", "sse"] as const;
type DeliveryStatusFilter = (typeof deliveryStatuses)[number] | "all";
type DeliveryChannelFilter = (typeof deliveryChannels)[number] | "all";

export function WebhooksPage() {
  const { t } = useTranslation();
  const { projectId } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const viewParam = searchParams.get("view");
  const view =
    viewParam === "deliveries" || viewParam === "notifications"
      ? viewParam
      : "webhooks";
  const webhooksQuery = useWebhooks(projectId);
  const createWebhook = useCreateWebhook(projectId);
  const deleteWebhook = useDeleteWebhook();
  const [showForm, setShowForm] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<WebhookType | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [form, setForm] = useState({
    url: "",
    events: "pipeline.started, pipeline.finished",
  });

  function changeView(next: string) {
    const params = new URLSearchParams(searchParams);
    if (next === "webhooks") params.delete("view");
    else params.set("view", next);
    setSearchParams(params, { replace: true });
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    createWebhook.mutate(
      {
        url: form.url,
        events: form.events
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean),
      },
      {
        onSuccess: () => {
          setShowForm(false);
          setForm({ url: "", events: "pipeline.started, pipeline.finished" });
          toast.success(t("webhooks.created"));
        },
        onError: (err) => toast.error(err.message),
      },
    );
  }

  function handleDelete(w: WebhookType) {
    setDeleteError(null);
    setPendingDelete(w);
  }

  return (
    <div className="space-y-4">
      <PageHeader
        title={t("webhooks.pageTitle")}
        leading={<Webhook className="h-5 w-5 text-accent" />}
      />
      <Tabs value={view} onValueChange={changeView}>
        <TabsList className="grid h-auto w-full max-w-xl grid-cols-3">
          <TabsTrigger value="webhooks" className="min-w-0 whitespace-normal">
            {t("webhooks.title")}
          </TabsTrigger>
          <TabsTrigger value="deliveries" className="min-w-0 whitespace-normal">
            {t("deliveries.title")}
          </TabsTrigger>
          <TabsTrigger
            value="notifications"
            className="min-w-0 whitespace-normal"
          >
            {t("notifications.title")}
          </TabsTrigger>
        </TabsList>

        <TabsContent value="webhooks" className="mt-4 space-y-4">
          <div className="flex justify-end">
            <Button
              type="button"
              size="sm"

              onClick={() => setShowForm((v) => !v)}
            >
              <Plus className="h-4 w-4" />
              {t("webhooks.create")}
            </Button>
          </div>
          {showForm && (
            <Card>
              <CardContent>
                <form onSubmit={handleSubmit} className="grid gap-3">
                  <FormField id="wh-url" label={t("webhooks.url")} required>
                    {(attrs) => (
                      <Input
                        {...attrs}

                        placeholder="https://example.com/hook"
                        value={form.url}
                        onChange={(e) =>
                          setForm({ ...form, url: e.target.value })
                        }
                      />
                    )}
                  </FormField>
                  <FormField id="wh-events" label={t("webhooks.events")}>
                    {(attrs) => (
                      <Input
                        {...attrs}

                        placeholder="pipeline.started, pipeline.finished"
                        value={form.events}
                        onChange={(e) =>
                          setForm({ ...form, events: e.target.value })
                        }
                      />
                    )}
                  </FormField>
                  <div className="flex flex-wrap gap-2">
                    <Button
                      type="submit"

                      disabled={createWebhook.isPending}
                    >
                      {t("webhooks.create")}
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"

                      onClick={() => setShowForm(false)}
                    >
                      {t("common.cancel")}
                    </Button>
                  </div>
                </form>
              </CardContent>
            </Card>
          )}

          <QueryState
            data={webhooksQuery.data}
            isLoading={webhooksQuery.isLoading}
            error={webhooksQuery.error}
            errorMessage={t("webhooks.loadFailed")}
            onRetry={() => void webhooksQuery.refetch()}
            isEmpty={(list) => list.length === 0}
            empty={{ title: t("webhooks.empty") }}
          >
            {(webhooks) => (
              <Card className="overflow-hidden">
                <ResourceList asChild>
                  <div className="md:hidden">
                    {webhooks.map((w) => (
                      <ResourceListRow asChild key={w.id}>
                        <div className="flex items-start gap-2">
                          <div className="min-w-0 flex-1 space-y-2">
                            <ResourceListTitle asChild technical>
                              <p className="break-all">
                              {w.url}
                            </p>
                            </ResourceListTitle>
                            <ResourceListDescription asChild>
                              <p >
                              {w.events.length ? w.events.join(", ") : "—"}
                            </p>
                            </ResourceListDescription>
                            <WebhookStatusBadge enabled={w.enabled} />
                          </div>
                          <Button
                            type="button"
                            size="icon"
                            variant="destructive-outline"
                            aria-label={`${t("common.delete")} ${w.url}`}
                            title={t("common.delete")}
                            className="shrink-0"
                            onClick={() => handleDelete(w)}
                          >
                            <Trash2 className="h-4 w-4" />
                          </Button>
                        </div>
                      </ResourceListRow>
                    ))}
                  </div>
                </ResourceList>
                <div className="hidden overflow-x-auto md:block">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>{t("webhooks.url")}</TableHead>
                        <TableHead>{t("webhooks.events")}</TableHead>
                        <TableHead>{t("webhooks.enabled")}</TableHead>
                        <TableHead className="w-14">
                          <span className="sr-only">{t("common.actions")}</span>
                        </TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {webhooks.map((w) => (
                        <TableRow key={w.id}>
                          <TableCell className="max-w-xl break-all font-mono text-xs">
                            {w.url}
                          </TableCell>
                          <TableCell className="text-xs text-text-secondary">
                            {w.events.length ? w.events.join(", ") : "—"}
                          </TableCell>
                          <TableCell>
                            <WebhookStatusBadge enabled={w.enabled} />
                          </TableCell>
                          <TableCell>
                            <Button
                              type="button"
                              size="icon"
                              variant="destructive-outline"
                              aria-label={`${t("common.delete")} ${w.url}`}
                              title={t("common.delete")}

                              onClick={() => handleDelete(w)}
                            >
                              <Trash2 className="h-4 w-4" />
                            </Button>
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
              </Card>
            )}
          </QueryState>
        </TabsContent>
        <TabsContent value="deliveries" className="mt-4">
          <DeliveryHistorySection />
        </TabsContent>
        <TabsContent value="notifications" className="mt-4">
          <NotificationsSection />
        </TabsContent>
      </Tabs>

      <ConfirmDialog
        open={pendingDelete !== null}
        title={t("webhooks.deleteConfirm")}
        description={deleteError ?? pendingDelete?.url}
        isPending={deleteWebhook.isPending}
        closeOnConfirm={false}
        onOpenChange={(open) => {
          if (open) return;
          setPendingDelete(null);
          setDeleteError(null);
        }}
        onConfirm={() => {
          if (pendingDelete)
            deleteWebhook.mutate(pendingDelete.id, {
              onSuccess: () => {
                setPendingDelete(null);
                toast.success(t("webhooks.deleted"));
              },
              onError: () => setDeleteError(t("webhooks.deleteFailed")),
            });
        }}
        confirmLabel={t("common.delete")}
        pendingLabel={t("common.delete")}
      />
    </div>
  );
}

function WebhookStatusBadge({ enabled }: { enabled: boolean }) {
  const { t } = useTranslation();
  return (
    <Badge tone={enabled ? "success" : "muted"}>
      {enabled ? t("webhooks.on") : t("webhooks.off")}
    </Badge>
  );
}

function DeliveryHistorySection() {
  const { t } = useTranslation();
  const { projectId } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const rawPage = Number(searchParams.get("page"));
  const page =
    Number.isSafeInteger(rawPage) && rawPage > 0 && rawPage <= maxDeliveryPage
      ? rawPage
      : 1;
  const rawStatus = searchParams.get("status");
  const status: DeliveryStatusFilter = deliveryStatuses.includes(
    rawStatus as (typeof deliveryStatuses)[number],
  )
    ? (rawStatus as (typeof deliveryStatuses)[number])
    : "all";
  const rawChannel = searchParams.get("channel");
  const channel: DeliveryChannelFilter = deliveryChannels.includes(
    rawChannel as (typeof deliveryChannels)[number],
  )
    ? (rawChannel as (typeof deliveryChannels)[number])
    : "all";
  const deliveriesQuery = useOutboxDeliveries(projectId, {
    limit: deliveryPageSize,
    offset: (page - 1) * deliveryPageSize,
    status: status === "all" ? undefined : status,
    channel: channel === "all" ? undefined : channel,
  });
  const detail = useOutboxDelivery(selectedId);
  const requeue = useRequeueOutboxDelivery();
  const selectedDelivery = detail.data?.delivery;
  const hasFilters = status !== "all" || channel !== "all";

  useEffect(() => setSelectedId(null), [page, status, channel]);

  useEffect(() => {
    if (!deliveriesQuery.data || deliveriesQuery.isPlaceholderData) return;
    const lastPage = Math.max(
      1,
      Math.ceil(deliveriesQuery.data.total / deliveryPageSize),
    );
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
  }, [
    deliveriesQuery.data,
    deliveriesQuery.isPlaceholderData,
    page,
    setSearchParams,
  ]);

  function changePage(nextPage: number) {
    setSearchParams((current) => {
      const next = new URLSearchParams(current);
      if (nextPage <= 1) next.delete("page");
      else next.set("page", String(nextPage));
      return next;
    });
  }

  function changeFilter(key: "status" | "channel", value: string) {
    setSearchParams(
      (current) => {
        const next = new URLSearchParams(current);
        if (value === "all") next.delete(key);
        else next.set(key, value);
        next.delete("page");
        return next;
      },
      { replace: true },
    );
  }

  function handleRequeue(delivery: OutboxDelivery) {
    requeue.mutate(delivery.id, {
      onSuccess: (result) => {
        setSelectedId(result.id);
        toast.success(t("deliveries.requeued"));
      },
      onError: (err) => toast.error(err.message),
    });
  }

  return (
    <div className="space-y-4">
      <QueryState
        data={deliveriesQuery.data}
        isLoading={deliveriesQuery.isLoading}
        error={deliveriesQuery.error}
        errorMessage={t("deliveries.loadFailed")}
        onRetry={() => void deliveriesQuery.refetch()}
        isEmpty={(result) => result.total === 0 && !hasFilters}
        empty={{ title: t("deliveries.empty") }}
      >
        {(result) => {
          const pageCount = Math.max(
            1,
            Math.ceil(result.total / deliveryPageSize),
          );
          const firstItem = result.total === 0 ? 0 : result.offset + 1;
          const lastItem = result.offset + result.items.length;
          return (
            <section
              aria-label={t("deliveries.title")}
              aria-busy={deliveriesQuery.isFetching}
              className="space-y-3"
            >
              <div className="grid gap-2 sm:grid-cols-2">
                <FormField
                  id="delivery-status"
                  label={t("deliveries.statusFilter")}
                >
                  {(attrs) => (
                    <Select
                      {...attrs}
                      value={status}
                      onValueChange={(nextValue) =>
                        changeFilter("status", nextValue)
                      }
                      className="w-full"
                      options={[
                        {
                          value: "all",
                          label: String(t("deliveries.allStatuses")),
                        },
                        ...(deliveryStatuses.map((value) => ({
                          value: String(value),
                          label: String(
                            t(
                              value === "retry_scheduled"
                                ? "deliveries.retryScheduled"
                                : `deliveries.${value}`,
                            ),
                          ),
                        })) ?? []),
                      ]}
                    />
                  )}
                </FormField>
                <FormField
                  id="delivery-channel"
                  label={t("deliveries.channelFilter")}
                >
                  {(attrs) => (
                    <Select
                      {...attrs}
                      value={channel}
                      onValueChange={(nextValue) =>
                        changeFilter("channel", nextValue)
                      }
                      className="w-full"
                      options={[
                        {
                          value: "all",
                          label: String(t("deliveries.allChannels")),
                        },
                        ...(deliveryChannels.map((value) => ({
                          value: String(value),
                          label: String(t(`deliveries.channel_${value}`)),
                        })) ?? []),
                      ]}
                    />
                  )}
                </FormField>
              </div>
              <p
                className="flex flex-wrap gap-2 text-xs text-text-muted"
                aria-live="polite"
              >
                <span>
                  {t("deliveries.shown", {
                    from: firstItem,
                    to: lastItem,
                    total: result.total,
                  })}
                </span>
                {deliveriesQuery.isFetching && (
                  <span>{t("common.loading")}</span>
                )}
              </p>
              {result.total === 0 ? (
                <Notice tone="neutral" appearance="inline" role="status">
                  {t("deliveries.noMatches")}
                </Notice>
              ) : (
                <Card className="overflow-hidden">
                  <ResourceList asChild>
                    <div className="md:hidden">
                      {result.items.map((delivery) => (
                        <ResourceListRow asChild key={delivery.id}>
                          <div className="space-y-2">
                            <DeliveryOpenButton
                              delivery={delivery}
                              selected={selectedId === delivery.id}
                              onClick={() =>
                                setSelectedId((current) =>
                                  current === delivery.id ? null : delivery.id,
                                )
                              }
                            />
                            <ResourceListDescription asChild technical>
                              <p className="break-all">
                              {delivery.channel} / {delivery.destination}
                            </p>
                            </ResourceListDescription>
                            <div className="flex flex-wrap items-center gap-3 text-xs text-text-secondary">
                              <DeliveryStatusBadge status={delivery.status} />
                              <span>
                                {t("deliveries.attempts")}: {delivery.attempts}
                              </span>
                              <time dateTime={delivery.created_at}>
                                {new Date(delivery.created_at).toLocaleString()}
                              </time>
                            </div>
                            {delivery.status === "failed" && (
                              <Button
                                type="button"
                                size="sm"
                                variant="outline"

                                disabled={requeue.isPending}
                                onClick={() => handleRequeue(delivery)}
                              >
                                <RotateCcw className="h-4 w-4" />{" "}
                                {t("deliveries.requeue")}
                              </Button>
                            )}
                          </div>
                        </ResourceListRow>
                      ))}
                    </div>
                  </ResourceList>
                  <div className="hidden overflow-x-auto md:block">
                    <Table>
                      <TableHeader>
                        <TableRow>
                          <TableHead>{t("deliveries.event")}</TableHead>
                          <TableHead>{t("deliveries.destination")}</TableHead>
                          <TableHead>{t("deliveries.status")}</TableHead>
                          <TableHead>{t("deliveries.attempts")}</TableHead>
                          <TableHead>{t("deliveries.created")}</TableHead>
                          <TableHead className="w-28">
                            <span className="sr-only">
                              {t("common.actions")}
                            </span>
                          </TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {result.items.map((delivery) => (
                          <TableRow key={delivery.id}>
                            <TableCell className="max-w-xl">
                              <DeliveryOpenButton
                                delivery={delivery}
                                selected={selectedId === delivery.id}
                                onClick={() =>
                                  setSelectedId((current) =>
                                    current === delivery.id
                                      ? null
                                      : delivery.id,
                                  )
                                }
                              />
                            </TableCell>
                            <TableCell className="max-w-md break-all font-mono text-xs">
                              {delivery.channel} / {delivery.destination}
                            </TableCell>
                            <TableCell>
                              <DeliveryStatusBadge status={delivery.status} />
                            </TableCell>
                            <TableCell className="font-mono text-xs">
                              {delivery.attempts}
                            </TableCell>
                            <TableCell className="text-xs text-text-secondary">
                              <time dateTime={delivery.created_at}>
                                {new Date(delivery.created_at).toLocaleString()}
                              </time>
                            </TableCell>
                            <TableCell>
                              {delivery.status === "failed" && (
                                <Button
                                  type="button"
                                  size="sm"
                                  variant="ghost"

                                  disabled={requeue.isPending}
                                  onClick={() => handleRequeue(delivery)}
                                >
                                  <RotateCcw className="h-4 w-4" />{" "}
                                  {t("deliveries.requeue")}
                                </Button>
                              )}
                            </TableCell>
                          </TableRow>
                        ))}
                      </TableBody>
                    </Table>
                  </div>
                </Card>
              )}
              {result.total > deliveryPageSize && (
                <PageNavigation
                  page={page}
                  pageCount={pageCount}
                  onPageChange={changePage}
                  label={t("deliveries.pages")}
                  previousLabel={t("deliveries.previous")}
                  nextLabel={t("deliveries.next")}
                  pending={deliveriesQuery.isFetching}
                />
              )}
            </section>
          );
        }}
      </QueryState>
      {selectedId && (
        <Card id="delivery-details">
          <CardContent>
            <div className="mb-3 flex items-center justify-between gap-3">
              <SectionHeader level={3} title={t("deliveries.details")} />
              <div className="flex items-center gap-2">
                {selectedDelivery?.status === "failed" && (
                  <Button
                    type="button"
                    size="sm"

                    disabled={requeue.isPending}
                    onClick={() => handleRequeue(selectedDelivery)}
                  >
                    <RotateCcw className="h-4 w-4" /> {t("deliveries.requeue")}
                  </Button>
                )}
                <Button
                  type="button"
                  size="icon"
                  variant="ghost"

                  aria-label={t("common.close")}
                  title={t("common.close")}
                  onClick={() => setSelectedId(null)}
                >
                  <X className="h-4 w-4" />
                </Button>
              </div>
            </div>
            <QueryState
              data={detail.data}
              isLoading={detail.isLoading}
              error={detail.error}
              errorMessage={t("deliveries.detailFailed")}
              onRetry={() => void detail.refetch()}
            >
              {(data) =>
                data.attempts.length === 0 ? (
                  <EmptyState message={t("deliveries.noAttempts")} />
                ) : (
                  <>
                    <ResourceList asChild>
                      <div className="md:hidden">
                        {data.attempts.map((attempt) => (
                          <ResourceListRow asChild key={attempt.id}>
                            <div className="space-y-1 text-xs">
                              <div className="flex items-center justify-between gap-2">
                                <ResourceListTitle asChild>
                                  <span >
                                  #{attempt.attempt_number}
                                </span>
                                </ResourceListTitle>
                                <DeliveryStatusBadge status={attempt.outcome} />
                              </div>
                              <ResourceListDescription asChild>
                                <p>
                                {t("deliveries.httpStatus")}:{" "}
                                {attempt.http_status ?? "—"} ·{" "}
                                {attempt.duration_ms} ms
                              </p>
                              </ResourceListDescription>
                              {attempt.error_message && (
                                <Notice
                                  tone="danger"
                                  appearance="inline"
                                  className="break-all"
                                >
                                  {attempt.error_message}
                                </Notice>
                              )}
                            </div>
                          </ResourceListRow>
                        ))}
                      </div>
                    </ResourceList>
                    <div className="hidden overflow-x-auto md:block">
                      <Table>
                        <TableHeader>
                          <TableRow>
                            <TableHead>{t("deliveries.attempt")}</TableHead>
                            <TableHead>{t("deliveries.outcome")}</TableHead>
                            <TableHead>{t("deliveries.httpStatus")}</TableHead>
                            <TableHead>{t("deliveries.duration")}</TableHead>
                            <TableHead>{t("deliveries.error")}</TableHead>
                          </TableRow>
                        </TableHeader>
                        <TableBody>
                          {data.attempts.map((attempt) => (
                            <TableRow key={attempt.id}>
                              <TableCell className="font-mono text-xs">
                                {attempt.attempt_number}
                              </TableCell>
                              <TableCell>
                                <DeliveryStatusBadge status={attempt.outcome} />
                              </TableCell>
                              <TableCell className="font-mono text-xs">
                                {attempt.http_status ?? "—"}
                              </TableCell>
                              <TableCell className="font-mono text-xs">
                                {attempt.duration_ms} ms
                              </TableCell>
                              <TableCell className="max-w-lg break-all text-xs text-text-secondary">
                                {attempt.error_message ?? "—"}
                              </TableCell>
                            </TableRow>
                          ))}
                        </TableBody>
                      </Table>
                    </div>
                  </>
                )
              }
            </QueryState>
          </CardContent>
        </Card>
      )}
    </div>
  );
}

function DeliveryOpenButton({
  delivery,
  selected,
  onClick,
}: {
  delivery: OutboxDelivery;
  selected: boolean;
  onClick: () => void;
}) {
  const { t } = useTranslation();
  return (
    <Button
      variant="ghost"
      type="button"
      className="w-full flex-col items-start whitespace-normal text-left"
      aria-label={t("deliveries.openDetails", {
        event: delivery.event_type,
        id: delivery.aggregate_id.slice(0, 8),
      })}
      aria-expanded={selected}
      aria-controls={selected ? "delivery-details" : undefined}
      onClick={onClick}
    >
      <span className="block text-sm font-medium">{delivery.event_type}</span>
      <span className="mt-1 block font-mono text-xs text-text-secondary">
        {delivery.aggregate_type} · {delivery.aggregate_id.slice(0, 8)} · gen{" "}
        {delivery.generation}
      </span>
      {delivery.last_error && (
        <span className="mt-1 block text-xs text-danger">
          {delivery.last_error}
        </span>
      )}
    </Button>
  );
}

function DeliveryStatusBadge({ status }: { status: string }) {
  const { t } = useTranslation();
  const tone =
    status === "delivered"
      ? "success"
      : status === "failed"
        ? "danger"
        : status === "retry_scheduled"
          ? "warning"
          : "muted";
  const labelKey =
    status === "retry_scheduled"
      ? "deliveries.retryScheduled"
      : `deliveries.${status}`;
  return <Badge tone={tone}>{t(labelKey)}</Badge>;
}

function NotificationsSection() {
  const { t } = useTranslation();
  const { projectId } = useParams();
  const notificationsQuery = useNotifications(projectId);
  const eventsQuery = useNotificationEvents(projectId);
  const save = useSaveNotifications(projectId);
  const [items, setItems] = useState<NotificationInput[]>([]);
  const [loadedData, setLoadedData] = useState<
    NotificationConfig[] | undefined
  >();
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    if (notificationsQuery.data) {
      setItems(
        notificationsQuery.data.map((n) => ({
          channel: n.channel,
          target: n.target,
          enabled: n.enabled,
          aggregation_window_secs: n.aggregation_window_secs,
          quiet_start_min: n.quiet_start_min,
          quiet_end_min: n.quiet_end_min,
          quiet_action: n.quiet_action,
          quiet_bypass_statuses: n.quiet_bypass_statuses,
        })),
      );
      setLoadedData(notificationsQuery.data);
    }
  }, [notificationsQuery.data]);

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setSaveError(null);
    save.mutate(items, {
      onSuccess: () => toast.success(t("notifications.saved")),
      onError: () => setSaveError(t("notifications.saveFailed")),
    });
  }

  return (
    <div className="space-y-3">
      <Card>
        <CardContent>
          <QueryState
            data={notificationsQuery.data}
            isLoading={
              notificationsQuery.isLoading ||
              (!notificationsQuery.error &&
                notificationsQuery.data !== undefined &&
                loadedData !== notificationsQuery.data)
            }
            error={notificationsQuery.error}
            errorMessage={t("notifications.loadFailed")}
            onRetry={() => void notificationsQuery.refetch()}
          >
            {() => (
              <form onSubmit={handleSubmit} className="space-y-3">
                {items.map((item, i) => (
                  <div
                    key={i}
                    className="grid gap-2 sm:grid-cols-[1fr_2fr_auto]"
                  >
                    <Input
                      required
                      placeholder={t("notifications.channelPlaceholder")}
                      value={item.channel}
                      onChange={(e) =>
                        setItems(
                          items.map((it, idx) =>
                            idx === i ? { ...it, channel: e.target.value } : it,
                          ),
                        )
                      }
                    />
                    <Input
                      required
                      placeholder={t("notifications.targetPlaceholder")}
                      value={item.target}
                      onChange={(e) =>
                        setItems(
                          items.map((it, idx) =>
                            idx === i ? { ...it, target: e.target.value } : it,
                          ),
                        )
                      }
                    />
                    <Button
                      type="button"
                      variant="destructive-outline"
                      size="icon"
                      aria-label={t("common.delete")}
                      title={t("common.delete")}

                      onClick={() =>
                        setItems(items.filter((_, idx) => idx !== i))
                      }
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                ))}
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"

                  onClick={() =>
                    setItems([
                      ...items,
                      {
                        channel: "in_app",
                        target: "dashboard",
                        enabled: true,
                        aggregation_window_secs: 0,
                        quiet_start_min: 0,
                        quiet_end_min: 0,
                        quiet_action: "hold",
                        quiet_bypass_statuses: [],
                      },
                    ])
                  }
                >
                  <Plus className="h-4 w-4" /> {t("notifications.add")}
                </Button>
                {saveError && (
                  <Notice role="alert" tone="danger" appearance="inline">
                    {saveError}
                  </Notice>
                )}
                <div className="flex gap-2">
                  <Button
                    type="submit"

                    disabled={save.isPending}
                  >
                    {t("common.save")}
                  </Button>
                </div>
              </form>
            )}
          </QueryState>
        </CardContent>
      </Card>
      <QueryState
        data={eventsQuery.data}
        isLoading={eventsQuery.isLoading}
        error={eventsQuery.error}
        errorMessage={t("notifications.eventsFailed")}
        onRetry={() => void eventsQuery.refetch()}
      >
        {(events) =>
          events.length === 0 ? (
            <EmptyState message={t("notifications.noEvents")} />
          ) : (
            <Card className="overflow-hidden">
              <ResourceList asChild>
                <div className="md:hidden">
                  {events.map((event) => (
                    <ResourceListRow asChild key={event.id}>
                      <div className="space-y-2">
                        <ResourceListTitle asChild className="break-words">
                          <p >
                          {event.message}
                        </p>
                        </ResourceListTitle>
                        <ResourceListDescription asChild technical>
                          <p className="break-all">
                          {event.event_type} · {event.pipeline_id.slice(0, 8)}
                        </p>
                        </ResourceListDescription>
                        <ResourceListDescription asChild technical>
                          <p className="break-all">
                          {event.channel} / {event.target}
                        </p>
                        </ResourceListDescription>
                        <div className="flex flex-wrap items-center gap-2 text-xs text-text-secondary">
                          <NotificationStatusBadge event={event} />
                          <time dateTime={event.created_at}>
                            {new Date(event.created_at).toLocaleString()}
                          </time>
                        </div>
                      </div>
                    </ResourceListRow>
                  ))}
                </div>
              </ResourceList>
              <div className="hidden overflow-x-auto md:block">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>{t("notifications.event")}</TableHead>
                      <TableHead>{t("notifications.channel")}</TableHead>
                      <TableHead>{t("notifications.delivery")}</TableHead>
                      <TableHead>{t("notifications.created")}</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {events.map((event) => (
                      <TableRow key={event.id}>
                        <TableCell>
                          <div className="max-w-xl">
                            <p className="text-sm font-medium">
                              {event.message}
                            </p>
                            <p className="mt-1 font-mono text-xs text-text-muted">
                              {event.event_type} -{" "}
                              {event.pipeline_id.slice(0, 8)}
                            </p>
                          </div>
                        </TableCell>
                        <TableCell className="break-all font-mono text-xs">
                          {event.channel} / {event.target}
                        </TableCell>
                        <TableCell>
                          <NotificationStatusBadge event={event} />
                        </TableCell>
                        <TableCell className="text-xs text-text-muted">
                          {new Date(event.created_at).toLocaleString()}
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </div>
            </Card>
          )
        }
      </QueryState>
    </div>
  );
}

function NotificationStatusBadge({ event }: { event: NotificationEvent }) {
  const { t } = useTranslation();
  return (
    <Badge
      tone={
        event.last_error ? "danger" : event.delivered_at ? "success" : "warning"
      }
    >
      {event.last_error
        ? t("notifications.failed")
        : event.delivered_at
          ? t("notifications.delivered")
          : t("notifications.pending")}
    </Badge>
  );
}
