import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render } from '@testing-library/svelte'

import RunMark from './RunMark.svelte'

afterEach(() => { cleanup(); vi.useRealTimers() })

describe('RunMark', () => {
  it('holds each word at least 800ms and labels the mark with the word', async () => {
    vi.useFakeTimers()
    const { container, rerender } = render(RunMark, { props: { stage: 'routing', d: 'M0 0h48v48H0z' } })
    const label = () => container.querySelector('svg').getAttribute('aria-label')
    expect(container.textContent).toBe('Routing')
    expect(label()).toBe('Routing')
    await vi.advanceTimersByTimeAsync(300)
    await rerender({ stage: 'tool:grep', d: 'M0 0h48v48H0z' })
    expect(container.textContent).toBe('Routing')
    await vi.advanceTimersByTimeAsync(499)
    expect(container.textContent).toBe('Routing')
    await vi.advanceTimersByTimeAsync(1)
    expect(container.textContent).toBe('Reading')
    expect(label()).toBe('Reading')
    await vi.advanceTimersByTimeAsync(800)
    await rerender({ stage: 'writing', d: 'M0 0h48v48H0z' })
    expect(container.textContent).toBe('Writing')
  })
})
