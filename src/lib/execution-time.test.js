import { describe, expect, it, vi } from 'vitest'
import { cleanup, render } from '@testing-library/svelte'
import { tick } from 'svelte'
import ExecutionTime from './ExecutionTime.svelte'
import { executionTime, nextSecondChange, receiptTime } from './execution-time.js'

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

  it('shows a new second where the rounding turns over, so its last live value is the receipt', async () => {
    expect(nextSecondChange(0)).toBe(501)
    expect(nextSecondChange(5499)).toBe(2)
    expect(nextSecondChange(5500)).toBe(1001)
    expect(nextSecondChange(-20)).toBe(1000)
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-01-01T00:00:00.000Z'))
    try {
      // The chip mounts 300 ms after the run starts, off the second.
      vi.advanceTimersByTime(300)
      const { container, unmount } = render(ExecutionTime, { startedAt: '2026-01-01T00:00:00.000Z' })
      await tick()
      expect(container.textContent).toContain('0s')
      await vi.advanceTimersByTimeAsync(5199)
      expect(container.textContent).toContain('5s')
      await vi.advanceTimersByTimeAsync(2)
      expect(container.textContent).toContain('6s')
      // The runtime records 5.6 seconds for the run, and the receipt reads as the chip.
      await vi.advanceTimersByTimeAsync(99)
      expect(container.textContent).toContain(receiptTime('5.6s'))
      expect(receiptTime('5.6s')).toBe('6s')
      unmount()
      expect(vi.getTimerCount()).toBe(0)
    } finally {
      cleanup()
      vi.useRealTimers()
    }
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
