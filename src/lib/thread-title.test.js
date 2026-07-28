import { describe, expect, it } from 'vitest'

import { threadTitle } from './thread-title.js'

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
