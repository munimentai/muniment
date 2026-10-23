import { describe, expect, it } from 'vitest'
import { listedThreads, readThreadOrganization } from './thread-organization.js'

describe('thread organization', () => {
  it('recovers from invalid storage and rejects non-boolean choices', () => {
    expect(readThreadOrganization({ getItem: () => '{' })).toEqual({})
    expect(readThreadOrganization({ getItem: () => JSON.stringify({ a: null, b: { pinned: 'false', archived: true } }) }))
      .toEqual({ b: { pinned: false, archived: true } })
  })
})

describe('listed threads', () => {
  const summaries = [
    { threadId: 'chat', title: 'Plain chat' },
    { threadId: 'artifact', title: 'New Year Countdown' },
    { threadId: 'creation', title: 'Research brief' },
    { threadId: 'agent', title: 'Agent conversation' },
    { threadId: 'group', title: 'Group' },
  ]

  it('keeps chats that saved an artifact or created an agent and leaves out agent and group conversations', () => {
    const listed = listedThreads(summaries, {
      agentThreads: { creation: 'agent-1', agent: 'agent-1' },
      groupThreads: ['group'],
      creations: [{ threadId: 'artifact', kind: 'artifact' }, { threadId: 'creation', kind: 'agent' }],
    })
    expect(listed.map((thread) => thread.threadId)).toEqual(['chat', 'artifact', 'creation'])
  })

  it('does not hide a fresh install\'s first thread once core records its artifact', () => {
    const before = listedThreads(summaries.slice(0, 2))
    const after = listedThreads(summaries.slice(0, 2), { creations: [{ threadId: 'artifact', kind: 'artifact', resultId: 'a1' }] })
    expect(after).toEqual(before)
  })
})
