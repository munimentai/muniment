import { expect, it } from 'vitest'
import { isProposal, lastFiredLabel, pollLabel, triggerSummary } from './routine-labels.js'

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
