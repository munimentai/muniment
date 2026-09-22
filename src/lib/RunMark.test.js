import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render } from '@testing-library/svelte'

import RunMark from './RunMark.svelte'
import { graphPaths } from './graph-mark.js'

afterEach(() => { cleanup(); vi.useRealTimers() })

describe('RunMark', () => {
  it('holds each word at least 800ms and labels the mark with the word', async () => {
    vi.useFakeTimers()
    const { container, rerender } = render(RunMark, { props: { stage: 'routing' } })
    const label = () => container.querySelector('svg').getAttribute('aria-label')
    expect(container.textContent).toBe('Routing')
    expect(label()).toBe('Routing')
    await vi.advanceTimersByTimeAsync(300)
    await rerender({ stage: 'tool:grep' })
    expect(container.textContent).toBe('Routing')
    await vi.advanceTimersByTimeAsync(499)
    expect(container.textContent).toBe('Routing')
    await vi.advanceTimersByTimeAsync(1)
    expect(container.textContent).toBe('Reading')
    expect(label()).toBe('Reading')
    await vi.advanceTimersByTimeAsync(800)
    await rerender({ stage: 'writing' })
    expect(container.textContent).toBe('Writing')
  })

  it('draws the reduced graph at 20px with an outline and trace in one rotating group', () => {
    const { container } = render(RunMark, { props: { stage: 'routing' } })
    const svg = container.querySelector('svg')
    expect(svg.getAttribute('width')).toBe('20')
    expect(svg.getAttribute('viewBox')).toBe('0 0 48 48')
    const group = svg.querySelector('g')
    expect(group.getAttribute('transform')).toBe('rotate(0.00 24 24)')
    expect(group.querySelector('path.body').getAttribute('d')).toBe(graphPaths(20).edges)
    expect(group.querySelectorAll('path.body')).toHaveLength(2)
    expect(group.querySelectorAll('path.body')[1].getAttribute('d')).toBe(graphPaths(20).outline)
    expect(group.querySelector('path.accent').style.opacity).toBe('0')
  })
})
