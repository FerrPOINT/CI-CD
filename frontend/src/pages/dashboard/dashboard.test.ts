import { describe, expect, it } from 'vitest'
import { statusLabel } from './status'

describe('statusLabel', () => {
  it('capitalizes a known status', () => {
    expect(statusLabel('queued')).toBe('Queued')
  })

  it('handles success', () => {
    expect(statusLabel('success')).toBe('Success')
  })
})