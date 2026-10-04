import { pathToFileURL } from 'node:url'
import semver from 'semver'
import { readCorePins } from './muniment-core.mjs'

// The pins come from pins/pins.toml in the muniment-core release Cargo resolves.
export function readPins() {
  const pins = readCorePins()
  const packages = Object.fromEntries(pins.packages.map(({ name, version }) => [name, version]))
  return { pi: pins.pi.version, packages, claude: pins.claude_code.version }
}

export function assess(name, pinned, manifest, pi) {
  if (!semver.valid(manifest.version) || manifest.name !== name) throw new Error(`Invalid registry metadata for ${name}`)
  const incompatible = Object.entries(manifest.peerDependencies ?? {})
    .filter(([peer, range]) => peer.startsWith('@earendil-works/pi-') && !semver.satisfies(pi, range))
    .map(([peer, range]) => `${peer} requires ${range}`)
  return { name, pinned, latest: manifest.version,
    status: incompatible.length ? 'peer review required' : semver.gt(manifest.version, pinned) ? 'update required' : 'current',
    incompatible }
}

export async function audit(fetcher = fetch) {
  const { pi, packages, claude } = readPins()
  const pins = { '@anthropic-ai/claude-code': claude, '@earendil-works/pi-coding-agent': pi, ...packages }
  return Promise.all(Object.entries(pins).map(async ([name, pinned]) => {
    const response = await fetcher(`https://registry.npmjs.org/${encodeURIComponent(name)}/latest`, { signal: AbortSignal.timeout(15000) })
    if (!response.ok) throw new Error(`${name}: registry HTTP ${response.status}`)
    return assess(name, pinned, await response.json(), pi)
  }))
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const rows = await audit()
    for (const row of rows) console.log(`${row.name}: ${row.pinned} / latest ${row.latest}: ${row.status}${row.incompatible.length ? ` (${row.incompatible.join(', ')})` : ''}`)
    // A current version is not proof of compatibility. Native and installed chat
    // regression checks must pass before any candidate pin can ship.
    if (rows.some(row => row.status !== 'current')) process.exitCode = 1
  } catch (error) {
    console.error(error.message)
    process.exitCode = 1
  }
}
