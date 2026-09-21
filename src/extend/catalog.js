import anthropic from './anthropic-catalog.json'
// Discovery metadata is not a connection proxy. Never offer catalog-hosted relays.
export function providerDestination(value) {
  try {
    const url = new URL(value)
    const host = url.hostname.toLowerCase().replace(/\.$/, '')
    return ['https:', 'http:'].includes(url.protocol) && !url.username && !url.password
      && !['anthropic.com', 'claude.com', 'claude.ai', 'example-server.modelcontextprotocol.io'].some(domain => host === domain || host.endsWith(`.${domain}`))
  } catch { return false }
}
export const catalog = anthropic.filter(entry => entry.type === 'remote' && (!entry.url || providerDestination(entry.url)))
  .map(entry => ({ ...entry, publisher: /anthropic/i.test(entry.publisher) ? '' : entry.publisher,
    website: providerDestination(entry.website) ? entry.website : entry.url ? new URL(entry.url).origin : '',
    categories: entry.categories?.length ? entry.categories : ['other'] }))
export const categoryLabel = value => value.split('-').map(word => word[0]?.toUpperCase() + word.slice(1)).join(' ')
export const categories = [...new Set(catalog.flatMap(entry => entry.categories))].sort()
export function filterCatalog(entries, { query = '', category = '', type = '', installed = false, setup = false } = {}, installedSources = new Set()) {
  const words = query.toLowerCase().trim().split(/\s+/).filter(Boolean)
  return entries.filter(entry => (!category || entry.categories.includes(category)) && (!type || entry.type === type)
    && (!installed || installedSources.has(entry.source)) && (!setup || !entry.url)
    && words.every(word => `${entry.name} ${entry.publisher} ${entry.categories.join(' ')} ${entry.url}`.toLowerCase().includes(word)))
}
export function invocations(items) {
  return items.filter(item => item.enabled !== false).flatMap(item => [
    ...(item.kind === 'plugin' ? [{ id: item.id, name: item.name, description: item.description, kind: 'plugin' }] : []),
    ...(item.skills || []).map(skill => ({ id: `${item.id}:${skill.path}`, name: skill.name, description: skill.description, kind: 'skill' })),
  ])
}
export function invocationQuery(text) {
  // Only a leading command token. Paths and code never open this picker.
  const match = text.match(/^[\\/]([\w-]*)$/)
  return match ? match[1].toLowerCase() : null
}

export function sortCatalog(entries, sort = 'popular') {
  return [...entries].sort((a, b) => (sort === 'popular' ? (b.popularity || 0) - (a.popularity || 0) : 0)
    || (sort === 'name-desc' ? b.name.localeCompare(a.name) : a.name.localeCompare(b.name)))
}
export function commandName(item) {
  return item.name.toLowerCase().replace(/[^a-z0-9_-]+/g, '-').replace(/^-|-$/g, '') || item.kind
}
export function commandChoices(items) {
  const used = new Set()
  return invocations(items).map(item => {
    const base = commandName(item)
    let name = base, index = 2
    while (used.has(name)) name = `${base}-${index++}`
    used.add(name)
    return { ...item, command: name }
  })
}
export function selectedCommands(prompt, choices) {
  const names = new Set([...prompt.matchAll(/(?:^|\s)\/([a-z0-9_-]+)(?=\s|$)/g)].map(match => match[1]))
  return choices.filter(item => names.has(item.command)).map(item => item.id)
}
