import { describe, expect, it } from 'vitest'

import {
  accumulateRun,
  identityOptions,
  externalPrefix,
  importErrorLine,
  mappedCount,
  mappingData,
  mappingLines,
  runSummaryLines,
  sourceOptions,
  sourceCredentials,
  credentialsFilled,
  packSecret,
  suggestKind,
  suggestFields,
  suggestIdentity,
  suggestProperty,
  targetProperties,
} from './record-import-state.js'

const org = {
  name: 'org',
  schema: { properties: { name: {}, legal_name: {}, domain: {}, industry: {}, employee_band: {}, city: {} } },
  extension: { properties: { x_headcount: { type: 'integer' } } },
}

const description = {
  source: 'csv',
  object: '/exports/Customers 2026.csv',
  label: 'Customers 2026.csv',
  rows: 3,
  fields: [
    { name: 'Company', guess: 'string', samples: ['Northwind'], filled: 3 },
    { name: 'Website', guess: 'domain', samples: ['northwind.example'], filled: 3 },
    { name: 'Contact email', guess: 'email', samples: ['a@b.co'], filled: 2 },
    { name: 'Headcount', guess: 'integer', samples: ['120'], filled: 3 },
    { name: 'Sector', guess: 'string', samples: ['Shipping'], filled: 3 },
    { name: 'Notes', guess: 'string', samples: ['x'], filled: 1 },
  ],
}

