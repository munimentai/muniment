import { describe, expect, it } from 'vitest'
import { profileOnNavigate, workspaceOnNavigate } from './workspace-panel-state.js'

describe('workspace panel on navigation', () => {
  it('hides the open tool on Settings or a catalog page and restores it on return', () => {
    const away = workspaceOnNavigate({ panel: 'artifacts', parked: null }, false)
    expect(away).toEqual({ panel: null, parked: 'artifacts' })
    expect(workspaceOnNavigate(away, true)).toEqual({ panel: 'artifacts', parked: null })
  })

  it('keeps a tool the user opened away from the thread and drops the parked one', () => {
    expect(workspaceOnNavigate({ panel: 'files', parked: 'artifacts' }, true)).toEqual({ panel: 'files', parked: null })
  })

  it('leaves a closed panel closed', () => {
    expect(workspaceOnNavigate({ panel: null, parked: null }, false)).toEqual({ panel: null, parked: null })
    expect(workspaceOnNavigate({ panel: null, parked: null }, true)).toEqual({ panel: null, parked: null })
  })
})

describe('agent profile on navigation', () => {
  it('puts the open profile aside on another page and opens it again on return to the same thread', () => {
    const open = profileOnNavigate({}, { threadView: true, threadId: 'thread-1', open: true })
    expect(open).toEqual({ shown: 'thread-1', parked: null, restore: false })
    // The page that opens closes the profile in the same step.
    const away = profileOnNavigate(open, { threadView: false, threadId: 'thread-1', open: false })
    expect(away).toEqual({ shown: null, parked: 'thread-1', restore: false })
    expect(profileOnNavigate(away, { threadView: false, threadId: 'thread-1', open: false })).toEqual(away)
    expect(profileOnNavigate(away, { threadView: true, threadId: 'thread-1', open: false })).toEqual({ shown: 'thread-1', parked: null, restore: true })
  })

  it('drops the profile put aside when another thread shows', () => {
    const away = { shown: null, parked: 'thread-1' }
    // The page closes before the other thread opens, so the old thread still names itself.
    expect(profileOnNavigate(away, { threadView: true, threadId: 'thread-1', open: false, switching: true })).toEqual({ ...away, restore: false })
    expect(profileOnNavigate(away, { threadView: true, threadId: 'thread-2', open: false })).toEqual({ shown: null, parked: null, restore: false })
  })

  it('keeps a profile closed in the thread view closed after a round trip', () => {
    const closed = profileOnNavigate({ shown: 'thread-1' }, { threadView: true, threadId: 'thread-1', open: false })
    expect(closed).toEqual({ shown: null, parked: null, restore: false })
    const away = profileOnNavigate(closed, { threadView: false, threadId: 'thread-1', open: false })
    expect(profileOnNavigate(away, { threadView: true, threadId: 'thread-1', open: false }).restore).toBe(false)
  })
})
