import { afterEach, beforeEach, expect, it } from 'vitest'
import { isProposal, lastFiredLabel, nextRunLabel, pollLabel, routineWhen, triggerSummary } from './routine-labels.js'
import { runTimeLabel } from '../lib/agent-labels.js'
import { saveTimeSetting } from '../lib/time-format.svelte.js'

beforeEach(() => saveTimeSetting(null, { zone: 'UTC', clock: '24h' }))
afterEach(() => saveTimeSetting(null, { zone: '', clock: '' }))

it('names each trigger kind with its schedule or sender', () => {
  // A draft or an import preview names no time zone.
  expect(triggerSummary({ kind: 'cron', expr: '0 9 * * 1-5', tz: 'UTC' })).toBe('Weekdays at 09:00')
  expect(triggerSummary({ kind: 'cron', expr: '0 9 * * 1-5', tz: 'America/New_York' })).not.toMatch(/America|New_York/)
  expect(triggerSummary({ kind: 'webhook', hook_id: 'h', preset: 'forgejo' })).toBe('Webhook · Forgejo')
  expect(triggerSummary({ kind: 'webhook', hook_id: 'h' })).toBe('Webhook · any sender')
  expect(triggerSummary({ kind: 'mcp', server: 'crm', tool: 'search_leads', every: { expr: '0 9 * * *', tz: 'UTC' } })).toBe('crm search_leads · Every day at 09:00')
})

it('shows when a routine last fired and how that run went', () => {
  const now = Date.parse('2026-01-01T15:00:00Z')
  expect(lastFiredLabel({ last_fired_at: null })).toBe('Not run yet')
  expect(lastFiredLabel({ last_fired_at: '2026-01-01T14:00:00Z', last_result: { status: 'completed', silent: false } }, now)).toBe('1h ago · Completed')
  expect(lastFiredLabel({ last_fired_at: '2026-01-01T14:00:00Z', last_result: { status: 'completed', silent: true } }, now)).toBe('1h ago · No reply needed')
})

it('treats a routine core marks proposed as a proposal', () => {
  expect(isProposal({ created_from: 'instructions', enabled: false, proposed: true })).toBe(true)
  expect(isProposal({ created_from: 'instructions', enabled: false, last_fired_at: null })).toBe(false)
  expect(isProposal({ created_from: 'instructions', enabled: true })).toBe(false)
})

it('shows a tool check\'s last poll, its failure and its next try', () => {
  const now = Date.parse('2026-01-01T15:00:00Z')
  const mcp = (last_poll) => ({ trigger: { kind: 'mcp' }, last_poll })
  expect(pollLabel({ trigger: { kind: 'cron' } })).toBe('')
  expect(pollLabel(mcp(null))).toBe('Not checked yet')
  expect(pollLabel(mcp({ at: '2026-01-01T14:55:00Z', fired: true, failures: 0 }), now)).toBe('Checked 5m ago · condition held, run started')
  expect(pollLabel(mcp({ at: '2026-01-01T14:00:00Z', fired: false, failures: 0 }), now)).toBe('Checked 1h ago · condition did not hold')
  const failed = pollLabel(mcp({ at: '2026-01-01T14:59:50Z', fired: false, error: 'The server did not answer.', failures: 3, retry_at: '2026-01-01T15:04:00Z' }), now)
  expect(failed).toMatch(/^Checked just now · failed: The server did not answer\. · 3 failures in a row · next try /)
})

it('names when a routine runs with no time zone, in the zone the Time setting shows', async () => {
  const { routineIcon } = await import('./routine-labels.js')
  const now = Date.parse('2026-10-20T12:00:00Z')
  expect(routineWhen({ trigger: { kind: 'cron', expr: '0 9 * * 1-5', tz: 'UTC' } }, now)).toBe('Weekdays at 09:00')
  expect(routineWhen({ trigger: { kind: 'cron', expr: '0 9 * * *', tz: 'Asia/Tokyo' } }, now)).toBe('Every day at 00:00')
  await saveTimeSetting(null, { zone: 'Asia/Tokyo', clock: '12h' })
  expect(routineWhen({ trigger: { kind: 'cron', expr: '0 9 * * *', tz: 'Asia/Tokyo' } }, now)).toBe('Every day at 9:00 AM')
  expect(routineWhen({ trigger: { kind: 'webhook', preset: 'github' } })).toBe('Webhook · GitHub')
  expect(routineWhen({ trigger: { kind: 'mcp', server: 'crm', tool: 'search', every: { expr: '*/15 * * * *', tz: 'UTC' } } })).toBe('crm search · every 15 minutes')
  expect(['cron', 'webhook', 'mcp'].map((kind) => routineIcon({ kind }))).toEqual(['clock', 'link', 'plug-zap'])
})

it('reads a routine row and the agent card next run on one clock', async () => {
  // A weekday schedule at 08:00 in New York, with its next run on Tuesday.
  const now = Date.parse('2026-10-20T10:00:00Z')
  const next = Date.parse('2026-10-20T12:00:00Z') / 1000
  const routine = { id: 'brief', enabled: true, trigger: { kind: 'cron', expr: '0 8 * * 1-5', tz: 'America/New_York' }, next_fire_at: next }
  await saveTimeSetting(null, { zone: 'America/New_York', clock: '12h' })
  expect(routineWhen(routine, now)).toBe('Weekdays at 8:00 AM')
  expect(nextRunLabel({ routines: [routine] }, now)).toBe('Today at 8:00 AM')
  await saveTimeSetting(null, { clock: '24h' })
  expect(routineWhen(routine, now)).toBe('Weekdays at 08:00')
  expect(nextRunLabel({ routines: [routine] }, now)).toBe('Today at 08:00')
  await saveTimeSetting(null, { zone: 'Europe/London' })
  expect(routineWhen(routine, now)).toBe('Weekdays at 13:00')
  expect(nextRunLabel({ routines: [routine] }, now)).toBe('Today at 13:00')
})

it('names the next run in local time with no time zone', () => {
  const now = Date.now()
  const next = Math.floor(now / 1000) + 3600
  const cron = (tz, extra = {}) => ({ id: tz, enabled: true, trigger: { kind: 'cron', expr: '0 2 * * *', tz }, ...extra })
  expect(nextRunLabel({ routines: [cron('UTC', { next_fire_at: next })] }, now)).toBe(runTimeLabel(next * 1000, now))
  expect(nextRunLabel({ routines: [cron('UTC')] }, now)).toBe('Every day at 02:00')
  expect(nextRunLabel({ routines: [cron('UTC')] }, now)).not.toMatch(/UTC|GMT/)
  expect(nextRunLabel({ routines: [cron('UTC', { enabled: false })] }, now)).toBe('Manual')
  expect(nextRunLabel({ paused_reason: 'budget', routines: [cron('UTC')] }, now)).toBe('Paused')
})
