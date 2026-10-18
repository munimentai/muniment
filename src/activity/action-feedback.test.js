import fs from 'node:fs'
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import { actionGroups, actionDuration, responseParts } from './action-feedback.js'
import { callLine, headlineParts, rawInput, ruleInput } from './tool-call.js'

// Core's presentations of `core/protocol-fixtures/tool-presentation/calls.json`.
const gallery = JSON.parse(fs.readFileSync(path.join(process.cwd(), 'core/protocol-fixtures/tool-presentation/presented.json'), 'utf8'))
const call = (id) => gallery.calls.find((activity) => activity.effectId === id)

describe('action feedback', () => {
  it('names each call by the headline core gives it', () => {
    const labels = actionGroups([call('mcp'), call('schema'), call('web')]).flatMap((group) => group.actions.map((action) => action.label))
    expect(labels).toEqual([
      'Ask Linear: create issue “Fix sync bug”',
      'Ask CRM: find leads “EMEA”',
      'Search the web for “Local desktop October 2026” and 1 more on news.ycombinator.com',
    ])
  })

  it('groups consecutive calls of one kind without changing order', () => {
    const groups = actionGroups([call('read'), call('grep'), call('test'), call('push'), call('mcp'), call('limit')], true)
    expect(groups.map((group) => [group.kind, group.label])).toEqual([['read', 'Read files'], ['search', 'Searched files'], ['command', 'Ran commands'], ['mcp', 'Called servers']])
    expect(groups[2].actions.map((action) => action.result)).toEqual(['Exit 0 in 4.2 s', 'Exit 1 in 1.4 s'])
    expect(groups[3].actions[1]).toMatchObject({ failed: true, state: 'Failed', result: 'Failed: rate limit exceeded' })
    expect(groups[0].actions[0].path).toBe('/Users/sam/work/muniment/README.md')
  })

  it('shows the words while a call runs and stops them when the run ends', () => {
    const running = call('plugin')
    expect(callLine(running, true)).toMatchObject({ label: 'Using translate document', running: true, state: 'In progress', result: '' })
    expect(callLine(running, false)).toMatchObject({ label: 'Translate document brief.pdf', running: false, state: 'Interrupted' })
  })

  it('names a record without a presentation by its tool and formats elapsed time', () => {
    expect(actionGroups([{ effectId: 'a', displayName: 'old_tool', status: 'completed' }])[0].actions[0].label).toBe('old_tool')
    expect(actionGroups([{ effectId: 'a', status: 'completed' }])[0].actions[0].label).toBe('Tool call')
    expect(actionDuration('2026-01-01T00:00:00Z', '2026-01-01T00:04:48Z')).toBe('4m 48s')
    expect(actionDuration(undefined, undefined)).toBe('')
  })

  it('never puts raw JSON in a headline or a detail', () => {
    for (const activity of gallery.calls) {
      const { presentation } = activity
      for (const text of [presentation.headline, presentation.progress, presentation.result ?? '', ...(presentation.details ?? []).map((detail) => detail.value)]) {
        expect(text, activity.effectId).not.toMatch(/[{[]"|":/)
      }
    }
  })

  it('splits code out of a headline and keeps the raw input for details', () => {
    expect(headlineParts('Run `npm test` in work')).toEqual([{ text: 'Run ', code: false }, { text: 'npm test', code: true }, { text: ' in work', code: false }])
    expect(rawInput('{"a":1}')).toBe('{\n  "a": 1\n}')
    expect(rawInput('plain')).toBe('plain')
    expect(ruleInput(gallery.gates[0].rule)).toContain('"tool": "web_search"')
  })
})

it('places actions between the text that precedes and follows them', () => {
  const first = { effectId: 'search', textOffset: 0 }
  const second = { effectId: 'read', textOffset: 9 }
  expect(responseParts({ text: 'Check 🔎. Result.', toolActivity: [first, second] })).toEqual([
    { type: 'actions', activities: [first] },
    { type: 'text', text: 'Check 🔎.' },
    { type: 'actions', activities: [second] },
    { type: 'text', text: ' Result.' },
  ])
})
