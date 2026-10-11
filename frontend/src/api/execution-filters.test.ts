import { describe, expect, it } from 'vitest'
import { executionBoundary } from './execution-filters'

describe('execution period bookmarks', () => {
  it('retains the exact ISO instant and lets the API reject malformed input', () => {
    expect(executionBoundary('2026-10-09T12:34:56+07:00', false)).toBe('2026-10-09T05:34:56.000Z')
    expect(executionBoundary('invalid', true)).toBe('invalid')
  })
  it('includes both boundaries of the local calendar day', () => {
    expect(new Date(executionBoundary('2026-10-09', false)).getHours()).toBe(0)
    const end = new Date(executionBoundary('2026-10-09', true))
    expect([end.getHours(), end.getMinutes(), end.getSeconds(), end.getMilliseconds()]).toEqual([
      23, 59, 59, 999,
    ])
  })
})
