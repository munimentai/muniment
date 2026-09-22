import anthropic from './anthropic-catalog.json'
import automaticRegistration from './automatic-registration.json'
// Discovery metadata is not a connection proxy. Never offer catalog-hosted relays.
export function providerDestination(value) {
  try {
    const url = new URL(value)
    const host = url.hostname.toLowerCase().replace(/\.$/, '')
    return ['https:', 'http:'].includes(url.protocol) && !url.username && !url.password
      && !['anthropic.com', 'claude.com', 'claude.ai', 'example-server.modelcontextprotocol.io'].some(domain => host === domain || host.endsWith(`.${domain}`))
  } catch { return false }
}
// Bind eligibility to the inspected endpoint. New or changed endpoints require another check.
export const catalog = anthropic.filter(entry => entry.type === 'remote' && providerDestination(entry.url)
  && automaticRegistration[entry.id] === entry.url)
  .map(entry => ({ ...entry, publisher: /anthropic/i.test(entry.publisher) ? '' : entry.publisher,
    website: providerDestination(entry.website) ? entry.website : entry.url ? new URL(entry.url).origin : '',
    categories: entry.categories?.length ? entry.categories : ['other'] }))
export const categoryLabel = value => value.split('-').map(word => word[0]?.toUpperCase() + word.slice(1)).join(' ')
export const categories = [...new Set(catalog.flatMap(entry => entry.categories))].sort()
export function filterCatalog(entries, { query = '', category = '', type = '', installed = false, setup = false } = {}, installedSources = new Set()) {
  const normalize = value => String(value || '').normalize('NFKD').replace(/\p{M}/gu, '').toLowerCase().replace(/[^\p{L}\p{N}]+/gu, ' ').trim()
  const words = normalize(query).split(/\s+/).filter(Boolean)
  return entries.filter(entry => (!category || entry.categories.includes(category)) && (!type || entry.type === type)
    && (!installed || installedSources.has(entry.source)) && (!setup || !entry.url)
    && words.every(word => normalize([entry.name, entry.publisher, entry.description, ...(entry.categories || []), entry.url].filter(Boolean).join(' ')).includes(word)))
}
export function sortCatalog(entries, sort = 'popular') {
  return [...entries].sort((a, b) => (sort === 'popular' ? (b.popularity || 0) - (a.popularity || 0) : 0)
    || (sort === 'name-desc' ? b.name.localeCompare(a.name) : a.name.localeCompare(b.name)))
}
export { invocations, invocationQuery, commandName, commandChoices, selectedCommands } from './commands.js'
