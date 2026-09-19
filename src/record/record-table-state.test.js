import { describe, expect, it } from 'vitest'

import {
  cellText,
  cellValue,
  coerceInput,
  createOperation,
  deleteOperation,
  diffLines,
  linkOperation,
  mergeOperation,
  relationsFrom,
  editOperation,
  formFields,
  groupEdges,
  nextSort,
  tableColumns,
} from './record-table-state.js'

const deal = {
  name: 'deal',
  schema: {
    properties: {
      name: { type: 'string' },
      stage: { type: 'string', enum: ['discovery', 'won'] },
      amount: { type: 'number' },
      expected_close: { type: 'string', format: 'date' },
      utm_source: { type: 'string' },
    },
    required: ['name', 'stage', 'expected_close'],
    stateProperty: 'stage',
  },
  extension: { properties: { x_renewal_risk: { type: 'string', enum: ['low', 'high'] } } },
  states: ['discovery', 'won'],
}

describe('record table state', () => {
  it('generates the columns from the kind: base, core, then own, with the state once', () => {
    const columns = tableColumns(deal)
    expect(columns.map((column) => column.key)).toEqual(['title', 'state', 'updated_at', 'name', 'amount', 'expected_close', 'utm_source', 'x_renewal_risk'])
    expect(columns.find((column) => column.key === 'title').mono).toBe(false)
    expect(columns.find((column) => column.key === 'name').mono).toBe(false)
    expect(columns.find((column) => column.key === 'amount').mono).toBe(true)
    expect(columns.find((column) => column.key === 'expected_close').type).toBe('date')
    expect(columns.find((column) => column.key === 'x_renewal_risk')).toMatchObject({ own: true, label: 'renewal risk', mono: true })
    expect(columns.find((column) => column.key === 'name').required).toBe(true)
    expect(tableColumns({ name: 'person', schema: { properties: { full_name: { type: 'string' } } } }).map((column) => column.key)).toEqual(['title', 'updated_at', 'full_name'])
    expect(tableColumns(null)).toEqual([])
  })

  it('shows a property that shares a base column name once and still asks for it in the form', () => {
    const task = { name: 'task', schema: { properties: { title: { type: 'string' }, status: { type: 'string', enum: ['todo', 'done'] }, due_at: { type: 'string', format: 'date-time' } }, required: ['title', 'status'], stateProperty: 'status' }, states: ['todo', 'done'] }
    expect(tableColumns(task).map((column) => column.key)).toEqual(['title', 'state', 'updated_at', 'due_at'])
    expect(formFields(task).map((field) => field.key)).toEqual(['status', 'title', 'due_at'])
  })

  it('reads and formats a cell', () => {
    const row = { id: 'e1', title: 'Renewal', state: 'won', updated_at: '2026-09-15T10:30:00.000Z', data: { amount: 96000, tags: ['a', 'b'], live: true } }
    const columns = tableColumns(deal)
    expect(cellValue(row, columns[0])).toBe('Renewal')
    expect(cellValue(row, columns.find((column) => column.key === 'amount'))).toBe(96000)
    expect(cellText(row.updated_at, columns[2])).toBe('2026-09-15 10:30')
    expect(cellText(['a', 'b'], {})).toBe('a, b')
    expect(cellText(true, {})).toBe('yes')
    expect(cellText(null, {})).toBe('')
    expect(cellText({ a: 1 }, {})).toBe('{"a":1}')
  })

  it('flips the sort on the sorted column and sorts dates descending first', () => {
    expect(nextSort({ sort: 'updated_at', descending: true }, 'title')).toEqual({ sort: 'title', descending: false })
    expect(nextSort({ sort: 'title', descending: false }, 'title')).toEqual({ sort: 'title', descending: true })
    expect(nextSort(null, 'updated_at')).toEqual({ sort: 'updated_at', descending: true })
  })

  it('coerces typed input and builds the operations', () => {
    expect(coerceInput(' 96000 ', { type: 'number' })).toBe(96000)
    expect(coerceInput('ten', { type: 'number' })).toBe('ten')
    expect(coerceInput('3', { type: 'integer' })).toBe(3)
    expect(coerceInput('yes', { type: 'boolean' })).toBe(true)
    expect(coerceInput('a, b', { type: 'array' })).toEqual(['a', 'b'])
    expect(coerceInput('', { type: 'string' })).toBeNull()
    expect(editOperation({ id: 'e1' }, { key: 'amount' }, 5)).toEqual({ op: 'update', entity: 'entity:e1', data: { amount: 5 } })
    expect(createOperation(deal, { name: 'N', stage: 'won', amount: '', utm_source: null })).toEqual({ op: 'create', kind: 'deal', data: { name: 'N', stage: 'won' } })
  })

  it('orders the form fields required first with the state property once', () => {
    expect(formFields(deal).map((field) => field.key)).toEqual(['stage', 'name', 'expected_close', 'amount', 'utm_source', 'x_renewal_risk'])
  })

  it('spells a diff one line per change and groups edges by relation', () => {
    expect(diffLines({ op: 'update', before: { data: { industry: 'Shipping', city: 'Porto' } }, after: { data: { industry: 'Logistics', city: 'Porto' } } })).toEqual(['industry: Shipping to Logistics'])
    expect(diffLines({ op: 'update', before: { data: {} }, after: { data: { city: 'Porto' } } })).toEqual(['city: empty to Porto'])
    expect(diffLines({ op: 'delete', before: { kind: 'org', title: 'Northwind' } })).toEqual(['org Northwind is deleted'])
    expect(diffLines({ op: 'merge', loser: { title: 'Northwind Inc' }, survivor: { title: 'Northwind' }, identities_moved: 2 })).toEqual(['Northwind Inc into Northwind, 2 identities move'])
    expect(deleteOperation('org-1')).toEqual({ op: 'delete', entity: 'entity:org-1' })
    expect(linkOperation('person-1', 'works_at', 'org-1')).toEqual({ op: 'link', src: 'entity:person-1', relation: 'works_at', dst: 'entity:org-1' })
    expect(mergeOperation('org-2', 'org-1')).toEqual({ op: 'merge', loser: 'entity:org-2', survivor: 'entity:org-1' })
    const relations = [{ name: 'works_at', from: ['person'], to: ['org'] }, { name: 'about', from: ['any'], to: ['any'] }, { name: 'billed_to', from: ['invoice'], to: ['org'] }]
    const kinds = [{ name: 'person' }, { name: 'org' }]
    expect(relationsFrom(relations, 'person', kinds)).toEqual([{ name: 'works_at', targets: ['org'] }, { name: 'about', targets: ['person', 'org'] }])
    expect(relationsFrom(relations, 'invoice', kinds)).toEqual([{ name: 'about', targets: ['person', 'org'] }, { name: 'billed_to', targets: ['org'] }])
    expect(relationsFrom(null, 'org', kinds)).toEqual([])
    expect(diffLines({ op: 'create', after: { data: { name: 'N', amount: 5 } } })).toEqual(['name: N', 'amount: 5'])
    expect(diffLines(null)).toEqual([])
    const groups = groupEdges([
      { id: 'a', relation: 'works_at', src_id: 'p', dst_id: 'o', dst_title: 'Northwind', dst_kind: 'org', src_title: 'Elena', src_kind: 'person', valid_from: '2026', valid_to: '2027' },
      { id: 'b', relation: 'works_at', src_id: 'p', dst_id: 'o2', dst_title: 'Contoso', dst_kind: 'org', src_title: 'Elena', src_kind: 'person', valid_from: '2027', valid_to: null },
      { id: 'c', relation: 'owns', src_id: 'p', dst_id: 'd', dst_title: 'Renewal', dst_kind: 'deal', src_title: 'Elena', src_kind: 'person', valid_from: '2026', valid_to: null },
      { id: 'd', relation: 'reports_to', src_id: 'q', dst_id: 'p', dst_title: 'Elena', dst_kind: 'person', src_title: 'Dana', src_kind: 'person', valid_from: '2026', valid_to: null },
    ], 'p')
    expect(groups.map((group) => group.relation)).toEqual(['works_at', 'owns', 'reports_to (incoming)'])
    expect(groups[0].items.map((item) => item.otherTitle)).toEqual(['Contoso', 'Northwind'])
    expect(groups[2].items[0]).toMatchObject({ outgoing: false, otherTitle: 'Dana' })
  })
})

