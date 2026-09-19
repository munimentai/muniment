import { describe, expect, it } from 'vitest'
import { organizeThreads, readThreadOrganization } from './thread-organization.js'

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
