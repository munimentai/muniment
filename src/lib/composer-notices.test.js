import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { COMPOSER_NOTICE_TIMEOUT, createComposerNotices } from './composer-notices.js'

const notice = (id, cause = 'a', extra = {}) => ({ id, cause, text: `${id} text`, ...extra })

function setup(covered) {
  const notices = createComposerNotices()
  const seen = []
  notices.subscribe((current) => seen.push(current?.id ?? null))
  const detach = notices.attach(covered)
  return { notices, seen, detach }
}

describe('composer notices', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => {
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  it('leaves on its own after about six seconds', () => {
    const { notices } = setup()
    expect(COMPOSER_NOTICE_TIMEOUT).toBe(6000)
    notices.show(notice('one'))
    vi.advanceTimersByTime(5999)
    expect(notices.current()?.id).toBe('one')
    vi.advanceTimersByTime(1)
    expect(notices.current()).toBeNull()
  })

  it('shows one notice at a time and the next once the current one leaves', () => {
    const { notices, seen } = setup()
    notices.show(notice('one'))
    notices.show(notice('two'))
    expect(notices.current()?.id).toBe('one')
    vi.advanceTimersByTime(6000)
    expect(notices.current()?.id).toBe('two')
    vi.advanceTimersByTime(6000)
    expect(seen).toEqual([null, 'one', 'two', null])
  })

  it('holds the clock while the pointer or keyboard focus holds the notice', () => {
    const { notices } = setup()
    notices.show(notice('one'))
    vi.advanceTimersByTime(4000)
    notices.hold('pointer')
    notices.hold('focus')
    vi.advanceTimersByTime(60_000)
    expect(notices.current()?.id).toBe('one')
    notices.release('pointer')
    vi.advanceTimersByTime(60_000)
    expect(notices.current()?.id).toBe('one')
    notices.release('focus')
    vi.advanceTimersByTime(1999)
    expect(notices.current()?.id).toBe('one')
    vi.advanceTimersByTime(1)
    expect(notices.current()).toBeNull()
  })

  it('closes on dismiss, tells its owner and shows the next', () => {
    const { notices } = setup()
    const ondismiss = vi.fn()
    notices.show(notice('one', 'a', { ondismiss }))
    notices.show(notice('two'))
    notices.hold('pointer')
    notices.dismiss()
    expect(ondismiss).toHaveBeenCalledOnce()
    expect(notices.current()?.id).toBe('two')
    vi.advanceTimersByTime(6000)
    expect(notices.current()).toBeNull()
  })

  it('runs the action and keeps the notice on its clock', () => {
    const { notices } = setup()
    const run = vi.fn()
    notices.show(notice('one', 'a', { action: { label: 'Open', run } }))
    vi.advanceTimersByTime(3000)
    notices.act()
    expect(run).toHaveBeenCalledOnce()
    expect(notices.current()?.id).toBe('one')
    vi.advanceTimersByTime(3000)
    expect(notices.current()).toBeNull()
  })

  it('shows once per cause and again when the cause changes', () => {
    const { notices } = setup()
    expect(notices.show(notice('one', 'a'))).toBe(true)
    expect(notices.show(notice('one', 'a'))).toBe(false)
    vi.advanceTimersByTime(6000)
    expect(notices.show(notice('one', 'a'))).toBe(false)
    expect(notices.current()).toBeNull()
    notices.show(notice('two'))
    notices.dismiss()
    expect(notices.show(notice('two'))).toBe(false)
    expect(notices.show(notice('one', 'b'))).toBe(true)
    expect(notices.current()).toMatchObject({ id: 'one', cause: 'b' })
  })

  it('restarts the clock when a showing notice gets a new cause', () => {
    const { notices } = setup()
    notices.show(notice('one', 1))
    vi.advanceTimersByTime(5000)
    notices.show(notice('one', 2))
    vi.advanceTimersByTime(5999)
    expect(notices.current()?.cause).toBe('2')
    vi.advanceTimersByTime(1)
    expect(notices.current()).toBeNull()
  })

  it('withdraws a notice whose cause ended, queued or shown', () => {
    const { notices } = setup()
    const ondismiss = vi.fn()
    notices.show(notice('one', 'a', { ondismiss }))
    notices.show(notice('two'))
    notices.show(notice('three'))
    notices.withdraw('two')
    notices.withdraw('one')
    expect(ondismiss).not.toHaveBeenCalled()
    expect(notices.current()?.id).toBe('three')
  })

  it('keeps its clock still while no view shows it', () => {
    const notices = createComposerNotices()
    notices.show(notice('one'))
    vi.advanceTimersByTime(60_000)
    expect(notices.current()?.id).toBe('one')
    const detach = notices.attach()
    vi.advanceTimersByTime(3000)
    detach()
    vi.advanceTimersByTime(60_000)
    expect(notices.current()?.id).toBe('one')
    notices.attach()
    vi.advanceTimersByTime(3000)
    expect(notices.current()).toBeNull()
  })

  it('waits a full timeout once a covering panel closes', () => {
    let covered = true
    const { notices } = setup(() => covered)
    notices.show(notice('one'))
    vi.advanceTimersByTime(6000)
    expect(notices.current()?.id).toBe('one')
    covered = false
    vi.advanceTimersByTime(5999)
    expect(notices.current()?.id).toBe('one')
    vi.advanceTimersByTime(1)
    expect(notices.current()).toBeNull()
  })

  it('keeps a persistent notice until its cause withdraws it', () => {
    const { notices } = setup()
    notices.show(notice('download', 'a', { persistent: true }))
    vi.advanceTimersByTime(60000)
    expect(notices.current()?.id).toBe('download')
    notices.withdraw('download')
    expect(notices.current()).toBeNull()
  })

  it('updates a notice in place without showing it again', () => {
    const { notices } = setup()
    const seen = []
    notices.show(notice('download', 'a', { persistent: true, progress: { value: 1, max: 4 } }))
    notices.subscribe((current) => seen.push(current))
    expect(notices.update('download', { progress: { value: 2, max: 4 } })).toBe(true)
    expect(seen.map((current) => current.progress.value)).toEqual([1, 2])
    expect(notices.current()).toMatchObject({ id: 'download', cause: 'a', persistent: true })
    expect(notices.update('other', { text: 'x' })).toBe(false)
  })

  it('updates a queued notice so it shows with the new fields', () => {
    const { notices } = setup()
    notices.show(notice('one'))
    notices.show(notice('download', 'a', { progress: { value: 1, max: 4 } }))
    expect(notices.update('download', { progress: { value: 3, max: 4 } })).toBe(true)
    vi.advanceTimersByTime(6000)
    expect(notices.current()?.progress.value).toBe(3)
  })

  it('starts the clock once an update makes a notice transient', () => {
    const { notices } = setup()
    notices.show(notice('download', 'a', { persistent: true }))
    vi.advanceTimersByTime(10000)
    notices.update('download', { persistent: false })
    vi.advanceTimersByTime(5999)
    expect(notices.current()?.id).toBe('download')
    vi.advanceTimersByTime(1)
    expect(notices.current()).toBeNull()
  })

  it('turns a progress notice into its success notice under the same id', () => {
    const { notices } = setup()
    notices.show(notice('download', 'installing', { persistent: true }))
    notices.show(notice('download', 'installed', { text: 'Installed.' }))
    expect(notices.current()).toMatchObject({ cause: 'installed', text: 'Installed.' })
    expect(notices.current().persistent).toBeUndefined()
    vi.advanceTimersByTime(6000)
    expect(notices.current()).toBeNull()
  })
})
