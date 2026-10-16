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
  expect(templateNote({ coverage: 'provided', skills: [{ name: 'Tone' }], plugins: ['Gmail', 'Drive'] })).toBe('Imported from a template. 1 skill stays as reference text. 2 tools need setup in Extend.')
})
