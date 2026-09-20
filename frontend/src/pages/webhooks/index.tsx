import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams, useSearchParams } from 'react-router'
import { useWebhooks, useCreateWebhook, useDeleteWebhook, useOutboxDeliveries, useOutboxDelivery, useRequeueOutboxDelivery, useNotifications, useNotificationEvents, useSaveNotifications } from '@/api/hooks'
import { Card } from '@sdlc/ui/ui'
import { Button } from '@sdlc/ui/ui'
import { Input } from '@sdlc/ui/ui'
import { Label } from '@sdlc/ui/ui'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@sdlc/ui/ui'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@sdlc/ui/ui'
import { RotateCcw, Webhook, Plus, Trash2, X } from 'lucide-react'
import { toast } from 'sonner'
import { ConfirmDialog } from '@/shared/ui/confirm-dialog'
import { QueryState } from '@/shared/ui/query-state'
import type { NotificationEvent, OutboxDelivery, Webhook as WebhookType } from '@/api/types'

export function WebhooksPage() {
  const { t } = useTranslation()
  const { projectId } = useParams()
  const [searchParams, setSearchParams] = useSearchParams()
  const viewParam = searchParams.get('view')
  const view = viewParam === 'deliveries' || viewParam === 'notifications' ? viewParam : 'webhooks'
  const webhooksQuery = useWebhooks(projectId)
  const createWebhook = useCreateWebhook(projectId)
  const deleteWebhook = useDeleteWebhook()
  const [showForm, setShowForm] = useState(false)
  const [pendingDelete, setPendingDelete] = useState<WebhookType | null>(null)
  const [deleteError, setDeleteError] = useState<string | null>(null)
  const [form, setForm] = useState({ url: '', events: 'pipeline.started, pipeline.finished' })

  function changeView(next: string) {
    const params = new URLSearchParams(searchParams)
    if (next === 'webhooks') params.delete('view')
    else params.set('view', next)
    setSearchParams(params, { replace: true })
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    createWebhook.mutate(
      { url: form.url, events: form.events.split(',').map(s => s.trim()).filter(Boolean) },
      {
        onSuccess: () => { setShowForm(false); setForm({ url: '', events: 'pipeline.started, pipeline.finished' }); toast.success(t('webhooks.created')) },
        onError: (err) => toast.error(err.message),
      },
    )
  }

  function handleDelete(w: WebhookType) {
    setDeleteError(null)
    setPendingDelete(w)
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2">
        <Webhook className="h-5 w-5 text-accent" />
        <h1 className="text-2xl font-bold">{t('webhooks.pageTitle')}</h1>
      </div>
      <Tabs value={view} onValueChange={changeView}>
        <TabsList className="grid h-auto w-full max-w-xl grid-cols-3">
          <TabsTrigger value="webhooks" className="min-h-11 min-w-0 whitespace-normal px-2 text-xs sm:text-sm">{t('webhooks.title')}</TabsTrigger>
          <TabsTrigger value="deliveries" className="min-h-11 min-w-0 whitespace-normal px-2 text-xs sm:text-sm">{t('deliveries.title')}</TabsTrigger>
          <TabsTrigger value="notifications" className="min-h-11 min-w-0 whitespace-normal px-2 text-xs sm:text-sm">{t('notifications.title')}</TabsTrigger>
        </TabsList>

        <TabsContent value="webhooks" className="mt-4 space-y-4">
          <div className="flex justify-end">
            <Button type="button" size="sm" className="min-h-10 sm:min-h-10" onClick={() => setShowForm(v => !v)}>
              <Plus className="h-4 w-4" />
              {t('webhooks.create')}
            </Button>
          </div>
          {showForm && (
            <Card className="p-4">
              <form onSubmit={handleSubmit} className="grid gap-3">
                <div className="space-y-1.5">
                  <Label htmlFor="wh-url">{t('webhooks.url')}</Label>
                  <Input id="wh-url" required className="h-10" placeholder="https://example.com/hook" value={form.url} onChange={e => setForm({ ...form, url: e.target.value })} />
                </div>
                <div className="space-y-1.5">
                  <Label htmlFor="wh-events">{t('webhooks.events')}</Label>
                  <Input id="wh-events" className="h-10" placeholder="pipeline.started, pipeline.finished" value={form.events} onChange={e => setForm({ ...form, events: e.target.value })} />
                </div>
                <div className="flex flex-wrap gap-2">
                  <Button type="submit" className="min-h-10" disabled={createWebhook.isPending}>{t('webhooks.create')}</Button>
                  <Button type="button" variant="ghost" className="min-h-10" onClick={() => setShowForm(false)}>{t('common.cancel')}</Button>
                </div>
              </form>
            </Card>
          )}

          <QueryState
            data={webhooksQuery.data}
            isLoading={webhooksQuery.isLoading}
            error={webhooksQuery.error}
            errorMessage={t('webhooks.loadFailed')}
            onRetry={() => void webhooksQuery.refetch()}
            isEmpty={list => list.length === 0}
            empty={{ title: t('webhooks.empty') }}
          >
            {webhooks => <Card className="overflow-hidden">
              <div className="divide-y divide-border md:hidden">
                {webhooks.map(w => <div key={w.id} className="flex items-start gap-2 p-3">
                  <div className="min-w-0 flex-1 space-y-2">
                    <p className="break-all font-mono text-xs">{w.url}</p>
                    <p className="text-xs text-text-secondary">{w.events.length ? w.events.join(', ') : '—'}</p>
                    <WebhookStatusBadge enabled={w.enabled} />
                  </div>
                  <Button type="button" size="icon" variant="ghost" aria-label={`${t('common.delete')} ${w.url}`} title={t('common.delete')} className="h-10 w-10 shrink-0 text-danger hover:text-danger" onClick={() => handleDelete(w)}>
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>)}
              </div>
              <div className="hidden overflow-x-auto md:block">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>{t('webhooks.url')}</TableHead>
                      <TableHead>{t('webhooks.events')}</TableHead>
                      <TableHead>{t('webhooks.enabled')}</TableHead>
                      <TableHead className="w-14"><span className="sr-only">{t('common.actions')}</span></TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {webhooks.map(w => <TableRow key={w.id}>
                      <TableCell className="max-w-xl break-all font-mono text-xs">{w.url}</TableCell>
                      <TableCell className="text-xs text-text-secondary">{w.events.length ? w.events.join(', ') : '—'}</TableCell>
                      <TableCell><WebhookStatusBadge enabled={w.enabled} /></TableCell>
                      <TableCell>
                        <Button type="button" size="icon" variant="ghost" aria-label={`${t('common.delete')} ${w.url}`} title={t('common.delete')} className="h-10 w-10 text-danger hover:text-danger" onClick={() => handleDelete(w)}>
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </TableCell>
                    </TableRow>)}
                  </TableBody>
                </Table>
              </div>
            </Card>}
          </QueryState>
        </TabsContent>
        <TabsContent value="deliveries" className="mt-4"><DeliveryHistorySection /></TabsContent>
        <TabsContent value="notifications" className="mt-4"><NotificationsSection /></TabsContent>
      </Tabs>

      <ConfirmDialog
        open={pendingDelete !== null}
        title={t('webhooks.deleteConfirm')}
        description={deleteError ?? pendingDelete?.url}
        pending={deleteWebhook.isPending}
        closeOnConfirm={false}
        onCancel={() => { setPendingDelete(null); setDeleteError(null) }}
        onConfirm={() => {
          if (pendingDelete) deleteWebhook.mutate(pendingDelete.id, {
            onSuccess: () => { setPendingDelete(null); toast.success(t('webhooks.deleted')) },
            onError: () => setDeleteError(t('webhooks.deleteFailed')),
          })
        }}
      />
    </div>
  )
}

