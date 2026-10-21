import '@testing-library/jest-dom/vitest'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render } from '@testing-library/svelte'
import { createRawSnippet } from 'svelte'

import StatusRing from './StatusRing.svelte'
import { RESULT_MS } from './status-ring.js'

afterEach(() => { cleanup(); vi.useRealTimers() })

const icon = createRawSnippet(() => ({ render: () => '<span class="own-icon"></span>' }))
let keys = 0
const ring = (props) => render(StatusRing, { props: { key: `thread-${++keys}`, children: icon, ...props } })
const mark = () => document.querySelector('.status-ring')

describe('StatusRing', () => {
  it.each([
    ['working', 'Working', 'success'],
    ['needs_you', 'Approve', 'warning'],
  ])('travels while the thread is %s, in its tone, named in core\'s words', (status, label, tone) => {
    const { container } = ring({ status, label })
    expect(mark()).toHaveAttribute('data-tone', tone)
    expect(mark()).toHaveAttribute('data-phase', 'travel')
    expect(mark()).toHaveAttribute('aria-hidden', 'true')
    expect(mark().querySelectorAll('[data-icon="circle-dashed"] path')).toHaveLength(8)
    expect(container.querySelector('.visually-hidden')).toHaveTextContent(label)
    expect(container.querySelector('.own-icon')).toBeNull()
  })

  it.each([
    ['done', 'Done', 'success'],
    ['failed', 'Failed', 'danger'],
  ])('lights every dash for a %s result, then shows the row\'s own icon and keeps the words', async (status, label, tone) => {
    vi.useFakeTimers()
    const { container } = ring({ status, label })
    expect(mark()).toHaveAttribute('data-tone', tone)
    expect(mark()).toHaveAttribute('data-phase', 'lit')
    expect(container.querySelector('.own-icon')).toBeNull()
    await vi.advanceTimersByTimeAsync(RESULT_MS - 1)
    expect(mark()).not.toBeNull()
    await vi.advanceTimersByTimeAsync(1)
    expect(mark()).toBeNull()
    expect(container.querySelector('.own-icon')).not.toBeNull()
    expect(container.querySelector('.visually-hidden')).toHaveTextContent(label)
  })

  it('lights a result once: a row drawn again keeps the time it had left', async () => {
    vi.useFakeTimers()
    const first = render(StatusRing, { props: { status: 'done', label: 'Done', key: 'shared' } })
    await vi.advanceTimersByTimeAsync(RESULT_MS / 2)
    first.unmount()
    render(StatusRing, { props: { status: 'done', label: 'Done', key: 'shared' } })
    expect(mark()).not.toBeNull()
    await vi.advanceTimersByTimeAsync(RESULT_MS / 2)
    expect(mark()).toBeNull()
    cleanup()
    render(StatusRing, { props: { status: 'done', label: 'Done', key: 'shared' } })
    expect(mark()).toBeNull()
  })

  it('shows the row\'s own icon for a read thread, and an empty slot with none', () => {
    const { container } = ring({ status: 'read', label: '' })
    expect(mark()).toBeNull()
    expect(container.querySelector('.own-icon')).not.toBeNull()
    expect(container.querySelector('.status-lead')).not.toHaveAttribute('data-empty')
    cleanup()
    const bare = render(StatusRing, { props: { status: 'read', key: 'bare' } })
    expect(bare.container.querySelector('.status-lead')).toHaveAttribute('data-empty', 'true')
  })

  it('travels again when a settled thread starts work', async () => {
    const { rerender } = ring({ status: 'read', label: '' })
    expect(mark()).toBeNull()
    await rerender({ status: 'working', label: 'Working' })
    expect(mark()).toHaveAttribute('data-phase', 'travel')
  })
})