import { NO_STATE_COLUMN, askSql, boardColumns, moveOperation, viewData, viewSettings, viewsFor } from './record-table-state.js'

describe('board, saved views and Ask', () => {
  const kind = { name: 'deal', schema: { properties: { name: { type: 'string' }, stage: { type: 'string', enum: ['discovery', 'won'] }, amount: { type: 'number' } }, stateProperty: 'stage' }, states: ['discovery', 'won'] }
  const rows = [
    { id: 'a', title: 'A', state: 'won', data: {} },
    { id: 'b', title: 'B', state: 'discovery', data: {} },
    { id: 'c', title: 'C', state: null, data: {} },
  ]

  it('lays the rows into one column per state and one for none', () => {
    const columns = boardColumns(kind, rows)
    expect(columns.map((column) => column.label)).toEqual(['Discovery', 'Won', NO_STATE_COLUMN])
    expect(columns[0].rows.map((row) => row.id)).toEqual(['b'])
    expect(columns[1].rows.map((row) => row.id)).toEqual(['a'])
    expect(columns[2].rows.map((row) => row.id)).toEqual(['c'])
    expect(boardColumns(kind, rows.slice(0, 2)).map((column) => column.label)).toEqual(['Discovery', 'Won'])
    expect(boardColumns({ name: 'person', schema: {} }, [])).toEqual([])
  })

  it('moves a card as one update of the state property', () => {
    expect(moveOperation(rows[1], kind, 'won')).toEqual({ op: 'update', entity: 'entity:b', data: { stage: 'won' } })
    expect(moveOperation(rows[0], kind, 'won')).toBeNull()
    expect(moveOperation(rows[0], { name: 'person', schema: {} }, 'won')).toBeNull()
  })

  it('round-trips a saved view through the view kind', () => {
    const data = viewData('Won deals', kind, { layout: 'board', sort: { sort: 'amount', descending: true }, state: 'won', search: 'renewal', columns: ['title', 'amount'] })
    expect(data).toEqual({
      name: 'Won deals', kind: 'deal', layout: 'board',
      sort: [{ column: 'amount', descending: true }],
      filters: [{ property: 'state', equals: 'won' }, { property: 'search', matches: 'renewal' }],
      columns: ['title', 'amount'],
    })
    expect(viewSettings({ data })).toEqual({ layout: 'board', sort: { sort: 'amount', descending: true }, state: 'won', search: 'renewal' })
    expect(viewSettings({ data: { name: 'Plain', kind: 'deal' } })).toEqual({ layout: 'table', sort: { sort: 'updated_at', descending: true }, state: null, search: '' })
    expect(viewData('All', kind)).toEqual({ name: 'All', kind: 'deal', layout: 'table' })
    expect(viewsFor([{ kind: 'view', data: { kind: 'deal' } }, { kind: 'view', data: { kind: 'org' } }, { kind: 'deal', data: {} }], 'deal')).toHaveLength(1)
  })

  it('spells the open view as SQL over the generated view', () => {
    expect(askSql(kind, { sort: { sort: 'amount', descending: true }, state: 'won', search: "o'neil renewal" })).toBe(
      'select "title", "state", "updated_at", "name", "amount"\nfrom "v_deal"\nwhere state = \'won\' and id in (select entity_id from entity_search where entity_search match \'"o\'\'neil"* "renewal"*\')\norder by "amount" desc\nlimit 200',
    )
    expect(askSql(kind)).toBe('select "title", "state", "updated_at", "name", "amount"\nfrom "v_deal"\norder by "updated_at" desc\nlimit 200')
    expect(askSql(null)).toBe('')
  })
})