function WebhookStatusBadge({ enabled }: { enabled: boolean }) {
  const { t } = useTranslation()
  return <span className={`inline-block rounded-full px-2 py-0.5 text-xs ${enabled ? 'bg-emerald-500/15 text-text-primary' : 'bg-surface-raised text-text-secondary'}`}>
    {enabled ? t('webhooks.on') : t('webhooks.off')}
  </span>
}

function DeliveryHistorySection() {
  const { t } = useTranslation()
  const { projectId } = useParams()
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const deliveriesQuery = useOutboxDeliveries(projectId, { limit: 20 })
  const detail = useOutboxDelivery(selectedId)
  const requeue = useRequeueOutboxDelivery()
  const selectedDelivery = detail.data?.delivery

  function handleRequeue(delivery: OutboxDelivery) {
    requeue.mutate(delivery.id, {
      onSuccess: (result) => {
        setSelectedId(result.id)
        toast.success(t('deliveries.requeued'))
      },
      onError: (err) => toast.error(err.message),
    })
  }

  return (
    <div className="space-y-4">
      <QueryState
        data={deliveriesQuery.data}
        isLoading={deliveriesQuery.isLoading}
        error={deliveriesQuery.error}
        errorMessage={t('deliveries.loadFailed')}
        onRetry={() => void deliveriesQuery.refetch()}
        isEmpty={list => list.length === 0}
        empty={{ title: t('deliveries.empty') }}
      >
        {deliveries => <Card className="overflow-hidden">
          <div className="divide-y divide-border md:hidden">
            {deliveries.map(delivery => <div key={delivery.id} className="space-y-2 p-3">
              <DeliveryOpenButton delivery={delivery} selected={selectedId === delivery.id} onClick={() => setSelectedId(current => current === delivery.id ? null : delivery.id)} />
              <p className="break-all font-mono text-xs text-text-secondary">{delivery.channel} / {delivery.destination}</p>
              <div className="flex flex-wrap items-center gap-3 text-xs text-text-secondary">
                <DeliveryStatusBadge status={delivery.status} />
                <span>{t('deliveries.attempts')}: {delivery.attempts}</span>
                <time dateTime={delivery.created_at}>{new Date(delivery.created_at).toLocaleString()}</time>
              </div>
              {delivery.status === 'failed' && <Button type="button" size="sm" variant="outline" className="min-h-10 sm:min-h-10" disabled={requeue.isPending} onClick={() => handleRequeue(delivery)}>
                <RotateCcw className="h-4 w-4" /> {t('deliveries.requeue')}
              </Button>}
            </div>)}
          </div>
          <div className="hidden overflow-x-auto md:block">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t('deliveries.event')}</TableHead>
                  <TableHead>{t('deliveries.destination')}</TableHead>
                  <TableHead>{t('deliveries.status')}</TableHead>
                  <TableHead>{t('deliveries.attempts')}</TableHead>
                  <TableHead>{t('deliveries.created')}</TableHead>
                  <TableHead className="w-28"><span className="sr-only">{t('common.actions')}</span></TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {deliveries.map(delivery => <TableRow key={delivery.id}>
                  <TableCell className="max-w-xl">
                    <DeliveryOpenButton delivery={delivery} selected={selectedId === delivery.id} onClick={() => setSelectedId(current => current === delivery.id ? null : delivery.id)} />
                  </TableCell>
                  <TableCell className="max-w-md break-all font-mono text-xs">{delivery.channel} / {delivery.destination}</TableCell>
                  <TableCell><DeliveryStatusBadge status={delivery.status} /></TableCell>
                  <TableCell className="font-mono text-xs">{delivery.attempts}</TableCell>
                  <TableCell className="text-xs text-text-secondary"><time dateTime={delivery.created_at}>{new Date(delivery.created_at).toLocaleString()}</time></TableCell>
                  <TableCell>
                    {delivery.status === 'failed' && <Button type="button" size="sm" variant="ghost" className="min-h-10 sm:min-h-10" disabled={requeue.isPending} onClick={() => handleRequeue(delivery)}>
                      <RotateCcw className="h-4 w-4" /> {t('deliveries.requeue')}
                    </Button>}
                  </TableCell>
                </TableRow>)}
              </TableBody>
            </Table>
          </div>
        </Card>}
      </QueryState>
      {selectedId && (
        <Card id="delivery-details" className="p-4">
          <div className="mb-3 flex items-center justify-between gap-3">
            <h3 className="text-sm font-semibold">{t('deliveries.details')}</h3>
            <div className="flex items-center gap-2">
              {selectedDelivery?.status === 'failed' && <Button type="button" size="sm" className="min-h-10 sm:min-h-10" disabled={requeue.isPending} onClick={() => handleRequeue(selectedDelivery)}>
                <RotateCcw className="h-4 w-4" /> {t('deliveries.requeue')}
              </Button>}
              <Button type="button" size="icon" variant="ghost" className="h-10 w-10" aria-label={t('common.close')} title={t('common.close')} onClick={() => setSelectedId(null)}>
                <X className="h-4 w-4" />
              </Button>
            </div>
          </div>
          <QueryState data={detail.data} isLoading={detail.isLoading} error={detail.error} errorMessage={t('deliveries.detailFailed')} onRetry={() => void detail.refetch()}>
            {data => data.attempts.length === 0
              ? <p className="text-sm text-text-secondary">{t('deliveries.noAttempts')}</p>
              : <>
                <div className="divide-y divide-border md:hidden">
                  {data.attempts.map(attempt => <div key={attempt.id} className="space-y-1 py-2 text-xs">
                    <div className="flex items-center justify-between gap-2"><span className="font-medium">#{attempt.attempt_number}</span><DeliveryStatusBadge status={attempt.outcome} /></div>
                    <p className="text-text-secondary">{t('deliveries.httpStatus')}: {attempt.http_status ?? '—'} · {attempt.duration_ms} ms</p>
                    {attempt.error_message && <p className="break-all text-danger">{attempt.error_message}</p>}
                  </div>)}
                </div>
                <div className="hidden overflow-x-auto md:block">
                  <Table>
                    <TableHeader><TableRow>
                      <TableHead>{t('deliveries.attempt')}</TableHead>
                      <TableHead>{t('deliveries.outcome')}</TableHead>
                      <TableHead>{t('deliveries.httpStatus')}</TableHead>
                      <TableHead>{t('deliveries.duration')}</TableHead>
                      <TableHead>{t('deliveries.error')}</TableHead>
                    </TableRow></TableHeader>
                    <TableBody>{data.attempts.map(attempt => <TableRow key={attempt.id}>
                      <TableCell className="font-mono text-xs">{attempt.attempt_number}</TableCell>
                      <TableCell><DeliveryStatusBadge status={attempt.outcome} /></TableCell>
                      <TableCell className="font-mono text-xs">{attempt.http_status ?? '—'}</TableCell>
                      <TableCell className="font-mono text-xs">{attempt.duration_ms} ms</TableCell>
                      <TableCell className="max-w-lg break-all text-xs text-text-secondary">{attempt.error_message ?? '—'}</TableCell>
                    </TableRow>)}</TableBody>
                  </Table>
                </div>
              </>}
          </QueryState>
        </Card>
      )}
    </div>
  )
}

