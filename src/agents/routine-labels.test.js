import { expect, it } from 'vitest'
import { isProposal, lastFiredLabel, nextRunLabel, pollLabel, triggerSummary } from './routine-labels.js'
import { runTimeLabel } from '../lib/agent-labels.js'

it('names each trigger kind with its schedule or sender', () => {
  expect(triggerSummary({ kind: 'cron', expr: '0 9 * * 1-5', tz: 'America/New_York' })).toBe('Weekdays at 09:00 · America/New_York')
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

it('names when a routine runs with no time zone, in local time for another zone', async () => {
  const { routineIcon, routineWhen } = await import('./routine-labels.js')
  const { localTimeZone } = await import('./cron.js')
  const { runTimeLabel } = await import('../lib/agent-labels.js')
  expect(routineWhen({ trigger: { kind: 'cron', expr: '0 9 * * 1-5', tz: localTimeZone() } })).toBe('Weekdays at 09:00')
  const zone = localTimeZone() === 'Asia/Tokyo' ? 'Europe/Paris' : 'Asia/Tokyo'
  const next = Math.floor(Date.now() / 1000) + 3600
  expect(routineWhen({ trigger: { kind: 'cron', expr: '0 9 * * *', tz: zone }, next_fire_at: next })).toBe(`Next run ${runTimeLabel(next * 1000).replace(/^T/, 't')}`)
  expect(routineWhen({ trigger: { kind: 'cron', expr: '0 9 * * *', tz: zone } })).toBe('Every day at 09:00')
  expect(routineWhen({ trigger: { kind: 'webhook', preset: 'github' } })).toBe('Webhook · GitHub')
  expect(routineWhen({ trigger: { kind: 'mcp', server: 'crm', tool: 'search', every: { expr: '*/15 * * * *', tz: 'UTC' } } })).toBe('crm search · every 15 minutes')
  expect(['cron', 'webhook', 'mcp'].map((kind) => routineIcon({ kind }))).toEqual(['clock', 'link', 'plug-zap'])
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