describe('record import state', () => {
  it('suggests a property by name, synonym or guessed type, and never twice', () => {
    expect(targetProperties(org)).toEqual(['name', 'legal_name', 'domain', 'industry', 'employee_band', 'city', 'x_headcount'])
    expect(suggestProperty({ name: 'Company' }, org)).toBe('name')
    expect(suggestProperty({ name: 'Website' }, org)).toBe('domain')
    expect(suggestProperty({ name: 'Sector' }, org)).toBe('industry')
    expect(suggestProperty({ name: 'Headcount' }, org)).toBe('x_headcount')
    expect(suggestProperty({ name: 'Domain Name', guess: 'domain' }, org)).toBe('domain')
    expect(suggestProperty({ name: 'Homepage', guess: 'domain' }, org, new Set(['domain']))).toBe('')
    expect(suggestProperty({ name: 'Notes' }, org)).toBe('')
    expect(suggestFields(description, org)).toEqual({
      Company: 'name',
      Website: 'domain',
      'Contact email': '',
      Headcount: 'x_headcount',
      Sector: 'industry',
      Notes: '',
    })
    expect(mappedCount(suggestFields(description, org))).toBe(4)
  })

  it('offers the title, the typed identity columns and an external id per field', () => {
    const options = identityOptions(description)
    expect(options[0]).toEqual({ value: '', label: 'the title' })
    expect(options[1]).toEqual({ value: 'domain:Website', label: 'domain in Website' })
    expect(options[2]).toEqual({ value: 'email:Contact email', label: 'email in Contact email' })
    expect(options[3]).toEqual({ value: 'external:csv:customers_2026:Company', label: 'id in Company' })
    expect(options).toHaveLength(9)
    expect(suggestIdentity(description)).toBe('email:Contact email')
    expect(suggestIdentity({ fields: [{ name: 'A', guess: 'string' }] })).toBe('')

    const stripe = { source: 'stripe', object: 'customers', label: 'Customers', counted: false, fields: [{ name: 'id', guess: 'id', samples: ['cus_1'], filled: 3 }, { name: 'email', guess: 'email', samples: ['a@b.co'], filled: 2 }, { name: 'name', guess: 'string', samples: ['Northwind'], filled: 3 }] }
    expect(externalPrefix(stripe)).toBe('stripe:customers')
    expect(externalPrefix(description)).toBe('csv:customers_2026')
    const stripeOptions = identityOptions(stripe)
    expect(stripeOptions[1]).toEqual({ value: 'external:stripe:customers:id', label: 'id in id' })
    expect(stripeOptions[2]).toEqual({ value: 'email:email', label: 'email in email' })
    expect(stripeOptions.map((option) => option.value)).not.toContain('external:stripe:customers:id:id')
    expect(suggestIdentity(stripe)).toBe('external:stripe:customers:id')
    expect(sourceOptions().map((option) => option.value)).toEqual(['csv', 'stripe', 'hubspot', 'pipedrive', 'salesforce', 'zendesk', 'intercom', 'freshdesk'])
    expect(sourceOptions().find((option) => option.value === 'hubspot').secret).toBe('private app access token')
    expect(sourceCredentials('stripe').map((credential) => credential.name)).toEqual(['token'])
    expect(sourceCredentials('zendesk').map((credential) => credential.name)).toEqual(['subdomain', 'email', 'api_token'])
    expect(sourceCredentials('zendesk').map((credential) => credential.secret)).toEqual([false, false, true])
    expect(credentialsFilled('stripe', { token: ' sk ' })).toBe(true)
    expect(credentialsFilled('zendesk', { subdomain: 'acme', email: 'a@b.co' })).toBe(false)
    expect(packSecret('stripe', { token: ' sk_test ' })).toBe('sk_test')
    expect(['contacts', 'persons', 'users', 'leads'].map((object) => suggestKind('hubspot', object))).toEqual(['person', 'person', 'person', 'person'])
    expect(['companies', 'organizations', 'accounts', 'customers'].map((object) => suggestKind('salesforce', object))).toEqual(['org', 'org', 'org', 'org'])
    expect([suggestKind('pipedrive', 'deals'), suggestKind('salesforce', 'opportunities'), suggestKind('zendesk', 'tickets'), suggestKind('stripe', 'invoices'), suggestKind('stripe', 'subscriptions')]).toEqual(['deal', 'deal', 'ticket', 'invoice', 'subscription'])
    expect(suggestKind('csv', 'contacts.csv')).toBe(null)
    expect(JSON.parse(packSecret('salesforce', { instance_url: ' https://acme.my.salesforce.com ', client_id: 'k', client_secret: 's' }))).toEqual({ instance_url: 'https://acme.my.salesforce.com', client_id: 'k', client_secret: 's' })
    expect(mappingLines(stripe, org, { name: 'name' }, 'external:stripe:customers:id')).toEqual(['name fills name', 'keyed on external in id', 'kind org', 'stripe Customers'])
  })

  it('builds the mapping record from the mapped fields alone', () => {
    const data = mappingData(description, org, { Company: 'name', Website: 'domain', Notes: '' }, 'domain:Website')
    expect(data).toEqual({
      source: 'csv',
      object: '/exports/Customers 2026.csv',
      kind: 'org',
      fields: { Company: 'name', Website: 'domain' },
      identity: 'domain:Website',
      approved: true,
    })
    expect(mappingData(description, org, { Company: 'name' }, '')).not.toHaveProperty('identity')
    expect(mappingLines(description, org, { Company: 'name', Website: 'domain', Notes: '' }, 'domain:Website')).toEqual(['Company fills name', 'Website fills domain', 'keyed on domain in Website', 'kind org', 'file Customers 2026.csv'])
    expect(mappingLines(description, org, { Company: 'name' }, 'external:csv:customers_2026:Company')).toEqual(['Company fills name', 'keyed on external in Company', 'kind org', 'file Customers 2026.csv'])
    expect(mappingLines(description, org, { Company: 'name' }, '')).toContain('keyed on the title')
  })

  it('reads a run as mono lines and tallies the calls of one run', () => {
    const run = { total: 3, label: 'customers.csv', created: 2, updated: 0, unchanged: 0, unplaced: 1, changed: true, queue: [{ row: 3, title: 'Gmail Co', reason: 'gmail.com names a mail provider' }] }
    expect(runSummaryLines(run)).toEqual(['3 rows in customers.csv', '2 created, 0 updated, 0 unchanged', '1 row not placed'])
    expect(runSummaryLines({ total: 1, object: '/a.csv', changed: false })).toEqual(['1 row in /a.csv', '0 created, 0 updated, 0 unchanged', 'The file is the one the last run read.'])
    expect(runSummaryLines(null)).toEqual([])
    const total = accumulateRun(accumulateRun(null, run), { ...run, created: 1, unplaced: 0, queue: [] })
    expect(total.created).toBe(3)
    expect(total.unplaced).toBe(1)
    expect(total.queue).toHaveLength(1)
    expect(importErrorLine({ error: { code: 'unknown_object', message: '/x.csv is not there to read. Pick it again.' } })).toBe('/x.csv is not there to read.')
    expect(importErrorLine('The background service is unavailable')).toBe('The background service is unavailable.')
    expect(importErrorLine(undefined)).toBe('The import did not answer.')
  })
})
