import { expect, it } from 'vitest'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
// A search field offers no autofill history: the list covers the rows under it.
function files(dir) { return readdirSync(dir).flatMap((name) => { const path = join(dir, name); return statSync(path).isDirectory() ? (name === 'node_modules' ? [] : files(path)) : path.endsWith('.svelte') ? [path] : [] }) }
it('turns autofill off on every search field', () => {
  const missing = files('src').flatMap((path) => (readFileSync(path, 'utf8').match(/<input[^>]*type="search"[^>]*>/g) ?? []).filter((tag) => !tag.includes('autocomplete="off"')).map((tag) => `${path}: ${tag.slice(0, 60)}`))
  expect(missing).toEqual([])
})
