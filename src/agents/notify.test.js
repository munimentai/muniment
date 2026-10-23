import { afterEach, describe, expect, it, vi } from 'vitest'
import { notificationKey, notificationText, shouldNotify, showNotification } from './notify.js'

const run = (status, extra = {}) => ({ id: 'r1', status, trigger: { kind: 'cron' }, ...extra })

describe('run notifications', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('follows the runtime when it marks a run', () => {
    expect(shouldNotify(run('running', { notify: true }))).toBe(true)
    expect(shouldNotify(run('failed', { notify: false }))).toBe(false)
  })

  it('notifies for settled and waiting runs this app did not start', () => {
    for (const status of ['completed', 'failed', 'waiting_input']) expect(shouldNotify(run(status))).toBe(true)
    for (const status of ['queued', 'leased', 'running', 'cancelled', 'interrupted']) expect(shouldNotify(run(status))).toBe(false)
    expect(shouldNotify(run('completed'), new Set(['r1']))).toBe(false)
    expect(shouldNotify(run('completed', { trigger: { kind: 'manual' } }))).toBe(false)
  })

  it('keys a notification by machine, run and state', () => {
    expect(notificationKey(run('failed'))).toBe('local:r1:failed')
    expect(notificationKey(run('failed', { runtimeId: 'box' }))).toBe('box:r1:failed')
  })

  it('says what happened and where to look', () => {
    expect(notificationText(run('failed', { error: 'The model refused.' }), 'Scout')).toEqual({ title: 'Scout failed', body: 'The model refused.' })
    expect(notificationText(run('waiting_input'), 'Scout').title).toBe('Scout is waiting for you')
    expect(notificationText(run('completed', { result_summary: 'Three new leads.' }), 'Scout').body).toBe('Three new leads.')
    expect(notificationText(run('completed', { silent: true }), '').title).toBe('An agent finished')
  })

  it('shows a system notification only when the user allowed one', () => {
    vi.stubGlobal('Notification', undefined)
    expect(showNotification({ title: 't', body: 'b' })).toBe(false)
    const shown = []
    const Notice = vi.fn(function (title, options) { shown.push([title, options]) })
    Notice.permission = 'granted'
    vi.stubGlobal('Notification', Notice)
    expect(showNotification({ title: 't', body: 'b' })).toBe(true)
    expect(shown).toEqual([['t', { body: 'b' }]])
    Notice.permission = 'default'
    Notice.requestPermission = vi.fn(async () => 'denied')
    expect(showNotification({ title: 't', body: 'b' })).toBe(false)
    expect(Notice.requestPermission).toHaveBeenCalledOnce()
  })
})
