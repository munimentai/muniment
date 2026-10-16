import { expect, it } from 'vitest'
import { isProposal, lastFiredLabel, triggerSummary } from './routine-labels.js'

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

it('treats an untouched routine from instructions as a proposal', () => {
  expect(isProposal({ created_from: 'instructions', enabled: false, last_fired_at: null })).toBe(true)
  expect(isProposal({ created_from: 'instructions', enabled: true, last_fired_at: null })).toBe(false)
  expect(isProposal({ created_from: 'user', enabled: false, last_fired_at: null })).toBe(false)
  expect(isProposal({ created_from: 'agent', enabled: false, last_fired_at: '2026-01-01T00:00:00Z' })).toBe(false)
})
