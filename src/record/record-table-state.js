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

// Every property of the kind as a column, core first, then the company's own,
// without the state property, which the base `state` column shows.
export function propertyColumns(kind) {
  if (!kind) return []
  const required = new Set(kind.schema?.required ?? [])
  const stateProperty = kind.schema?.stateProperty
  const properties = Object.entries(kind.schema?.properties ?? {})
    .filter(([key]) => key !== stateProperty)
    .map(([key, schema]) => ({ ...propertyColumn(key, schema), required: required.has(key) }))
  const own = Object.entries(kind.extension?.properties ?? {})
    .map(([key, schema]) => propertyColumn(key, schema, true))
  return [...properties, ...own]
}

// A property that shares a base column's name, such as a task's `title`, is
// the base column: the table shows it once.
export function tableColumns(kind) {
  if (!kind) return []
  const stateProperty = kind.schema?.stateProperty
  const base = stateProperty ? BASE_COLUMNS : BASE_COLUMNS.filter((column) => column.key !== 'state')
  const taken = new Set(base.map((column) => column.key))
  return [...base, ...propertyColumns(kind).filter((column) => !taken.has(column.key))]
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
  const columns = propertyColumns(kind)
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

// The board: one column per state of the kind, and one for rows that hold none.
export const NO_STATE_COLUMN = 'no state'

export function boardColumns(kind, rows) {
  const states = Array.isArray(kind?.states) ? kind.states : []
  const columns = states.map((state) => ({ state, label: state, rows: [] }))
  const unset = { state: null, label: NO_STATE_COLUMN, rows: [] }
  for (const row of rows ?? []) {
    const column = columns.find((candidate) => candidate.state === row.state)
    if (column) column.rows.push(row)
    else unset.rows.push(row)
  }
  return unset.rows.length ? [...columns, unset] : columns
}

// Moving a card is one update of the kind's state property.
export function moveOperation(row, kind, toState) {
  const property = kind?.schema?.stateProperty
  if (!property || !row || row.state === toState) return null
  const data = {}
  data[property] = toState
  return { op: 'update', entity: `entity:${row.id}`, data }
}

// A saved view is a `view` record: what it shows and how.
export function viewData(name, kind, { layout = 'table', sort, state, search, columns } = {}) {
  const data = { name, kind: kind.name, layout }
  if (sort?.sort) data.sort = [{ column: sort.sort, descending: !!sort.descending }]
  const filters = []
  if (state) filters.push({ property: 'state', equals: state })
  if (search?.trim()) filters.push({ property: 'search', matches: search.trim() })
  if (filters.length) data.filters = filters
  if (Array.isArray(columns) && columns.length) data.columns = columns
  return data
}

export function viewSettings(row) {
  const data = row?.data ?? {}
  const sortEntry = Array.isArray(data.sort) ? data.sort[0] : null
  const filters = Array.isArray(data.filters) ? data.filters : []
  return {
    layout: data.layout === 'board' ? 'board' : 'table',
    sort: sortEntry?.column ? { sort: sortEntry.column, descending: !!sortEntry.descending } : { sort: 'updated_at', descending: true },
    state: filters.find((filter) => filter?.property === 'state')?.equals ?? null,
    search: filters.find((filter) => filter?.property === 'search')?.matches ?? '',
  }
}

export function viewsFor(rows, kindName) {
  return (rows ?? []).filter((row) => row?.kind === 'view' && row?.data?.kind === kindName)
}

// The SQL the open view stands for, over the kind's generated view, so a
// question in the thread starts from what the person sees.
export function askSql(kind, { sort, state, search, limit = 200 } = {}) {
  if (!kind) return ''
  const quote = (text) => `'${String(text).replaceAll("'", "''")}'`
  const identifier = (text) => `"${String(text).replaceAll('"', '""')}"`
  const columns = tableColumns(kind).map((column) => identifier(column.key))
  const clauses = []
  if (state) clauses.push(`state = ${quote(state)}`)
  if (search?.trim()) {
    const words = search.trim().split(/\s+/).map((word) => `"${word.replaceAll('"', '""')}"`).join(' ')
    clauses.push(`id in (select entity_id from entity_search where entity_search match ${quote(words)})`)
  }
  const order = sort?.sort ? `${identifier(sort.sort)} ${sort.descending ? 'desc' : 'asc'}` : '"updated_at" desc'
  return `select ${columns.join(', ')}\nfrom ${identifier(`v_${kind.name}`)}${clauses.length ? `\nwhere ${clauses.join(' and ')}` : ''}\norder by ${order}\nlimit ${limit}`
}
