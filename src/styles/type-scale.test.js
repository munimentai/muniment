import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

// docs/spec/design-spec.md §1.4 requires every component type size to resolve
// through the named --text-* register. There are no component exceptions; any
// future entry must cite the spec clause that requires it, as the shape guard does.
const TYPE_EXCEPTIONS = {}

const root = process.cwd()
const read = (file) => fs.readFileSync(path.join(root, file), 'utf8')
const styleBlock = /<style>([\s\S]*)<\/style>/

const declarations = (source) => [
  ...(source.match(styleBlock)?.[1] ?? source)
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .matchAll(/([^{}]+)\{([^{}]*)\}/g),
].flatMap(([, selectors, body]) => selectors.split(',').flatMap((selector) => [
  ...body.matchAll(/(?:^|;)\s*(font-size|font)\s*:\s*([^;]+)/gi),
].map(([, property, value]) => ({
  selector: selector.trim().replace(/\s+/g, ' '),
  property: property.toLowerCase(),
  value: value.trim().replace(/\s+/g, ' '),
}))))

const SIZE = /(?:\d*\.?\d+(?:px|rem|em|%|pt|pc|in|cm|mm|q|vw|vh|vmin|vmax|ch|ex|cap|ic|lh|rlh)\b|\b0\b)/i
const TEXT_TOKEN = /var\((--text-[a-z0-9-]+)\)/i
const TYPE_TOKENS = new Set([
  ...read('src/styles/tokens.css').matchAll(/(--text-[a-z0-9-]+)\s*:/gi),
].map(([, token]) => token.toLowerCase()))

const invalidTypeSizes = (source, exceptions = {}) => declarations(source)
  .filter(({ selector, property, value }) => {
    const declaresSize = property === 'font-size' || SIZE.test(value)
    const token = value.match(TEXT_TOKEN)?.[1].toLowerCase()
    return declaresSize && (!token || !TYPE_TOKENS.has(token)) && exceptions[selector]?.[0] !== value
  })

const components = (dir = 'src') => fs.readdirSync(path.join(root, dir), { withFileTypes: true })
  .flatMap((entry) => entry.isDirectory() ? components(`${dir}/${entry.name}`)
    : entry.name.endsWith('.svelte') ? [`${dir}/${entry.name}`] : [])
const COMPONENTS = components()

describe('§1.4 type scale', () => {
  it.each(COMPONENTS)('%s uses only named type-size tokens', (file) => {
    expect(invalidTypeSizes(read(file), TYPE_EXCEPTIONS[file] ?? {})).toEqual([])
  })

  it('allowlists only components and selectors that still exist', () => {
    expect(Object.keys(TYPE_EXCEPTIONS).filter((file) => !COMPONENTS.includes(file))).toEqual([])
    for (const [file, exceptions] of Object.entries(TYPE_EXCEPTIONS)) {
      const used = new Set(declarations(read(file)).map(({ selector }) => selector))
      expect(Object.keys(exceptions).filter((selector) => !used.has(selector))).toEqual([])
    }
  })

  it('rejects raw font-size values and raw sizes in font shorthands', () => {
    const drift = `<style>
      .pixels { font-size: 15px; }
      .relative { font-size: 1rem; }
      .shorthand { font: 600 12.5px/1.4 var(--font-mono); }
      .calculated { font: calc(1rem + 1px) var(--font-human); }
      .zero { font: 0 var(--font-mono); }
      .unknown { font-size: var(--text-unknown); }
    </style>`
    expect(invalidTypeSizes(drift).map(({ selector }) => selector))
      .toEqual(['.pixels', '.relative', '.shorthand', '.calculated', '.zero', '.unknown'])
  })

  it('accepts token sizes in both declaration forms and size-free shorthands', () => {
    const valid = `<style>
      .longhand { font-size: var(--text-15); }
      .shorthand { font: var(--text-12) var(--font-mono); }
      button { font: inherit; }
    </style>`
    expect(invalidTypeSizes(valid)).toEqual([])
  })
})
