import { describe, expect, it, vi } from 'vitest'
import { cleanup, render } from '@testing-library/svelte'
import { tick } from 'svelte'
import ExecutionTime from './ExecutionTime.svelte'
import { executionTime, receiptTime } from './execution-time.js'

describe('execution time', () => {
  it.each([[0, '0s'], [3.8, '4s'], [59.6, '1m 0s'], [111.6, '1m 52s'], [3599.6, '1h 0m 0s'], [7323, '2h 2m 3s']])('formats %s seconds as %s', (seconds, expected) => {
    expect(executionTime(seconds)).toBe(expected)
    expect(receiptTime(`${seconds}s`)).toBe(expected)
  })

  it('keeps existing human-readable receipts and rejects nonfinite durations', () => {
    expect(receiptTime('2m 3s')).toBe('2m 3s')
    expect(executionTime(NaN)).toBeNull()
    expect(executionTime(Infinity)).toBeNull()
    expect(executionTime(-10)).toBe('0s')
  })

  it('ticks from the original start time and clears its timer when removed', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-01-01T01:00:00Z'))
    try {
      const { container, unmount } = render(ExecutionTime, { startedAt: '2026-01-01T00:58:08.400Z' })
      await tick()
      expect(container.textContent).toContain('1m 52s')
      await vi.advanceTimersByTimeAsync(2000)
      expect(container.textContent).toContain('1m 54s')
      unmount()
      expect(vi.getTimerCount()).toBe(0)
    } finally {
      cleanup()
      vi.useRealTimers()
    }
  })
})
