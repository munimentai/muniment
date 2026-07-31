import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

// Each entry states which unbreakable text the surface can render.
const WRAPPING_SELECTORS = {
  '.provenance': 'provenance can contain server-supplied routes, models, and capabilities',
  '.user-message': 'user messages can contain pasted identifiers and paths',
  '.tool-name': 'tool names can come from external tool definitions',
  '.permission-card p': 'permission details can contain commands, paths, and hosts',
  '.receipt-record dd': 'receipt values can contain routes, models, capabilities, and identifiers',
}

const root = process.cwd()
const read = (file) => fs.readFileSync(path.join(root, file), 'utf8')
const styleBlock = /<style>([\s\S]*)<\/style>/

const declarations = (source) => [
  ...(source.match(styleBlock)?.[1] ?? source)
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .matchAll(/([^{}]+)\{([^{}]*)\}/g),
].flatMap(([, selectors, body]) => selectors.split(',').map((selector) => ({
  selector: selector.trim().replace(/\s+/g, ' '),
  wrapsAnywhere: [...body.matchAll(/(?:^|;)\s*overflow-wrap\s*:\s*([^;]+)/gi)]
    .some(([, value]) => value.trim().toLowerCase() === 'anywhere'),
})))

const selectorsWithoutWrap = (source, selectors) => {
  const rules = declarations(source)
  return selectors.filter((selector) => !rules.some((rule) => rule.selector === selector && rule.wrapsAnywhere))
}

describe('text wrapping', () => {
  it('wraps unbreakable text in every text-bearing surface', () => {
    expect(selectorsWithoutWrap(read('src/App.svelte'), Object.keys(WRAPPING_SELECTORS))).toEqual([])
    expect(selectorsWithoutWrap(read('src/lib/AssistantMarkdown.svelte'), ['.assistant-markdown'])).toEqual([])
  })

  it('rejects a listed selector without overflow-wrap anywhere', () => {
    const drift = '<style>.response-prose { white-space: pre-wrap; }</style>'
    expect(selectorsWithoutWrap(drift, ['.response-prose'])).toEqual(['.response-prose'])
  })
})
