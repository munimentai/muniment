import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

// docs/spec/design-spec.md §1.5/§4 permit only the radius scale and two depth
// tokens. These named exceptions are the circular dots, square resets, and the
// per-corner composition required by the segmented theme control.
const RADIUS_EXCEPTIONS = {
  'src/App.svelte': {
    '.active-thread > span': ['50%', '§1.5 circular current-thread status dot'],
    '.artifact-divider': ['0', '§4 square divider reset'],
    '.tool-dot': ['50%', '§1.5 circular tool status dot'],
  },
  'src/lib/AccessPanel.svelte': {
    '.theme-options button': ['0', '§4 square inner edges in the segmented control'],
    '.theme-options button:first-child': ['var(--radius-control) 0 0 var(--radius-control)', '§4 segmented-control corner composition'],
    '.theme-options button:last-child': ['0 var(--radius-control) var(--radius-control) 0', '§4 segmented-control corner composition'],
  },
}

const root = process.cwd()
const read = (file) => fs.readFileSync(path.join(root, file), 'utf8')
const styleBlock = /<style>([\s\S]*)<\/style>/

const declarations = (source) => [
  ...(source.match(styleBlock)?.[1] ?? source)
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .matchAll(/([^{}]+)\{([^{}]*)\}/g),
].flatMap(([, selectors, body]) => selectors.split(',').flatMap((selector) => [
  ...body.matchAll(/(?:^|;)\s*(border-radius|box-shadow)\s*:\s*([^;]+)/gi),
].map(([, property, value]) => ({
  selector: selector.trim().replace(/\s+/g, ' '),
  property: property.toLowerCase(),
  value: value.trim().replace(/\s+/g, ' '),
}))))

const VALID_RADII = new Set([
  'var(--radius-chip)',
  'var(--radius-control)',
  'var(--radius-panel)',
])
const VALID_SHADOWS = new Set([
  'var(--shadow-window)',
  'var(--shadow-overlay)',
])

const invalidShapes = (source, exceptions = {}) => declarations(source).filter(({ selector, property, value }) => {
  if (property === 'border-radius') {
    return !VALID_RADII.has(value) && exceptions[selector]?.[0] !== value
  }
  return !VALID_SHADOWS.has(value)
})

const components = (dir = 'src') => fs.readdirSync(path.join(root, dir), { withFileTypes: true })
  .flatMap((entry) => entry.isDirectory() ? components(`${dir}/${entry.name}`)
    : entry.name.endsWith('.svelte') ? [`${dir}/${entry.name}`] : [])
const COMPONENTS = components()

describe('§1.5/§4 shape scale', () => {
  it.each(COMPONENTS)('%s uses only radius and shadow tokens', (file) => {
    expect(invalidShapes(read(file), RADIUS_EXCEPTIONS[file] ?? {})).toEqual([])
  })

  it('allowlists only components and selectors that still exist', () => {
    expect(Object.keys(RADIUS_EXCEPTIONS).filter((file) => !COMPONENTS.includes(file))).toEqual([])
    for (const [file, exceptions] of Object.entries(RADIUS_EXCEPTIONS)) {
      const used = new Set(declarations(read(file)).map(({ selector }) => selector))
      expect(Object.keys(exceptions).filter((selector) => !used.has(selector))).toEqual([])
    }
  })

  it('rejects raw and off-scale radius values', () => {
    const drift = '<style>.raw { border-radius: 6px; } .off-scale { border-radius: 8px; }</style>'
    expect(invalidShapes(drift).map(({ selector }) => selector)).toEqual(['.raw', '.off-scale'])
  })

  it('rejects shadows outside the two-token scale', () => {
    const drift = '<style>.raw { box-shadow: 0 4px 24px rgba(0,0,0,.14); } .glow { box-shadow: 0 12px 36px color-mix(in srgb, var(--ink) 14%, transparent); }</style>'
    expect(invalidShapes(drift).map(({ selector }) => selector)).toEqual(['.raw', '.glow'])
  })
})