function DeliveryOpenButton({ delivery, selected, onClick }: { delivery: OutboxDelivery; selected: boolean; onClick: () => void }) {
  const { t } = useTranslation()
  return <button type="button" className="min-h-10 w-full text-left hover:text-accent focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent" aria-label={t('deliveries.openDetails', { event: delivery.event_type, id: delivery.aggregate_id.slice(0, 8) })} aria-expanded={selected} aria-controls={selected ? 'delivery-details' : undefined} onClick={onClick}>
    <span className="block text-sm font-medium">{delivery.event_type}</span>
    <span className="mt-1 block font-mono text-xs text-text-secondary">{delivery.aggregate_type} · {delivery.aggregate_id.slice(0, 8)} · gen {delivery.generation}</span>
    {delivery.last_error && <span className="mt-1 block text-xs text-danger">{delivery.last_error}</span>}
  </button>
}

function DeliveryStatusBadge({ status }: { status: string }) {
  const { t } = useTranslation()
  const tone = status === 'delivered'
    ? 'bg-emerald-500/15 text-text-primary'
    : status === 'failed'
      ? 'bg-red-500/15 text-text-primary'
      : status === 'retry_scheduled'
        ? 'bg-amber-500/15 text-text-primary'
        : 'bg-surface-raised text-text-muted'
  const labelKey = status === 'retry_scheduled' ? 'deliveries.retryScheduled' : `deliveries.${status}`
  return <span className={`rounded-full px-2 py-0.5 text-xs ${tone}`}>{t(labelKey)}</span>
}

