import '@testing-library/jest-dom/vitest'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render } from '@testing-library/svelte'

import RunMark from './RunMark.svelte'

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

  it('draws the pocket-fold mark at 20px cut into the parts an elephant moves', () => {
    const { container } = render(RunMark, { props: { stage: 'routing' } })
    const svg = container.querySelector('svg')
    expect(svg.getAttribute('width')).toBe('20')
    expect(svg.getAttribute('viewBox')).toBe('-20 -20 566 499')
    expect([...svg.querySelectorAll('path')].map((path) => path.getAttribute('class').replace(/\s*svelte-\S+/, ''))).toEqual(['body leg hind', 'body leg fore', 'body trunk', 'body torso', 'accent'])
    expect(svg.querySelector('path.torso').getAttribute('d')).toMatch(/^M160 0 H274/)
    expect(svg.querySelector('path.accent').getAttribute('d')).toBe('M294 0 L510 190 H366 A72 72 0 0 1 294 118 Z')
    expect(svg.querySelector('path.trunk').getAttribute('d')).toMatch(/L427 449 A10 10 0 0 0 437 459 L490 459 A36 36 0 0 0 526 423 /)
  })

  it('shows the stage word alone with a sheen for the text choice', () => {
    const { container } = render(RunMark, { props: { stage: 'writing', mark: 'text' } })
    expect(container.querySelector('svg')).toBeNull()
    expect(container.querySelector('.stage-sheen')).toHaveTextContent('Writing')
  })

  it('stills the elephant for the still choice and keeps only the ear for the ear choice', () => {
    const still = render(RunMark, { props: { stage: 'routing', mark: 'still' } })
    expect(still.container.querySelector('.walker')).toHaveClass('still')
    cleanup()
    const ear = render(RunMark, { props: { stage: 'routing', mark: 'ear' } })
    expect(ear.container.querySelector('.walker')).toHaveClass('ear-only')
  })
})
