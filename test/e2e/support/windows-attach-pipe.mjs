import { createHash } from 'node:crypto'
import { pathToFileURL } from 'node:url'

const canonicalSidPattern = /^S-1-\d+(-\d+)+$/

export function windowsAttachPipePath(sid) {
  if (typeof sid !== 'string' || !canonicalSidPattern.test(sid)) {
    throw new TypeError('The Windows SID is invalid.')
  }

  const userHash = createHash('sha256').update(sid.toUpperCase()).digest('hex').slice(0, 32)
  return `\\\\.\\pipe\\Muniment\\attach-v1-${userHash}`
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.stdout.write(`${windowsAttachPipePath(process.argv[2])}\n`)
}
