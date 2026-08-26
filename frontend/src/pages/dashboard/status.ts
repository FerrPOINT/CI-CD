export type Status = 'queued' | 'running' | 'success' | 'failed' | 'canceled'

export function statusLabel(status: Status): string {
  return status.charAt(0).toUpperCase() + status.slice(1)
}

export const statusColors: Record<Status, string> = {
  queued: 'text-text-muted',
  running: 'text-warning',
  success: 'text-success',
  failed: 'text-danger',
  canceled: 'text-text-muted',
}

export const statusDotColors: Record<Status, string> = {
  queued: 'bg-text-muted',
  running: 'bg-warning',
  success: 'bg-success',
  failed: 'bg-danger',
  canceled: 'bg-text-muted',
}