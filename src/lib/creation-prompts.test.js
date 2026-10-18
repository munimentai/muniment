import { expect, it } from 'vitest'
import { creationDefaults, creationName, creationPrompt, creationStarters, sentCreation } from './creation-prompts.js'

// The scaffold an older version sent, which history still holds.
const scaffold = (kind, plan) => `I want to create ${kind === 'agent' ? 'an agent' : 'an artifact'} in this dedicated chat. Goal: ${plan.goal}. Expected output: ${plan.output}. Ask a few focused questions about any missing requirements, then build it and save it in Muniment. Keep this chat for future revisions.`

it('sends the user words, or the goal when the user typed none, and never the scaffold', () => {
  const page = { goal: 'Make a web page', output: 'A self-contained HTML page' }
  expect(creationPrompt('artifact', page, '  A countdown to New Year with big colorful digits.  ')).toBe('A countdown to New Year with big colorful digits.')
  expect(creationPrompt('artifact', page, '   ')).toBe('Make a web page')
  expect(sentCreation(creationPrompt('artifact', page, 'A countdown'))).toBeNull()
})

it('reads an older sent creation scaffold back as its template and the user details', () => {
  const desk = creationStarters.agent.find((starter) => starter.id === 'research-desk')
  expect(sentCreation(`${scaffold('agent', desk)}\n\n  Focus on biology  `)).toEqual({ kind: 'agent', goal: desk.goal, output: desk.output, name: 'Research desk', details: 'Focus on biology' })
  expect(sentCreation(scaffold('artifact', creationDefaults('artifact')))).toMatchObject({ kind: 'artifact', name: 'New artifact', details: '' })
  expect(sentCreation('Summarize the lease')).toBeNull()
  expect(creationName('agent', { goal: 'Something else' })).toBe('New agent')
})

it('gives every agent template one sentence the builder drafts from', () => {
  for (const starter of creationStarters.agent) expect(starter.description).toMatch(/^[A-Z].+\.$/)
})
