import { describe, expect, it } from 'vitest'

import { COPY_CONFIRMATION_MS, copyAnnouncement, copyConfirmed, copyFailure, copyLabel, copyResult } from './message-actions.js'

describe('message actions', () => {
  it('confirms for about two seconds', () => {
    expect(COPY_CONFIRMATION_MS).toBe(2000)
  })

  it('labels only the response that was copied', () => {
    const copy = copyResult('run-1', true)
    expect(copyLabel(copy, 'run-1')).toBe('Copied')
    expect(copyLabel(copy, 'run-2')).toBe('Copy')
    expect(copyLabel(null, 'run-1')).toBe('Copy')
  })

  it('returns the button to Copy when the write failed', () => {
    expect(copyLabel(copyResult('run-1', false), 'run-1')).toBe('Copy')
    expect(copyConfirmed(copyResult('run-1', false), 'run-1')).toBe(false)
    expect(copyConfirmed(copyResult('run-1', true), 'run-1')).toBe(true)
    expect(copyConfirmed(copyResult('run-1', true), 'run-2')).toBe(false)
  })

  it('records the failure against its own response, with the next step and no apology', () => {
    const copy = copyResult('run-1', false)
    expect(copyFailure(copy, 'run-1')).toBe('Clipboard unavailable. Select the reply and press ⌘C to copy it.')
    expect(copyFailure(copy, 'run-1', 'Ctrl ')).toBe('Clipboard unavailable. Select the reply and press Ctrl C to copy it.')
    expect(copyFailure(copy, 'run-2')).toBe('')
    expect(copyFailure(copyResult('run-1', true), 'run-1')).toBe('')
    expect(copyFailure(null, 'run-1')).toBe('')
  })

  it('announces the outcome, and says nothing at all once the confirmation lapses', () => {
    expect(copyAnnouncement(copyResult('run-1', true))).toBe('Reply copied to the clipboard.')
    expect(copyAnnouncement(copyResult('run-1', false))).toBe('Clipboard unavailable. Select the reply and press ⌘C to copy it.')
    expect(copyAnnouncement(null)).toBe('')
  })
})
