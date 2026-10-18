import fs from 'node:fs'
import path from 'node:path'
import { expect, it } from 'vitest'
import { activityLabel, activityState, agentActivity, applyActivity } from './activity.js'

const event = JSON.parse(fs.readFileSync(path.join(process.cwd(), 'core/protocol-fixtures/muniment.attach/1/event-agent-activity.json'), 'utf8')).body

it('names each state and its action', () => {
  expect(activityLabel(event.activity)).toBe('Waiting for you: Run git push origin main in Lead-scout')
  expect(activityLabel({ state: 'running', action: 'Waits to start' })).toBe('Running: Waits to start')
  expect(activityLabel({ state: 'error' })).toBe('Last run failed')
  expect(activityLabel({ state: 'idle', since: null })).toBe('Idle')
  expect(activityState({ state: 'sleeping' })).toBe('idle')
  expect(activityState(undefined)).toBe('idle')
})

it('keeps the newest report per agent from each agent.activity event', () => {
  const live = applyActivity({}, { runtimeId: 'local', update: event })
  expect(live[event.agent_id]).toEqual(event.activity)
  const older = { ...event, activity: { state: 'running', action: 'Runs on request', since: '2025-12-31T00:00:00Z' } }
  expect(applyActivity(live, { update: older })).toBe(live)
  const newer = { ...event, activity: { state: 'idle', since: '2026-01-02T00:00:00Z' } }
  expect(applyActivity(live, { update: newer })[event.agent_id].state).toBe('idle')
  expect(applyActivity(live, { update: { agent_id: 'x' } })).toBe(live)
})

it('prefers the live report, then the listing, then the swarm directory', () => {
  const agent = { id: 'a', activity: { state: 'error' } }
  const directory = [{ agent_id: 'a', activity: { state: 'running' } }, { agent_id: 'b', activity: { state: 'waiting' } }]
  expect(agentActivity(agent, { a: { state: 'idle' } }, directory).state).toBe('idle')
  expect(agentActivity(agent, {}, directory).state).toBe('error')
  expect(agentActivity({ id: 'b' }, {}, directory).state).toBe('waiting')
  expect(agentActivity({ id: 'c' }, {}, directory)).toBeUndefined()
})
