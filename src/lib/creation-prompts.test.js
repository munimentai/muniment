import { expect, it } from 'vitest'
import { creationDefaults, creationName, creationPrompt, creationStarters, sentCreation } from './creation-prompts.js'

it('reads a sent creation prompt back as its template and the user details', () => {
  const desk = creationStarters.agent.find((starter) => starter.id === 'research-desk')
  expect(sentCreation(creationPrompt('agent', desk, '  Focus on biology  '))).toEqual({ kind: 'agent', goal: desk.goal, output: desk.output, name: 'Research desk', details: 'Focus on biology' })
  expect(sentCreation(creationPrompt('artifact', creationDefaults('artifact')))).toMatchObject({ kind: 'artifact', name: 'New artifact', details: '' })
  expect(sentCreation('Summarize the lease')).toBeNull()
  expect(creationName('agent', { goal: 'Something else' })).toBe('New agent')
})
