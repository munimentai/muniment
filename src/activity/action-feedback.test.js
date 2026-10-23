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

// The drive's answer as core sends it: two scripts of one web search each.
// Core gives each script its call's kind and marks it uncounted.
const script = (effectId, kind = 'web') => ({ effectId, displayName: 'codemode', status: 'completed', uncounted: true, input: JSON.stringify({ code: 'const r = await tools.web_search({}); text(r);' }), output: 'done', presentation: { category: 'code', kind, icon: 'file-code', headline: 'Run a script that uses Web search', progress: 'Running a script', result: 'Script completed', details: [{ label: 'Script', value: '1 line' }] } })
const made = (effectId, parentEffectId, headline, kind = 'web') => ({ effectId, parentEffectId, displayName: 'web_search', status: 'completed', input: '{"query":"x"}', presentation: { category: 'web', kind, icon: 'globe', headline, progress: 'Searching the web', result: 'Found 3 results', via: 'via script' } })
export const webAnswer = [
  script('call_a'), made('call_a/1', 'call_a', 'Search the web for “Tauri 2 release notes”'),
  script('call_b'), made('call_b/1', 'call_b', 'Search the web for “Tauri 2 release notes”'),
]

describe('script calls', () => {
  it('shows each script as the parent row with its one search under it', () => {
    const groups = actionGroups(webAnswer)
    expect(groups.map((group) => [group.kind, group.calls])).toEqual([['web', 2]])
    expect(groups[0].actions.map((action) => [action.label, action.calls, action.children.map((child) => [child.label, child.presentation.via])])).toEqual([
      ['Run a script that uses Web search', 1, [['Search the web for “Tauri 2 release notes”', 'via script']]],
      ['Run a script that uses Web search', 1, [['Search the web for “Tauri 2 release notes”', 'via script']]],
    ])
  })

  it('nests every call of a script under it and counts each call', () => {
    const groups = actionGroups([script('s1', 'other'), made('s1/1', 's1', 'Search A'), { ...made('s1/2', 's1', 'Read a.md', 'read') }, call('read')])
    expect(groups.map((group) => [group.kind, group.calls])).toEqual([['other', 2], ['read', 1]])
    expect(groups[0].actions[0].children.map((child) => child.label)).toEqual(['Search A', 'Read a.md'])
  })

  it('keeps a call whose script the list does not hold as its own row', () => {
    const groups = actionGroups([made('x/1', 'x', 'Search C')])
    expect(groups[0].actions[0]).toMatchObject({ label: 'Search C', calls: 1 })
    expect(groups[0].actions[0].children).toBeUndefined()
  })
})