function NotificationsSection() {
  const { t } = useTranslation()
  const { projectId } = useParams()
  const notificationsQuery = useNotifications(projectId)
  const isLoading = notificationsQuery.isLoading
  const eventsQuery = useNotificationEvents(projectId)
  const save = useSaveNotifications(projectId)
  const [items, setItems] = useState<{ channel: string; target: string }[]>([])

  useEffect(() => {
    if (notificationsQuery.data) {
      setItems(notificationsQuery.data.map(n => ({ channel: n.channel, target: n.target })))
    }
  }, [notificationsQuery.data])

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    save.mutate(items, {
      onSuccess: () => toast.success(t('notifications.saved')),
      onError: (err) => toast.error(err.message),
    })
  }

  return (
    <div className="space-y-3">
      <Card className="p-4">
        <form onSubmit={handleSubmit} className="space-y-3">
          {isLoading ? (
            <p className="text-sm text-text-muted">{t('common.loading')}</p>
          ) : (
            <>
              {items.map((item, i) => (
                <div key={i} className="grid gap-2 sm:grid-cols-[1fr_2fr_auto]">
                  <Input required placeholder={t('notifications.channelPlaceholder')} value={item.channel} onChange={e => setItems(items.map((it, idx) => idx === i ? { ...it, channel: e.target.value } : it))} />
                  <Input required placeholder={t('notifications.targetPlaceholder')} value={item.target} onChange={e => setItems(items.map((it, idx) => idx === i ? { ...it, target: e.target.value } : it))} />
                  <Button type="button" variant="ghost" size="icon" aria-label={t('common.delete')} title={t('common.delete')} className="h-10 w-10 sm:h-10 sm:w-10 text-danger" onClick={() => setItems(items.filter((_, idx) => idx !== i))}>
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
              ))}
              <Button type="button" variant="ghost" size="sm" className="min-h-10 sm:min-h-10" onClick={() => setItems([...items, { channel: 'in_app', target: 'dashboard' }])}>
                <Plus className="h-4 w-4" /> {t('notifications.add')}
              </Button>
              <div className="flex gap-2">
                <Button type="submit" className="min-h-10 sm:min-h-10" disabled={save.isPending}>{t('common.save')}</Button>
              </div>
            </>
          )}
        </form>
      </Card>
      <QueryState data={eventsQuery.data} isLoading={eventsQuery.isLoading} error={eventsQuery.error} errorMessage={t('notifications.eventsFailed')} onRetry={() => void eventsQuery.refetch()}>
        {events => events.length === 0
          ? <p className="text-sm text-text-secondary">{t('notifications.noEvents')}</p>
          : <Card className="overflow-hidden">
              <div className="divide-y divide-border md:hidden">
                {events.map(event => <div key={event.id} className="space-y-2 p-3">
                  <p className="break-words text-sm font-medium">{event.message}</p>
                  <p className="break-all font-mono text-xs text-text-secondary">{event.event_type} · {event.pipeline_id.slice(0, 8)}</p>
                  <p className="break-all font-mono text-xs text-text-secondary">{event.channel} / {event.target}</p>
                  <div className="flex flex-wrap items-center gap-2 text-xs text-text-secondary">
                    <NotificationStatusBadge event={event} />
                    <time dateTime={event.created_at}>{new Date(event.created_at).toLocaleString()}</time>
                  </div>
                </div>)}
              </div>
              <div className="hidden overflow-x-auto md:block">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>{t('notifications.event')}</TableHead>
                      <TableHead>{t('notifications.channel')}</TableHead>
                      <TableHead>{t('notifications.delivery')}</TableHead>
                      <TableHead>{t('notifications.created')}</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {events.map(event => <TableRow key={event.id}>
                      <TableCell>
                        <div className="max-w-xl">
                          <p className="text-sm font-medium">{event.message}</p>
                          <p className="mt-1 font-mono text-xs text-text-muted">{event.event_type} - {event.pipeline_id.slice(0, 8)}</p>
                        </div>
                      </TableCell>
                      <TableCell className="break-all font-mono text-xs">{event.channel} / {event.target}</TableCell>
                      <TableCell><NotificationStatusBadge event={event} /></TableCell>
                      <TableCell className="text-xs text-text-muted">{new Date(event.created_at).toLocaleString()}</TableCell>
                    </TableRow>)}
                  </TableBody>
                </Table>
              </div>
            </Card>}
      </QueryState>
    </div>
  )
}

function NotificationStatusBadge({ event }: { event: NotificationEvent }) {
  const { t } = useTranslation()
  return <span className={`inline-block rounded-full px-2 py-0.5 text-xs text-text-primary ${event.last_error ? 'bg-red-500/15' : event.delivered_at ? 'bg-emerald-500/15' : 'bg-amber-500/15'}`}>
    {event.last_error ? t('notifications.failed') : event.delivered_at ? t('notifications.delivered') : t('notifications.pending')}
  </span>
}
