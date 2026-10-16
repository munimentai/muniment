import { expect, it } from 'vitest'
import { groupMembers, groupRequest, groupRuntime } from './groups.js'

it('creates a group on this computer when it is in the swarm', () => {
  expect(groupRuntime({ self: { box: 'b', local: 'l' } })).toBe('local')
  expect(groupRuntime({ self: { box: 'b' } })).toBe('box')
  expect(groupRuntime({ self: {} })).toBeNull()
})

it('builds the create request with a lead among the members and capped rounds', () => {
  expect(groupRequest({ name: ' Launch room ', members: ['a', 'b'], lead: 'b', meetingMode: true, debateRounds: 5 })).toEqual({ body: { name: 'Launch room', members: ['a', 'b'], lead: 'b', meeting_mode: true, debate_rounds: 2 } })
  expect(groupRequest({ name: 'Chat', members: ['a', 'b', 'c'], lead: 'z' })).toEqual({ body: { name: 'Chat', members: ['a', 'b', 'c'], lead: 'a', meeting_mode: false } })
  expect(groupRequest({ name: 'Chat', members: ['a', 'b'], meetingMode: true, debateRounds: 0 }).body.debate_rounds).toBe(0)
})

it('refuses a draft without a name or with too few or too many agents', () => {
  expect(groupRequest({ name: '', members: ['a', 'b'] }).error).toBe('Name the group.')
  expect(groupRequest({ name: 'x', members: ['a'] }).error).toBe('Choose at least 2 agents.')
  expect(groupRequest({ name: 'x', members: ['a', 'b', 'c', 'd', 'e', 'f', 'g'] }).error).toBe('Choose at most 6 agents.')
})

it('lists a group\'s members from the roster', () => {
  expect(groupMembers({ members: ['b', 'x'] }, [{ id: 'b', name: 'Deal desk' }])).toEqual([{ id: 'b', name: 'Deal desk' }, { id: 'x', name: 'Unknown agent' }])
})
