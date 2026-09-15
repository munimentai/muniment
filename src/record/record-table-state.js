// The table view's pure state: which columns a kind shows, how a cell reads,
// and the propose operations an edit or a new record becomes. Every column
// generates from the kind row; no screen names a kind by hand.

const BASE_COLUMNS = [
  { key: 'title', label: 'title', type: 'title', mono: false, base: true },
  { key: 'state', label: 'state', type: 'state', mono: true, base: true },
  { key: 'updated_at', label: 'updated', type: 'date-time', mono: true, base: true },
]

// Text without a format reads in the body face; every typed value is a record and reads in mono.
export function propertyColumn(key, schema, own = false) {
  const type = schema?.type ?? 'string'
  const format = schema?.format
  const hasEnum = Array.isArray(schema?.enum)
  const mono = type !== 'string' || !!format || hasEnum
  return { key, label: own ? key.slice(2).replaceAll('_', ' ') : key.replaceAll('_', ' '), type: format ?? type, mono, own, enum: hasEnum ? schema.enum : null, required: false }
}

export function tableColumns(kind) {
  if (!kind) return []
  const required = new Set(kind.schema?.required ?? [])
  const stateProperty = kind.schema?.stateProperty
  const properties = Object.entries(kind.schema?.properties ?? {})
    .filter(([key]) => key !== stateProperty)
    .map(([key, schema]) => ({ ...propertyColumn(key, schema), required: required.has(key) }))
  const own = Object.entries(kind.extension?.properties ?? {})
    .map(([key, schema]) => propertyColumn(key, schema, true))
  const base = stateProperty ? BASE_COLUMNS : BASE_COLUMNS.filter((column) => column.key !== 'state')
  return [...base, ...properties, ...own]
}

export function cellValue(row, column) {
  if (column.base) return row?.[column.key]
  return row?.data?.[column.key]
}

export function cellText(value, column) {
  if (value === null || value === undefined) return ''
  if (column?.type === 'date-time' && typeof value === 'string') {
    const date = new Date(value)
    if (!Number.isNaN(date.getTime())) return date.toISOString().slice(0, 16).replace('T', ' ')
  }
  if (Array.isArray(value)) return value.map((item) => (typeof item === 'string' ? item : JSON.stringify(item))).join(', ')
  if (typeof value === 'object') return JSON.stringify(value)
  if (typeof value === 'boolean') return value ? 'yes' : 'no'
  return String(value)
}

// A click on the sorted column flips it; a click elsewhere sorts that column ascending.
export function nextSort(current, key) {
  if (current?.sort === key) return { sort: key, descending: !current.descending }
  return { sort: key, descending: key === 'updated_at' || key === 'created_at' }
}

// Reads one typed cell edit back into the property's JSON value.
export function coerceInput(text, column) {
  const trimmed = (text ?? '').trim()
  if (trimmed === '') return null
  switch (column?.type) {
    case 'number': {
      const number = Number(trimmed)
      return Number.isFinite(number) ? number : trimmed
    }
    case 'integer': {
      const number = Number(trimmed)
      return Number.isInteger(number) ? number : trimmed
    }
    case 'boolean':
      return ['yes', 'true', '1', 'y'].includes(trimmed.toLowerCase()) ? true : ['no', 'false', '0', 'n'].includes(trimmed.toLowerCase()) ? false : trimmed
    case 'array':
      return trimmed.split(',').map((item) => item.trim()).filter(Boolean)
    default:
      return trimmed
  }
}

export function editOperation(row, column, value) {
  const data = {}
  data[column.key] = value
  return { op: 'update', entity: `entity:${row.id}`, data }
}

export function createOperation(kind, data) {
  const cleaned = Object.fromEntries(Object.entries(data ?? {}).filter(([, value]) => value !== null && value !== '' && value !== undefined))
  return { op: 'create', kind: kind.name, data: cleaned }
}

// The form shows required properties first, then the rest, and never the state
// property twice.
export function formFields(kind) {
  const columns = tableColumns(kind).filter((column) => !column.base)
  const stateProperty = kind?.schema?.stateProperty
  if (stateProperty && kind.schema?.properties?.[stateProperty]) {
    const required = new Set(kind.schema?.required ?? [])
    columns.unshift({ ...propertyColumn(stateProperty, kind.schema.properties[stateProperty]), required: required.has(stateProperty) })
  }
  return [...columns.filter((column) => column.required), ...columns.filter((column) => !column.required)]
}

// One line per changed property: `industry: Shipping → Logistics`. The arrow
// is spelled, because the shell carries no em dash and no arrow glyph.
export function diffLines(diff) {
  if (!diff) return []
  if (diff.op === 'create') {
    return Object.entries(diff.after?.data ?? {}).map(([key, value]) => `${key}: ${cellText(value, {})}`)
  }
  if (diff.op === 'update') {
    const before = diff.before?.data ?? {}
    const after = diff.after?.data ?? {}
    const keys = new Set([...Object.keys(before), ...Object.keys(after)])
    return [...keys]
      .filter((key) => JSON.stringify(before[key]) !== JSON.stringify(after[key]))
      .map((key) => `${key}: ${cellText(before[key], {}) || 'empty'} to ${cellText(after[key], {}) || 'empty'}`)
  }
  if (diff.op === 'link') return [`${diff.link?.relation}: ${diff.src_title} to ${diff.dst_title}`]
  if (diff.op === 'merge') return [`${diff.loser?.title} into ${diff.survivor?.title}`]
  return []
}

// Edges grouped by relation for the record view, the open ones first.
export function groupEdges(edges, entityId) {
  const groups = new Map()
  for (const edge of edges ?? []) {
    const outgoing = edge.src_id === entityId
    const key = `${edge.relation}${outgoing ? '' : ' (incoming)'}`
    const entry = groups.get(key) ?? []
    entry.push({
      id: edge.id,
      relation: edge.relation,
      outgoing,
      otherId: outgoing ? edge.dst_id : edge.src_id,
      otherTitle: outgoing ? edge.dst_title : edge.src_title,
      otherKind: outgoing ? edge.dst_kind : edge.src_kind,
      validFrom: edge.valid_from,
      validTo: edge.valid_to,
    })
    groups.set(key, entry)
  }
  return [...groups.entries()].map(([relation, items]) => ({ relation, items: items.sort((a, b) => (a.validTo ? 1 : 0) - (b.validTo ? 1 : 0)) }))
}
