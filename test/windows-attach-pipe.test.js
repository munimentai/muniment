import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { describe, expect, it } from 'vitest'

import {
  readWindowsAttachPipePath,
  windowsAttachPipeNameFile,
  windowsAttachPipePrefix,
} from './e2e/support/windows-attach-pipe.mjs'

const sid = 'S-1-5-21-3623811015-3361044348-30300820-1013'
const suffix = '0123456789abcdef0123456789abcdef'

function withPipeNameFile(contents, check) {
  const localAppData = mkdtempSync(join(tmpdir(), 'muniment-pipe-'))
  try {
    const file = windowsAttachPipeNameFile(localAppData)
    mkdirSync(dirname(file), { recursive: true })
    writeFileSync(file, contents)
    check(localAppData)
  } finally {
    rmSync(localAppData, { recursive: true, force: true })
  }
}

describe('Windows attach pipe path', () => {
  it('derives the pipe prefix from a canonical SID', () => {
    expect(windowsAttachPipePrefix(sid))
      .toBe('\\\\.\\pipe\\Muniment\\attach-v1-72c8a69c4fadaa23e7baa5ff00d1aa92-')
  })

  it('reads the stored pipe path', () => {
    const path = `${windowsAttachPipePrefix(sid)}${suffix}`
    withPipeNameFile(`${path}\n`, (localAppData) => {
      expect(readWindowsAttachPipePath(sid, localAppData)).toBe(path)
    })
  })

  it.each([
    '',
    `${windowsAttachPipePrefix('S-1-5-18')}${suffix}`,
    `${windowsAttachPipePrefix(sid)}${suffix.toUpperCase()}`,
    `${windowsAttachPipePrefix(sid)}${suffix}0`,
  ])('rejects the stored path %j', (contents) => {
    withPipeNameFile(contents, (localAppData) => {
      expect(() => readWindowsAttachPipePath(sid, localAppData))
        .toThrow('The Windows attach pipe name file is invalid.')
    })
  })

  it.each([
    '',
    's-1-5-21',
    'S-2-5-21',
    'S-1-5',
    'S-1-5-',
    'S-1-5-abc',
  ])('rejects the invalid SID %j', (invalid) => {
    expect(() => windowsAttachPipePrefix(invalid)).toThrow('The Windows SID is invalid.')
  })
})
