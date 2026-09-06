import path from 'node:path'
import { homedir } from 'node:os'
import { fileURLToPath } from 'node:url'

// This matches normalizePath in Pi v0.85.1 utils/paths.ts with its default options.
export function piAgentDirectory(configured = process.env.PI_CODING_AGENT_DIR, home = homedir(), windows = process.platform === 'win32') {
  const paths = windows ? path.win32 : path.posix
  if (!configured) return paths.join(home, '.pi', 'agent')
  let value = configured
  if (windows && value.startsWith('/') && !value.startsWith('//') && !value.includes('\\')) {
    const match = value.match(/^\/(?:mnt\/|cygdrive\/)?([a-z])(?:\/(.*))?$/i)
    if (match) value = `${match[1].toUpperCase()}:\\${match[2]?.replaceAll('/', '\\') ?? ''}`
  }
  if (value === '~') return home
  if (value.startsWith('~/') || (windows && value.startsWith('~\\'))) return paths.join(home, value.slice(2))
  if (value.startsWith('file://')) return fileURLToPath(value, { windows })
  return value
}
