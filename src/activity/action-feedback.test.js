import { describe, expect, it } from 'vitest'
import { actionGroups, actionDuration } from './action-feedback.js'

describe('action feedback', () => {
  const actions = [
    { effectId: 'a', displayName: 'read', status: 'completed', input: '{"path":"/project/src/main.rs"}' },
    { effectId: 'b', displayName: 'read', status: 'running', input: '{"path":"C:\\\\project\\\\other.rs"}' },
    { effectId: 'c', displayName: 'bash', status: 'failed', input: '{"command":"cargo test","description":"Run tests"}' },
    { effectId: 'd', displayName: 'read', status: 'completed' },
  ]
  it('groups consecutive actions without changing order and uses readable names', () => {
    const groups = actionGroups(actions, true)
    expect(groups.map((group) => group.label)).toEqual(['Reading files', 'Ran commands', 'Read files'])
    expect(groups[0].actions.map((action) => action.label)).toEqual(['Read main.rs', 'Reading other.rs'])
    expect(groups[1].actions[0]).toMatchObject({ label: 'Run tests', state: 'Failed' })
  })
  it('does not keep an interrupted action active after the run ends', () => {
    const group = actionGroups(actions, false)[0]
    expect(group.running).toBe(false)
    expect(group.actions[1]).toMatchObject({ state: 'Interrupted', running: false })
  })
  it('handles old history without details and formats elapsed time', () => {
    expect(actionGroups([{ effectId: 'a', status: 'completed' }])[0].actions[0].label).toBe('Action')
    expect(actionDuration('2026-01-01T00:00:00Z', '2026-01-01T00:04:48Z')).toBe('4m 48s')
    expect(actionDuration(undefined, undefined)).toBe('')
  })
})
