import { expect, it } from 'vitest'
import { runActive, runDuration, runNote, runStatusLabel, runTone, triggerLabel, whenLabel } from './agent-labels.js'

it('names every run state the runtime sends, and ones it does not know', () => {
  expect(['queued', 'leased', 'running', 'waiting_input', 'completed', 'failed', 'cancelled', 'interrupted'].map(runStatusLabel))
    .toEqual(['Queued', 'Starting', 'Running', 'Waiting for you', 'Completed', 'Failed', 'Stopped', 'Interrupted'])
  expect(runStatusLabel('paused_by_owner')).toBe('Paused by owner')
  expect(runStatusLabel('')).toBe('')
  expect(runStatusLabel(undefined)).toBe('')
})

it('holds the agent only while a run is queued, starting, running or waiting', () => {
  expect(['queued', 'leased', 'running', 'waiting_input'].every(runActive)).toBe(true)
  expect(['completed', 'failed', 'cancelled', 'interrupted', undefined].some(runActive)).toBe(false)
  expect(runTone('failed')).toBe('critical')
  expect(runTone('waiting_input')).toBe('caution')
  expect(runTone('completed')).toBe('quiet')
})

it('names each trigger kind', () => {
  expect(['manual', 'cron', 'webhook', 'mcp', 'agent_message'].map(triggerLabel))
    .toEqual(['Manual', 'Schedule', 'Webhook', 'Tool check', 'Agent message'])
  expect(triggerLabel('calendar_event')).toBe('Calendar event')
  expect(triggerLabel(null)).toBe('')
})

it('times a settled run by its record and a live run until now', () => {
  const run = { status: 'completed', created_at: '2026-01-01T14:00:00Z', updated_at: '2026-01-01T14:02:10Z' }
  expect(runDuration(run)).toBe('2m 10s')
  expect(runDuration({ ...run, status: 'running' }, Date.parse('2026-01-01T14:00:45Z'))).toBe('45s')
  expect(runDuration({ status: 'completed' })).toBe('')
})

it('summarizes a run by its silence, error or result', () => {
  expect(runNote({ silent: true, result_summary: 'x' })).toBe('No reply needed')
  expect(runNote({ error: 'The model refused.', result_summary: 'x' })).toBe('The model refused.')
  expect(runNote({ result_summary: 'Three new leads.' })).toBe('Three new leads.')
  expect(runNote(null)).toBe('')
})

it('names a time by how long ago it was', () => {
  const now = Date.parse('2026-01-01T14:00:00Z')
  expect(whenLabel('2026-01-01T13:59:50Z', now)).toBe('Just now')
  expect(whenLabel('2026-01-01T13:35:00Z', now)).toBe('25m ago')
  expect(whenLabel('2026-01-01T09:00:00Z', now)).toBe('5h ago')
  expect(whenLabel(null, now)).toBe('')
})

it('notes what an imported template brought', async () => {
  const { templateNote } = await import('./agent-labels.js')
  expect(templateNote(null)).toBe('')
  expect(templateNote({ coverage: 'public-profile' })).toBe('Imported from a public share page, which holds the profile only.')
  expect(templateNote({ coverage: 'provided', skills: [{ name: 'Tone' }], plugins: ['Gmail', 'Drive'] })).toBe('Imported from a template. 1 skill stays as reference text. 2 tools need setup in Tools.')
})

it('names when a run started in local time with no time zone', async () => {
  const { runTimeLabel } = await import('./agent-labels.js')
  const now = new Date(2026, 9, 20, 15, 0).getTime()
  const time = (date) => date.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })
  const today = new Date(2026, 9, 20, 8, 56)
  const yesterday = new Date(2026, 9, 19, 9, 3)
  const earlier = new Date(2026, 9, 12, 9, 8)
  const lastYear = new Date(2025, 11, 30, 9, 8)
  expect(runTimeLabel(today.toISOString(), now)).toBe(`Today at ${time(today)}`)
  expect(runTimeLabel(yesterday.toISOString(), now)).toBe(`Yesterday at ${time(yesterday)}`)
  const tomorrow = new Date(2026, 9, 21, 2, 0)
  expect(runTimeLabel(tomorrow.toISOString(), now)).toBe(`Tomorrow at ${time(tomorrow)}`)
  expect(runTimeLabel(earlier.toISOString(), now)).toBe(`${earlier.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })} at ${time(earlier)}`)
  expect(runTimeLabel(lastYear.toISOString(), now)).toContain('2025')
  expect(runTimeLabel(today.toISOString(), now)).not.toMatch(/UTC|GMT|[A-Z]{3,4}T\b/)
  expect(runTimeLabel('not a time', now)).toBe('')
})

it('titles a run by its routine, else by what started it, and picks its stoplight', async () => {
  const { runLight, runOutcome, runTitle } = await import('./agent-labels.js')
  const routines = [{ id: 'daily', name: 'Morning digest' }]
  expect(runTitle({ trigger: { kind: 'cron', routine_id: 'daily' } }, routines)).toBe('Morning digest')
  expect(runTitle({ trigger: { kind: 'manual', source: 'desktop' } }, routines)).toBe('Run now')
  expect(runTitle({ trigger: { kind: 'manual', source: 'test' } }, routines)).toBe('Test run')
  expect(runTitle({ trigger: { kind: 'webhook', source: 'replay' } }, routines)).toBe('Retry')
  expect(runTitle({ trigger: { kind: 'agent_message' } }, routines)).toBe('Agent message')
  expect([{ status: 'completed' }, { status: 'completed', silent: true }, { status: 'waiting_input' }, { status: 'running' }, { status: 'failed' }].map(runLight)).toEqual(['green', 'yellow', 'yellow', 'yellow', 'red'])
  expect([{ status: 'completed' }, { status: 'completed', silent: true }, { status: 'failed' }].map(runOutcome)).toEqual(['', 'No reply needed', 'Failed'])
})
