import { describe, expect, it } from 'vitest'
import { listedThreads, organizeThreads, readThreadOrganization } from './thread-organization.js'

describe('thread organization', () => {
  const summaries = [
    { threadId: 'recent', title: 'Lease renewal' },
    { threadId: 'archive', title: 'Lease archive' },
    { threadId: 'pin', title: 'Vendor terms' },
  ]
  const choices = { archive: { archived: true, pinned: true }, pin: { pinned: true } }

  it('orders visible rows for selection and shortcuts without archived rows', () => {
    expect(organizeThreads(summaries, choices, '', false).flatMap((group) => group.threads.map((thread) => thread.threadId)))
      .toEqual(['pin', 'recent'])
  })

  it('searches archives and titles without case or outer whitespace sensitivity', () => {
    expect(organizeThreads(summaries, choices, ' LEASE ', false).map((group) => group.name)).toEqual(['Threads', 'Archived'])
    expect(organizeThreads(summaries, choices, '', true)).toEqual([{ name: 'Archived', threads: [summaries[1]] }])
  })

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
