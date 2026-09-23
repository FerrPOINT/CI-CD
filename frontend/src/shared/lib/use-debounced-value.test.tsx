import { act, renderHook } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useDebouncedValue } from './use-debounced-value'

afterEach(() => vi.useRealTimers())

describe('useDebouncedValue', () => {
  it('publishes only the latest value after the delay', () => {
    vi.useFakeTimers()
    const { result, rerender } = renderHook(
      ({ value }) => useDebouncedValue(value, 200),
      { initialProps: { value: '' } },
    )

    rerender({ value: 'release' })
    rerender({ value: 'release/2026' })
    act(() => vi.advanceTimersByTime(199))
    expect(result.current).toBe('')
    act(() => vi.advanceTimersByTime(1))
    expect(result.current).toBe('release/2026')
  })
})
