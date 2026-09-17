// The record panel's header state: which company is current, how the kind
// list orders, and the one line a kind shows before its table view exists.

export function currentCompany(companies) {
  if (!Array.isArray(companies) || companies.length === 0) return null
  return companies.find((company) => company?.current) ?? companies[0]
}

// Core kinds first in catalogue order, then the company's own kinds by name.
export function orderKinds(kinds) {
  if (!Array.isArray(kinds)) return []
  const core = kinds.filter((kind) => !kind.name.startsWith('x_'))
  const own = kinds.filter((kind) => kind.name.startsWith('x_')).sort((a, b) => a.name.localeCompare(b.name))
  return [...core, ...own]
}

export function kindProperties(kind) {
  const core = Object.keys(kind?.schema?.properties ?? {})
  const extension = Object.keys(kind?.extension?.properties ?? {})
  return { core, extension }
}

// One mono line. A kind the runtime counted reads by its records, `1,240
// records` or `none yet`. A kind without a count reads by its schema,
// `14 properties, 6 states`, and a company's own properties count as own.
export function kindSummary(kind) {
  if (typeof kind?.count === 'number') {
    if (kind.count === 0) return 'none yet'
    return `${kind.count.toLocaleString('en-US')} ${kind.count === 1 ? 'record' : 'records'}`
  }
  const { core, extension } = kindProperties(kind)
  const count = core.length + extension.length
  const parts = [`${count} ${count === 1 ? 'property' : 'properties'}`]
  if (Array.isArray(kind?.states) && kind.states.length) parts.push(`${kind.states.length} states`)
  if (extension.length) parts.push(`${extension.length} own`)
  return parts.join(', ')
}

export function kindLabel(name) {
  return name.startsWith('x_') ? name.slice(2).replaceAll('_', ' ') : name.replaceAll('_', ' ')
}

// The first sentence of a failure, one line.
export function recordErrorLine(error) {
  const text = typeof error === 'string' ? error : error?.message ?? String(error ?? '')
  const first = text.split(/(?<=[.!?])\s/)[0]?.trim() || 'The record did not answer.'
  return first.endsWith('.') ? first : `${first}.`
}

export function validCompanyName(name) {
  const trimmed = (name ?? '').trim()
  return trimmed.length > 0 && trimmed.length <= 120 && ![...trimmed].some((c) => c.charCodeAt(0) < 32)
}

// Whether the company holds no records at all, so the panel opens on the
// connect screen. A kind list the runtime has not counted is not empty.
export function companyEmpty(kinds) {
  return Array.isArray(kinds) && kinds.length > 0 && kinds.every((kind) => typeof kind?.count === 'number') && kinds.every((kind) => kind.count === 0)
}
