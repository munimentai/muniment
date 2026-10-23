import { expect, it } from 'vitest'
import { EVERYONE, agentMentions, insertAgentMention, mentionedAgentIds } from './mentions.js'
import { mentionQuery } from '../composer/composer-references.js'

const agents = [{ id: 'a', name: 'Lead scout' }, { id: 'b', name: 'Deal desk' }, { id: 'c', name: 'Writer' }, { id: 'a', name: 'Lead scout' }]

it('offers everyone and each member once, filtered by name or word, ignoring case', () => {
  expect(agentMentions('', agents).map((agent) => agent.name)).toEqual(['everyone', 'Lead scout', 'Deal desk', 'Writer'])
  expect(agentMentions('d', agents).map((agent) => agent.name)).toEqual(['Deal desk'])
  expect(agentMentions('DESK', agents).map((agent) => agent.name)).toEqual(['Deal desk'])
  expect(agentMentions('ev', agents)).toEqual([EVERYONE])
  expect(agentMentions('zz', agents)).toEqual([])
  expect(agentMentions('', agents, 2)).toHaveLength(2)
})

it('replaces the typed query with the full name', () => {
  const draft = 'Ask @dea about it'
  const mention = mentionQuery(draft, 8)
  expect(mention).toEqual({ query: 'dea', start: 4, end: 8 })
  expect(insertAgentMention(draft, mention, { name: 'Deal desk' })).toEqual({ text: 'Ask @Deal desk  about it', cursor: 15 })
  expect(insertAgentMention('@', { start: 0, end: 1 }, EVERYONE)).toEqual({ text: '@everyone ', cursor: 10 })
})

it('leaves out everyone outside a group and sends the ids of the agents the text still names', () => {
  expect(agentMentions('', agents, 8, { everyone: false }).map((agent) => agent.name)).toEqual(['Lead scout', 'Deal desk', 'Writer'])
  const chosen = [{ id: 'b', name: 'Deal desk' }, { id: 'c', name: 'Writer' }, { id: 'b', name: 'Deal desk' }]
  expect(mentionedAgentIds('@Deal desk price it, @Deal desk again', chosen)).toEqual(['b'])
  expect(mentionedAgentIds('No names left', chosen)).toEqual([])
})
