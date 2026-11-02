import { expect, it } from 'vitest'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
// A search field offers no autofill history: the list covers the rows under it.
// macOS autocorrect and spelling never change a query as it is typed, so the
// first click on a result picks it and does not accept a correction.
function files(dir) { return readdirSync(dir).flatMap((name) => { const path = join(dir, name); return statSync(path).isDirectory() ? (name === 'node_modules' ? [] : files(path)) : path.endsWith('.svelte') ? [path] : [] }) }
const OFF = ['autocomplete="off"', 'spellcheck="false"', 'autocorrect="off"', 'autocapitalize="off"']
const searchTags = (source) => source.match(/<input[^>]*(?:type="search"|aria-label="Search[^"]*"|aria-label="Filter[^"]*")[^>]*>/g) ?? []
it('turns autofill, autocorrect and spelling off on every search field', () => {
  const missing = files('src').flatMap((path) => searchTags(readFileSync(path, 'utf8')).flatMap((tag) => OFF.filter((attribute) => !tag.includes(attribute)).map((attribute) => `${path}: ${attribute} ${tag.slice(0, 60)}`)))
  expect(missing).toEqual([])
})
it('finds the search fields a person types a query into', () => {
  const tags = files('src').flatMap((path) => searchTags(readFileSync(path, 'utf8')))
  for (const label of ['Search settings', 'Search threads', 'Search models', 'Filter files', 'Search threads, pages and actions']) expect(tags.some((tag) => tag.includes(`aria-label="${label}"`))).toBe(true)
  expect(searchTags('<input type="search" autocomplete="off">')).toHaveLength(1)
})
