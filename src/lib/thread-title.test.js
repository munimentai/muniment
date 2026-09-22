import { describe, expect, it } from 'vitest'

import { threadTitle, creationTitle } from './thread-title.js'

describe('thread title', () => {
  it('uses a short first user prompt', () => {
    expect(threadTitle([{ role: 'user', text: 'Review the lease renewal' }])).toBe('Review the lease renewal')
  })

  it('limits a long prompt to 80 Unicode scalars including the ellipsis', () => {
    const title = threadTitle([{ role: 'user', text: '😀'.repeat(81) }])

    expect(Array.from(title)).toHaveLength(80)
    expect(title).toBe(`${'😀'.repeat(79)}…`)
  })

  it('collapses whitespace in a prompt', () => {
    expect(threadTitle([{ role: 'user', text: '  Review \n\t the   lease \r renewal  ' }])).toBe('Review the lease renewal')
  })

  it('uses the empty name for an empty transcript', () => {
    expect(threadTitle([])).toBe('New thread')
  })

  it.each([undefined, ' \n\t '])('uses the empty name when the first user prompt has no readable text', (text) => {
    expect(threadTitle([
      { role: 'assistant', text: 'Earlier response' },
      { role: 'user', text },
      { role: 'user', text: 'A later prompt' },
    ])).toBe('New thread')
  })
})

describe('creation titles', () => {
  it.each(['agent', 'artifact'])('uses the shared short thread name for a pending %s', kind => {
    const creation = {kind, threadId:'draft', goal:'Create a polished dashboard with charts and a checklist'}
    expect(creationTitle(creation, [{threadId:'draft', title:'Launch readiness'}])).toBe('Launch readiness')
    expect(creationTitle(creation, [{threadId:'draft', title:creation.goal}])).toBe(`New ${kind}`)
  })
  it('keeps an explicit published name and a short renamed draft', () => {
    expect(creationTitle({kind:'artifact', goal:'Draft report'})).toBe('Draft report')
    expect(creationTitle({kind:'artifact'}, [], 'Launch readiness')).toBe('Launch readiness')
  })
})
