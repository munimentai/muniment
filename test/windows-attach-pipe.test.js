import { describe, expect, it } from 'vitest'

import { windowsAttachPipePath } from './e2e/support/windows-attach-pipe.mjs'

describe('Windows attach pipe path', () => {
  it('derives the pipe path from a canonical SID', () => {
    expect(windowsAttachPipePath('S-1-5-21-3623811015-3361044348-30300820-1013'))
      .toBe('\\\\.\\pipe\\Muniment\\attach-v1-72c8a69c4fadaa23e7baa5ff00d1aa92')
  })

  it.each([
    '',
    's-1-5-21',
    'S-2-5-21',
    'S-1-5',
    'S-1-5-',
    'S-1-5-abc',
  ])('rejects the invalid SID %j', (sid) => {
    expect(() => windowsAttachPipePath(sid)).toThrow('The Windows SID is invalid.')
  })
})
