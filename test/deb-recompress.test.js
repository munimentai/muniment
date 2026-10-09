import { it, expect } from 'vitest'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { recompressDeb } from '../scripts/recompress-deb-linux.mjs'

it('recompresses only the data member and keeps the member order', () => {
  const directory = mkdtempSync(join(tmpdir(), 'deb-recompress-'))
  try {
    const deb = join(directory, 'muniment.deb')
    writeFileSync(deb, 'gzip package')
    const calls = []
    const run = (command, args, options) => {
      calls.push([command, ...args])
      if (command === 'ar' && args[0] === 't') return { status: 0, stdout: 'debian-binary\ncontrol.tar.gz\ndata.tar.gz\n' }
      if (command === 'ar' && args[0] === 'rcD') writeFileSync(join(options.cwd, args[1].split('/').pop()), 'xz package')
      return { status: 0, stdout: '' }
    }
    recompressDeb(deb, run)
    expect(calls.map(call => call.slice(0, 2).join(' '))).toEqual(['ar t', 'ar x', 'gzip -d', 'xz -6', 'ar rcD'])
    expect(calls.at(-1).slice(-3)).toEqual(['debian-binary', 'control.tar.gz', 'data.tar.xz'])
    expect(readFileSync(deb, 'utf8')).toBe('xz package')
    const unexpected = () => ({ status: 0, stdout: 'debian-binary\ncontrol.tar.zst\ndata.tar.zst\n' })
    expect(() => recompressDeb(deb, unexpected)).toThrow('Unexpected package members')
  } finally {
    rmSync(directory, { recursive: true, force: true })
  }
})
