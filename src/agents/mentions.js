// @mention suggestions. In a group thread core wakes the agents a message
// names as `@name`, every member for `@everyone`, and the first member when
// it names none. In any other thread the composer sends the ids of the
// agents the message still names, and core sends the message to each.

export const EVERYONE = { id: 'everyone', name: 'everyone', everyone: true }

// The members whose name starts with the query, or has a word that does,
// after `@everyone` unless `everyone` is false. Case does not matter.
export function agentMentions(query, agents = [], limit = 8, { everyone = true } = {}) {
  const wanted = String(query ?? '').toLowerCase()
  const matches = (name) => {
    const lower = String(name ?? '').toLowerCase()
    return !wanted || lower.startsWith(wanted) || lower.split(/\s+/).some((word) => word.startsWith(wanted))
  }
  const seen = new Set()
  const found = agents.filter((agent) => agent?.name && !seen.has(agent.id) && seen.add(agent.id) && matches(agent.name))
  return [...(everyone && matches(EVERYONE.name) ? [EVERYONE] : []), ...found].slice(0, limit)
}

// The ids of the chosen agents whose `@name` the text still holds, once each.
export function mentionedAgentIds(text, chosen = []) {
  const value = String(text ?? '')
  return [...new Set(chosen.filter((agent) => agent?.id && value.includes(`@${agent.name}`)).map((agent) => agent.id))]
}

// The draft with the `@query` at `mention` replaced by the agent's name.
export function insertAgentMention(text, mention, agent) {
  const token = `@${agent.name} `
  return { text: text.slice(0, mention.start) + token + text.slice(mention.end), cursor: mention.start + token.length }
}
