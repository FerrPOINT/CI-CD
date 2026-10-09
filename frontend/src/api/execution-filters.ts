// Date-only filters describe the user's local calendar day; ISO bookmarks stay exact.
export function executionBoundary(value: string, end: boolean): string {
  const dateOnly = /^\d{4}-\d{2}-\d{2}$/.test(value)
  const parsed = new Date(dateOnly ? `${value}T${end ? '23:59:59.999' : '00:00:00'}` : value)
  return Number.isNaN(parsed.getTime()) ? value : parsed.toISOString()
}
