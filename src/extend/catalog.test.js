import { describe, expect, it } from 'vitest'
import { catalog, categories, filterCatalog, invocations, invocationQuery, sortCatalog, commandChoices, selectedCommands } from './catalog.js'
describe('extension discovery', () => {
  it('covers the public Anthropic catalog without duplicate identities', () => {
    expect(catalog.length).toBeGreaterThanOrEqual(824)
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
