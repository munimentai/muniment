import { describe, expect, it } from 'vitest'
import { catalog, categories, filterCatalog, invocations, invocationQuery, sortCatalog, commandChoices, selectedCommands, providerDestination } from './catalog.js'
describe('extension discovery', () => {
  it('lists only remote servers without catalog branding or duplicate identities', () => {
    expect(catalog.length).toBeGreaterThanOrEqual(680)
    expect(catalog.every(item => item.type === 'remote' && !/anthropic/i.test(item.name + item.publisher))).toBe(true)
    expect(new Set(catalog.map(item => item.id)).size).toBe(catalog.length)
    expect(catalog.every(item => item.source.startsWith('https://claude.com/connectors/') && item.categories.length)).toBe(true)
  })
  it('combines category, connection, installed and text filters', () => {
    const item = catalog.find(item => item.url && item.type === 'remote')
    const results = filterCatalog(catalog, { query: item.name, category: item.categories[0], type: 'remote', installed: true }, new Set([item.source]))
    expect(results).toContainEqual(item)
    expect(filterCatalog(catalog, { query: item.name, installed: true })).toEqual([])
    expect(filterCatalog(catalog, { setup: true }).every(item => !item.url)).toBe(true)
    expect(categories.length).toBeGreaterThan(5)
  })
  it('only invokes installed enabled skills and plugins', () => {
    expect(invocations([{ id:'a', enabled:false, kind:'plugin' }, {id:'b',kind:'skill',skills:[{name:'review',path:'SKILL.md'}]}])).toEqual([{id:'b:SKILL.md',name:'review',description:undefined,kind:'skill'}])
    expect(invocationQuery('\\review')).toBe('review')
    expect(invocationQuery('/review')).toBe('review')
    for (const text of ['/Users/me', 'C:\\path', 'some /thing', '```/code']) expect(invocationQuery(text)).toBeNull()
  })
})

it('sorts by source popularity and resolves unique inline commands for this prompt', () => {
  const sorted = sortCatalog(catalog)
  expect(sorted[0].popularity).toBe(Math.max(...catalog.map(item => item.popularity)))
  const choices = commandChoices([{id:'one',kind:'plugin',name:'Review'}, {id:'two',kind:'plugin',name:'Review'}])
  expect(choices.map(item => item.command)).toEqual(['review','review-2'])
  expect(selectedCommands('/review-2 Check this', choices)).toEqual(['two'])
  expect(selectedCommands('Check this', choices)).toEqual([])
})

it('excludes directory relays and example endpoints without rejecting provider-owned host names', () => {
  expect(providerDestination('https://microsoft365.mcp.claude.com/mcp')).toBe(false)
  expect(providerDestination('https://HCLS.MCP.CLAUDE.COM./mcp')).toBe(false)
  expect(providerDestination('https://example-server.modelcontextprotocol.io/pdf/mcp')).toBe(false)
  expect(providerDestination('https://anthropic.mcp.creditkarma.com/mcp')).toBe(true)
  expect(providerDestination('https://mcp.notion.com/mcp')).toBe(true)
  expect(catalog.every(entry => !entry.url || providerDestination(entry.url))).toBe(true)
  expect(catalog.every(entry => !entry.website || providerDestination(entry.website))).toBe(true)
})

it('searches description and category words across fields regardless of punctuation or accents', () => {
  const entries = [{name:'Example',publisher:'Acme',description:'Résumé search and incident reports',categories:['developer-tools'],type:'remote'}]
  expect(filterCatalog(entries,{query:'resume developer tools'})).toEqual(entries)
  expect(filterCatalog(entries,{query:'ACME incident'})).toEqual(entries)
  expect(filterCatalog(entries,{query:'incident finance'})).toEqual([])
  expect(filterCatalog(entries,{query:'incident',category:'finance'})).toEqual([])
})
