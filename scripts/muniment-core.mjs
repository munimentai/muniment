// Locates the core tree in this repository and reads its runtime pins. The
// shared crates and pins/pins.toml live in core/.
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

// Test environments that replace the global URL still pass import.meta.url as a string.
const repository = dirname(dirname(fileURLToPath(import.meta.url)))

// MUNIMENT_CORE_ROOT names another core tree, such as a test fixture.
export function coreRoot() {
  return process.env.MUNIMENT_CORE_ROOT || join(repository, 'core')
}

// The installed E2E runners export this path before they isolate the spec home.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  console.log(coreRoot())
}

export const corePath = (...parts) => join(coreRoot(), ...parts)

// Reads the TOML subset pins.toml uses: tables, arrays of tables, and string or
// integer values.
export function parsePins(text) {
  const pins = {}
  let table = pins
  for (const raw of text.split('\n')) {
    const line = raw.replace(/#.*$/, '').trim()
    if (!line) continue
    let match
    if ((match = line.match(/^\[\[([\w.]+)\]\]$/))) {
      const keys = match[1].split('.')
      const parent = keys.slice(0, -1).reduce((node, key) => (node[key] ??= {}), pins)
      table = {}
      ;(parent[keys.at(-1)] ??= []).push(table)
    } else if ((match = line.match(/^\[([\w.]+)\]$/))) {
      table = match[1].split('.').reduce((node, key) => (node[key] ??= {}), pins)
    } else if ((match = line.match(/^(\w+) = (?:"([^"]*)"|(\d+))$/))) {
      table[match[1]] = match[2] ?? Number(match[3])
    } else {
      throw new Error(`Unsupported pins.toml line: ${raw}`)
    }
  }
  return pins
}

export function readCorePins() {
  return parsePins(readFileSync(corePath('pins', 'pins.toml'), 'utf8'))
}
