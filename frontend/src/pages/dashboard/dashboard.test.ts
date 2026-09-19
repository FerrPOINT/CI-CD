import { describe, expect, it } from 'vitest'
import { statusLabel } from './status'
import { summarizePipelines } from './model'
import type { Pipeline } from '@/api/types'

describe('statusLabel', () => {
  it('capitalizes a known status', () => {
    expect(statusLabel('queued')).toBe('Queued')
  })

  it('handles success', () => {
    expect(statusLabel('success')).toBe('Success')
  })
})

describe('summarizePipelines', () => {
  it('counts active states and shows the newest runs first', () => {
    const runs = [
      { id: 'older', status: 'failed', created_at: '2026-09-17T10:00:00Z' },
      { id: 'newer', status: 'running', created_at: '2026-09-19T10:00:00Z' },
      { id: 'middle', status: 'queued', created_at: '2026-09-18T10:00:00Z' },
    ] as Pipeline[]

    expect(summarizePipelines(runs)).toMatchObject({
      queued: 1,
      running: 1,
      failed: 1,
      recent: [{ id: 'newer' }, { id: 'middle' }, { id: 'older' }],
    })
    expect(runs[0].id).toBe('older')
  })

  it('limits the activity list to eight runs', () => {
    const runs = Array.from({ length: 10 }, (_, index) => ({
      id: `run-${index}`,
      status: 'success',
      created_at: `2026-09-19T10:${String(index).padStart(2, '0')}:00Z`,
    })) as Pipeline[]

    expect(summarizePipelines(runs).recent).toHaveLength(8)
    expect(summarizePipelines(runs).recent[0].id).toBe('run-9')
  })
})
