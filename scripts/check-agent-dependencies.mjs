import { readFileSync } from 'node:fs'
import { pathToFileURL } from 'node:url'
import semver from 'semver'

const root = new URL('../', import.meta.url)
export function readPins() {
  const read = path => readFileSync(new URL(path, root), 'utf8')
  const packages = Object.fromEntries([...read('src-tauri/core/src/pi_packages.rs').split('];')[0]
    .matchAll(/\("(pi-[^"]+)", "([^"]+)"\)/g)].map(match => [match[1], match[2]]))
  const pi = read('src-tauri/core/src/sidecar/pi_install.rs').match(/PI_RELEASE_BASE:.*\/v([\d.]+)"/)[1]
  const claude = read('src-tauri/core/src/model_router/transport.rs').match(/claude-cli\/([\d.]+)/)[1]
  return { pi, packages, claude }
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
