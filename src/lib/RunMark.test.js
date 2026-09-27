import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render } from '@testing-library/svelte'

import RunMark from './RunMark.svelte'
import { pocketBody, pocketEar } from './pocket-fold.js'

afterEach(() => { cleanup(); vi.useRealTimers() })

describe('RunMark', () => {
  it('holds each status word at least 800ms beside a decorative mark', async () => {
    vi.useFakeTimers()
    const { container, rerender } = render(RunMark, { props: { stage: 'routing' } })
    const hidden = () => container.querySelector('svg').getAttribute('aria-hidden')
    expect(container.textContent).toBe('Routing')
    expect(hidden()).toBe('true')
    await vi.advanceTimersByTimeAsync(300)
    await rerender({ stage: 'tool:grep' })
    expect(container.textContent).toBe('Routing')
    await vi.advanceTimersByTimeAsync(499)
    expect(container.textContent).toBe('Routing')
    await vi.advanceTimersByTimeAsync(1)
    expect(container.textContent).toBe('Reading')
    expect(hidden()).toBe('true')
    await vi.advanceTimersByTimeAsync(800)
    await rerender({ stage: 'writing' })
    expect(container.textContent).toBe('Writing')
  })

  it('draws the geometric symbol and straight verdigris fold at 20px', () => {
    const { container } = render(RunMark, { props: { stage: 'routing' } })
    const svg = container.querySelector('svg')
    expect(svg.getAttribute('width')).toBe('20')
    expect(svg.querySelectorAll('path')).toHaveLength(2)
    expect(svg.querySelectorAll('path')[0].getAttribute('d')).toBe(pocketBody)
    expect(svg.querySelectorAll('path')[1].getAttribute('d')).toBe(pocketEar)
    expect(svg.querySelectorAll('path')[1].getAttribute('fill')).toBe('#2A7264')
  })
})
