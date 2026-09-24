import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'

const canonicalSidPattern = /^S-1-\d+(-\d+)+$/
const suffixPattern = /^[0-9a-f]{32}$/

export function windowsAttachPipePrefix(sid) {
  if (typeof sid !== 'string' || !canonicalSidPattern.test(sid)) {
    throw new TypeError('The Windows SID is invalid.')
  }

  const userHash = createHash('sha256').update(sid.toUpperCase()).digest('hex').slice(0, 32)
  return `\\\\.\\pipe\\Muniment\\attach-v1-${userHash}-`
}

export function windowsAttachPipeNameFile(localAppData) {
  return join(localAppData, 'ai.muniment.desktop', 'attach-pipe-name')
}

// The runtime stores the pipe path, with its random suffix, in an owner-only file.
export function readWindowsAttachPipePath(sid, localAppData) {
  const prefix = windowsAttachPipePrefix(sid)
  const path = readFileSync(windowsAttachPipeNameFile(localAppData), 'utf8').trim()
  if (!path.startsWith(prefix) || !suffixPattern.test(path.slice(prefix.length))) {
    throw new TypeError('The Windows attach pipe name file is invalid.')
  }
  return path
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.stdout.write(`${readWindowsAttachPipePath(process.argv[2], process.argv[3])}\n`)
}
