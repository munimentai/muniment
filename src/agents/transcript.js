// How agent traffic reads in a transcript. Core wakes an agent with a prompt
// that wraps another agent's message or handoff in an untrusted block, posts
// group replies and delivery notes to threads, and records a routine run
// that had nothing to say as `NO_RESPONSE`. These mappings only shape what
// core wrote for display.

const ATTRIBUTE = /(\w+)="([^"]*)"/g

function attributes(text) {
  return Object.fromEntries([...text.matchAll(ATTRIBUTE)].map(([, name, value]) => [name, value]))
}

function block(text, tag) {
  const match = new RegExp(`<${tag}\\b([^>]*)>\\n?([\\s\\S]*?)\\n?</${tag}>`).exec(text ?? '')
  if (!match) return null
  return { attributes: attributes(match[1]), body: match[2].replaceAll(`&lt;/${tag}`, `</${tag}`) }
}

const hop = (value) => {
  const number = Number.parseInt(value, 10)
  return Number.isFinite(number) ? number : 0
}

// The agent message or handoff a run's prompt carries, or null for a prompt
// a person wrote.
export function agentEnvelope(prompt) {
  const message = block(prompt, 'agent_message')
  if (message) {
    const { from, agent_id: agentId, intent, message_id: messageId, reply_to: replyTo } = message.attributes
    return { kind: 'message', from: from || 'An agent', agentId: agentId || null, intent: intent || 'fyi', hop: hop(message.attributes.hop), messageId: messageId || null, replyTo: replyTo || null, text: message.body }
  }
  const handoff = block(prompt, 'agent_handoff')
  if (handoff) {
    const { from, agent_id: agentId, message_id: messageId } = handoff.attributes
    const [task, ...context] = handoff.body.split('\n\nContext:\n')
    return { kind: 'handoff', from: from || 'An agent', agentId: agentId || null, intent: 'handoff', hop: hop(handoff.attributes.hop), messageId: messageId || null, text: task, context: context.join('\n\nContext:\n') }
  }
  return null
}

const INTENTS = { request: 'Request', question: 'Question', result: 'Result', status: 'Status', fyi: 'FYI', handoff: 'Handoff' }

export function intentLabel(intent) {
  return INTENTS[intent] ?? 'Message'
}

export function hopLabel(count) {
  return `${count} ${count === 1 ? 'hop' : 'hops'}`
}

// A settled run whose whole reply is the silent marker.
export function silentRun(run) {
  return run?.phase === 'complete' && String(run.text ?? '').trim() === 'NO_RESPONSE'
}

// The posts that belong in the transcript. A user post a run consumed shows
// as that run's prompt, so only the others remain.
export function visiblePosts(posts = []) {
  return posts.filter((post) => post.author?.kind !== 'user' || !post.consumed)
}

const at = (value) => {
  const time = Date.parse(value ?? '')
  return Number.isFinite(time) ? time : null
}

// Messages and posts in one order. A post goes before the first message
// that started after it, and posts keep their own order.
export function timeline(messages = [], posts = []) {
  const items = []
  const queue = [...posts]
  for (const message of messages) {
    const started = at(message.sentAt ?? message.run?.startedAt)
    while (queue.length && started !== null && at(queue[0].posted_at) !== null && at(queue[0].posted_at) < started) items.push({ post: queue.shift() })
    items.push({ message })
  }
  for (const post of queue) items.push({ post })
  return items
}

// The author line of a post: the agent's directory entry when known.
export function postAuthor(post, agents = []) {
  const author = post?.author ?? {}
  if (author.kind === 'agent') {
    const agent = agents.find((entry) => entry.id === author.agent_id || entry.agent_id === author.agent_id)
    return { kind: 'agent', name: author.name || agent?.name || 'An agent', agent: agent ?? { id: author.agent_id, name: author.name } }
  }
  return { kind: author.kind === 'system' ? 'system' : 'user', name: author.kind === 'system' ? 'Muniment' : 'You' }
}

// Labels a meeting's rounds in a group thread. After each message from the
// user, each member's first reply is the blind round, its next replies are
// the debate rounds, and the lead's reply after those is the summary. Posts
// carry no round, so the count per member places each reply.
export function meetingSections(posts = [], group = null) {
  if (!group?.meeting_mode) return posts.map((post) => ({ post, section: null }))
  const debate = group.debate_rounds ?? 2
  let counts = null
  let last = null
  return posts.map((post) => {
    const kind = post.author?.kind
    if (kind === 'user') { counts = new Map(); last = null; return { post, section: null } }
    if (kind !== 'agent' || !counts) return { post, section: null }
    const id = post.author.agent_id
    const round = (counts.get(id) ?? 0) + 1
    counts.set(id, round)
    const label = round === 1 ? 'Round 1 · blind' : round <= debate + 1 ? `Debate round ${round - 1}` : 'Summary'
    const section = label === last ? null : label
    last = label
    return { post, section }
  })
}
