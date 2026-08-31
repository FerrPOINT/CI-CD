import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'
import { useWebhooks, useCreateWebhook, useDeleteWebhook, useOutboxDeliveries, useOutboxDelivery, useRequeueOutboxDelivery, useNotifications, useNotificationEvents, useSaveNotifications } from '@/api/hooks'
import { Card } from '@/shared/ui/card'
import { Button } from '@/shared/ui/button'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'
import { CapabilityCallout } from '@/shared/ui/capability-callout'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/ui/table'
import { Activity, RotateCcw, Webhook, Plus, Trash2, Bell } from 'lucide-react'
import { toast } from 'sonner'
import { ConfirmDialog } from '@/shared/ui/confirm-dialog'
import type { OutboxDelivery, Webhook as WebhookType } from '@/api/types'

export function WebhooksPage() {
  const { t } = useTranslation()
  const { projectId } = useParams()
  const { data: webhooks = [], isLoading } = useWebhooks(projectId)
  const createWebhook = useCreateWebhook(projectId)
  const deleteWebhook = useDeleteWebhook()
  const [showForm, setShowForm] = useState(false)
  const [pendingDelete, setPendingDelete] = useState<WebhookType | null>(null)
  const [form, setForm] = useState({ url: '', events: 'pipeline.started, pipeline.finished' })

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
    setPendingDelete(w)
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Webhook className="h-5 w-5 text-accent" />
          <h1 className="text-2xl font-bold">{t('webhooks.title')}</h1>
        </div>
        <Button size="sm" onClick={() => setShowForm(v => !v)}>
          <Plus className="h-4 w-4" />
          {t('webhooks.create')}
        </Button>
      </div>

      <CapabilityCallout
        tone="mvp"
        title={t('webhooks.capabilityTitle')}
        label={t('capability.currentMvp')}
        description={t('webhooks.capabilityDescription')}
      />

      {showForm && (
        <Card className="p-4">
          <form onSubmit={handleSubmit} className="grid gap-3">
            <div className="space-y-1.5">
              <Label htmlFor="wh-url">{t('webhooks.url')}</Label>
              <Input id="wh-url" required placeholder="https://example.com/hook" value={form.url} onChange={e => setForm({ ...form, url: e.target.value })} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="wh-events">{t('webhooks.events')}</Label>
              <Input id="wh-events" placeholder="pipeline.started, pipeline.finished" value={form.events} onChange={e => setForm({ ...form, events: e.target.value })} />
            </div>
            <div className="flex gap-2">
              <Button type="submit" disabled={createWebhook.isPending}>{t('webhooks.create')}</Button>
              <Button type="button" variant="ghost" onClick={() => setShowForm(false)}>{t('common.cancel')}</Button>
            </div>
          </form>
        </Card>
      )}

      {isLoading ? (
        <p className="text-sm text-text-muted">{t('common.loading')}</p>
      ) : webhooks.length === 0 ? (
        <Card className="p-8 text-center"><p className="text-text-muted">{t('webhooks.empty')}</p></Card>
      ) : (
        <Card>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t('webhooks.url')}</TableHead>
                <TableHead>{t('webhooks.events')}</TableHead>
                <TableHead>{t('webhooks.enabled')}</TableHead>
                <TableHead className="w-20"></TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {webhooks.map(w => (
                <TableRow key={w.id}>
                  <TableCell className="font-mono text-xs">{w.url}</TableCell>
                  <TableCell className="text-xs text-text-muted">{w.events.length ? w.events.join(', ') : '—'}</TableCell>
                  <TableCell>
                    <span className={`rounded-full px-2 py-0.5 text-xs ${w.enabled ? 'bg-emerald-500/15 text-emerald-500' : 'bg-surface-raised text-text-muted'}`}>
                      {w.enabled ? t('webhooks.on') : t('webhooks.off')}
                    </span>
                  </TableCell>
                  <TableCell>
                    <Button size="sm" variant="ghost" className="h-7 gap-1 px-2 text-xs text-danger hover:text-danger" onClick={() => handleDelete(w)}>
                      <Trash2 className="h-3 w-3" />
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </Card>
      )}

      <ConfirmDialog
        open={pendingDelete !== null}
        title={pendingDelete ? `${t('webhooks.deleteConfirm')} "${pendingDelete.url}"?` : ''}
        onCancel={() => setPendingDelete(null)}
        onConfirm={() => {
          if (pendingDelete) deleteWebhook.mutate(pendingDelete.id, { onSuccess: () => toast.success(t('webhooks.deleted')), onError: (err: Error) => toast.error(err.message) })
          setPendingDelete(null)
        }}
      />

      <DeliveryHistorySection />
      <NotificationsSection />
    </div>
  )
}

function DeliveryHistorySection() {
  const { t } = useTranslation()
  const { projectId } = useParams()
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const { data: deliveries = [], isLoading } = useOutboxDeliveries(projectId, { limit: 20 })
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
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <Activity className="h-5 w-5 text-accent" />
        <h2 className="text-lg font-semibold">{t('deliveries.title')}</h2>
      </div>
      <CapabilityCallout
        tone="mvp"
        title={t('deliveries.capabilityTitle')}
        label={t('capability.currentMvp')}
        description={t('deliveries.capabilityDescription')}
      />
      <Card>
        <div className="overflow-x-auto">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t('deliveries.event')}</TableHead>
                <TableHead>{t('deliveries.destination')}</TableHead>
                <TableHead>{t('deliveries.status')}</TableHead>
                <TableHead>{t('deliveries.attempts')}</TableHead>
                <TableHead>{t('deliveries.created')}</TableHead>
                <TableHead className="w-28"></TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {isLoading ? (
                <TableRow><TableCell colSpan={6} className="text-sm text-text-muted">{t('common.loading')}</TableCell></TableRow>
              ) : deliveries.length === 0 ? (
                <TableRow><TableCell colSpan={6} className="text-sm text-text-muted">{t('deliveries.empty')}</TableCell></TableRow>
              ) : deliveries.map(delivery => (
                <TableRow key={delivery.id} className="cursor-pointer" onClick={() => setSelectedId(delivery.id)}>
                  <TableCell>
                    <div className="max-w-xl">
                      <p className="text-sm font-medium">{delivery.event_type}</p>
                      <p className="mt-1 font-mono text-xs text-text-muted">{delivery.aggregate_type} - {delivery.aggregate_id.slice(0, 8)} - gen {delivery.generation}</p>
                      {delivery.last_error && <p className="mt-1 text-xs text-danger">{delivery.last_error}</p>}
                    </div>
                  </TableCell>
                  <TableCell className="max-w-md break-all font-mono text-xs">{delivery.channel} / {delivery.destination}</TableCell>
                  <TableCell><DeliveryStatusBadge status={delivery.status} /></TableCell>
                  <TableCell className="font-mono text-xs">{delivery.attempts}</TableCell>
                  <TableCell className="text-xs text-text-muted">{new Date(delivery.created_at).toLocaleString()}</TableCell>
                  <TableCell>
                    {delivery.status === 'failed' && (
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        className="h-8 gap-1 px-2 text-xs"
                        disabled={requeue.isPending}
                        onClick={(event) => {
                          event.stopPropagation()
                          handleRequeue(delivery)
                        }}
                      >
                        <RotateCcw className="h-3.5 w-3.5" />
                        {t('deliveries.requeue')}
                      </Button>
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      </Card>
      {selectedId && (
        <Card className="p-4">
          <div className="mb-3 flex items-center justify-between gap-3">
            <h3 className="text-sm font-semibold">{t('deliveries.details')}</h3>
            {selectedDelivery?.status === 'failed' && (
              <Button type="button" size="sm" disabled={requeue.isPending} onClick={() => handleRequeue(selectedDelivery)}>
                <RotateCcw className="h-4 w-4" />
                {t('deliveries.requeue')}
              </Button>
            )}
          </div>
          {detail.isLoading ? (
            <p className="text-sm text-text-muted">{t('common.loading')}</p>
          ) : detail.data ? (
            <div className="overflow-x-auto">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>{t('deliveries.attempt')}</TableHead>
                    <TableHead>{t('deliveries.outcome')}</TableHead>
                    <TableHead>{t('deliveries.httpStatus')}</TableHead>
                    <TableHead>{t('deliveries.duration')}</TableHead>
                    <TableHead>{t('deliveries.error')}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {detail.data.attempts.length === 0 ? (
                    <TableRow><TableCell colSpan={5} className="text-sm text-text-muted">{t('deliveries.noAttempts')}</TableCell></TableRow>
                  ) : detail.data.attempts.map(attempt => (
                    <TableRow key={attempt.id}>
                      <TableCell className="font-mono text-xs">{attempt.attempt_number}</TableCell>
                      <TableCell><DeliveryStatusBadge status={attempt.outcome} /></TableCell>
                      <TableCell className="font-mono text-xs">{attempt.http_status ?? '—'}</TableCell>
                      <TableCell className="font-mono text-xs">{attempt.duration_ms} ms</TableCell>
                      <TableCell className="max-w-lg break-all text-xs text-text-muted">{attempt.error_message ?? '—'}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          ) : (
            <p className="text-sm text-text-muted">{t('deliveries.noAttempts')}</p>
          )}
        </Card>
      )}
    </div>
  )
}

function DeliveryStatusBadge({ status }: { status: string }) {
  const { t } = useTranslation()
  const tone = status === 'delivered'
    ? 'bg-emerald-500/15 text-emerald-500'
    : status === 'failed'
      ? 'bg-red-500/15 text-red-500'
      : status === 'retry_scheduled'
        ? 'bg-amber-500/15 text-amber-500'
        : 'bg-surface-raised text-text-muted'
  const labelKey = status === 'retry_scheduled' ? 'deliveries.retryScheduled' : `deliveries.${status}`
  return <span className={`rounded-full px-2 py-0.5 text-xs ${tone}`}>{t(labelKey)}</span>
}

function NotificationsSection() {
  const { t } = useTranslation()
  const { projectId } = useParams()
  const notificationsQuery = useNotifications(projectId)
  const isLoading = notificationsQuery.isLoading
  const { data: events = [], isLoading: eventsLoading } = useNotificationEvents(projectId)
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
      <div className="flex items-center gap-2">
        <Bell className="h-5 w-5 text-accent" />
        <h2 className="text-lg font-semibold">{t('notifications.title')}</h2>
      </div>
      <CapabilityCallout
        tone="mvp"
        title={t('notifications.capabilityTitle')}
        label={t('capability.currentMvp')}
        description={t('notifications.capabilityDescription')}
      />
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
                  <Button type="button" variant="ghost" size="icon" className="h-9 w-9 text-danger" onClick={() => setItems(items.filter((_, idx) => idx !== i))}>
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
              ))}
              <Button type="button" variant="ghost" size="sm" onClick={() => setItems([...items, { channel: 'in_app', target: 'dashboard' }])}>
                <Plus className="h-4 w-4" /> {t('notifications.add')}
              </Button>
              <div className="flex gap-2">
                <Button type="submit" disabled={save.isPending}>{t('common.save')}</Button>
              </div>
            </>
          )}
        </form>
      </Card>
      <Card>
        <div className="overflow-x-auto">
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
              {eventsLoading ? (
                <TableRow><TableCell colSpan={4} className="text-sm text-text-muted">{t('common.loading')}</TableCell></TableRow>
              ) : events.length === 0 ? (
                <TableRow><TableCell colSpan={4} className="text-sm text-text-muted">{t('notifications.noEvents')}</TableCell></TableRow>
              ) : events.map(event => (
                <TableRow key={event.id}>
                  <TableCell>
                    <div className="max-w-xl">
                      <p className="text-sm font-medium">{event.message}</p>
                      <p className="mt-1 font-mono text-xs text-text-muted">{event.event_type} - {event.pipeline_id.slice(0, 8)}</p>
                    </div>
                  </TableCell>
                  <TableCell className="break-all font-mono text-xs">{event.channel} / {event.target}</TableCell>
                  <TableCell>
                    <span className={`rounded-full px-2 py-0.5 text-xs ${event.last_error ? 'bg-red-500/15 text-red-500' : event.delivered_at ? 'bg-emerald-500/15 text-emerald-500' : 'bg-amber-500/15 text-amber-500'}`}>
                      {event.last_error ? t('notifications.failed') : event.delivered_at ? t('notifications.delivered') : t('notifications.pending')}
                    </span>
                  </TableCell>
                  <TableCell className="text-xs text-text-muted">{new Date(event.created_at).toLocaleString()}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      </Card>
    </div>
  )
}
