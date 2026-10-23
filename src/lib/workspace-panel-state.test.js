import { describe, expect, it } from 'vitest'
import { workspaceOnNavigate } from './workspace-panel-state.js'

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
