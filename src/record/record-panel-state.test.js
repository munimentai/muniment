import { describe, expect, it } from 'vitest'

import {
  currentCompany,
  kindLabel,
  kindProperties,
  kindSummary,
  orderKinds,
  recordErrorLine,
  validCompanyName, companyEmpty } from './record-panel-state.js'

describe('record panel state', () => {
  it('picks the current company and falls back to the oldest', () => {
    expect(currentCompany([])).toBeNull()
    expect(currentCompany(undefined)).toBeNull()
    const companies = [{ id: 'a', name: 'A', current: false }, { id: 'b', name: 'B', current: true }]
    expect(currentCompany(companies).id).toBe('b')
    expect(currentCompany([{ id: 'a', current: false }]).id).toBe('a')
  })

  it('keeps the catalogue order and appends the company own kinds by name', () => {
    const ordered = orderKinds([{ name: 'x_vendor' }, { name: 'person' }, { name: 'x_asset' }, { name: 'org' }])
    expect(ordered.map((kind) => kind.name)).toEqual(['person', 'org', 'x_asset', 'x_vendor'])
    expect(orderKinds(null)).toEqual([])
  })

  it('summarizes a kind in one mono line', () => {
    const deal = {
      name: 'deal',
      schema: { properties: { name: {}, stage: {}, amount: {} } },
      states: ['discovery', 'won'],
      extension: { properties: { x_renewal_risk: {} } },
    }
    expect(kindProperties(deal)).toEqual({ core: ['name', 'stage', 'amount'], extension: ['x_renewal_risk'] })
    expect(kindSummary(deal)).toBe('4 properties, 2 states, 1 own')
    expect(kindSummary({ name: 'x_note', schema: { properties: { x_text: {} } } })).toBe('1 property')
    expect(kindSummary({ ...deal, count: 1240 })).toBe('1,240 records')
    expect(kindSummary({ ...deal, count: 1 })).toBe('1 record')
    expect(kindSummary({ ...deal, count: 0 })).toBe('none yet')
    expect(companyEmpty([{ name: 'deal', count: 0 }, { name: 'org', count: 0 }])).toBe(true)
    expect(companyEmpty([{ name: 'deal', count: 0 }, { name: 'org', count: 3 }])).toBe(false)
    expect(companyEmpty([{ name: 'deal' }])).toBe(false)
    expect(companyEmpty([])).toBe(false)
    expect(kindLabel('x_renewal_risk')).toBe('renewal risk')
    expect(kindLabel('fact_source')).toBe('fact source')
  })

  it('reduces an error to its first sentence and validates a company name', () => {
    expect(recordErrorLine('The background service is unavailable. Restart it from the notice.')).toBe('The background service is unavailable.')
    expect(recordErrorLine(new Error('company name is empty'))).toBe('company name is empty.')
    expect(recordErrorLine(undefined)).toBe('The record did not answer.')
    expect(validCompanyName('Northwind')).toBe(true)
    expect(validCompanyName('   ')).toBe(false)
    expect(validCompanyName('n'.repeat(121))).toBe(false)
    expect(validCompanyName(`bad${String.fromCharCode(7)}`)).toBe(false)
  })
})
