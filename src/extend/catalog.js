import anthropic from './anthropic-catalog.json'
export const catalog = anthropic.map(entry => ({ ...entry, categories: entry.categories?.length ? entry.categories : ['other'] }))
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
