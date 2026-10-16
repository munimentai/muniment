// Group chats: the request that creates one and the machine that owns it.
// Core checks the members, the lead and the rounds again.

export const MIN_MEMBERS = 2
export const MAX_MEMBERS = 6
export const MAX_DEBATE_ROUNDS = 2

// The machine a new group lives on: this computer when it is in the swarm,
// or else the first machine the app reaches that is.
export function groupRuntime(swarm) {
  const runtimes = Object.keys(swarm?.self ?? {})
  return runtimes.includes('local') ? 'local' : runtimes[0] ?? null
}

// The `group.create` body for a draft, or the reason it cannot be sent.
export function groupRequest({ name = '', members = [], lead = '', meetingMode = false, debateRounds = MAX_DEBATE_ROUNDS }) {
  const trimmed = name.trim()
  if (!trimmed) return { error: 'Name the group.' }
  if (members.length < MIN_MEMBERS) return { error: `Choose at least ${MIN_MEMBERS} agents.` }
  if (members.length > MAX_MEMBERS) return { error: `Choose at most ${MAX_MEMBERS} agents.` }
  const chosenLead = members.includes(lead) ? lead : members[0]
  return {
    body: {
      name: trimmed,
      members: [...members],
      lead: chosenLead,
      meeting_mode: !!meetingMode,
      ...(meetingMode ? { debate_rounds: Math.max(0, Math.min(MAX_DEBATE_ROUNDS, Number(debateRounds) || 0)) } : {}),
    },
  }
}

// The members of a group as roster agents, in the group's order.
export function groupMembers(group, roster = []) {
  return (group?.members ?? []).map((id) => roster.find((agent) => agent.id === id) ?? { id, name: 'Unknown agent' })
}
