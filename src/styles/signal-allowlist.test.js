import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

// DESIGN.md law 1 is a LAW with an exhaustive allowed list, and it
// asks for exactly this check: "the `--signal` token may only be referenced by
// components on the allowed list". Every entry names the clause that permits it;
// adding one is a spec decision, not a styling one.
const ALLOWED = {
  'src/App.svelte': {
    '.streaming-rule': '§1.2 the streaming underline on the active line',
    '.caret': '§1.2 the caret on the active line',
    '.provenance .route-segment': '§1.2 the route segment of the provenance line',
    '.receipt-record .route-value': '§1.2 the route segment, expanded into the receipt (§2.2)',
  },
  // A theme tile shows the theme's own signal, a swatch of the token and not a use of it.
  'src/lib/Appearance.svelte': {
    '.swatch-signal': '§1.3 the signal mark on a theme swatch',
  },
  'src/lib/RunMark.svelte': {
    '.thinking .body': "§1.2 the mark's thinking state (§1.8)",
    '.thinking .accent': "§1.2 the trace that runs the mark's outline while thinking (§1.8)",
  },
  // Nothing in the access popover is computation: badges and device states
  // are all on §1.2's forbidden list.
  'src/lib/AccessPanel.svelte': {},
  // The show switch reads as on or off by signal, not by a shade of gray.
  'src/lib/ModelsSection.svelte': {
    '.switch[aria-checked="true"]': '§1.2 the track of an enabled model\'s show switch',
    '.switch[aria-checked="true"] span': '§1.2 the knob of an enabled model\'s show switch',
  },
}

// Repo root: `npm test` runs vitest with `--root .`, as test/desktop-e2e-harness.test.js assumes.
const root = process.cwd()
const read = (file) => fs.readFileSync(path.join(root, file), 'utf8')
const styleBlock = /<style>([\s\S]*)<\/style>/

// Owning selectors of every declaration block that reads --signal or
// --signal-soft. Comments go first, so prose about the rule is not a reference,
// and grouped selectors are split so each is judged on its own.
const signalSelectors = (source) => [
  ...(source.match(styleBlock)?.[1] ?? source)
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .matchAll(/([^{}]+)\{([^{}]*)\}/g),
].filter(([, , body]) => body.includes('--signal'))
  .flatMap(([, selector]) => selector.split(',').map((one) => one.trim().replace(/\s+/g, ' ')))
  .filter(Boolean)

const unpermitted = (source, allowed) => signalSelectors(source).filter((selector) => !(selector in allowed))

// Every :focus-visible rule that sets an outline, so a component cannot bring a
// focus ring back through its own style block.
const focusOutlineRules = (source) => [
  ...(source.match(styleBlock)?.[1] ?? source)
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .matchAll(/([^{}]+)\{([^{}]*)\}/g),
].flatMap(([, selectors, body]) => selectors.split(',').map((selector) => ({
  selector: selector.trim().replace(/\s+/g, ' '),
  declarations: [...body.matchAll(/(?:^|;)\s*(outline(?:-(?:color|style|width|offset))?)\s*:\s*([^;]+)/gi)]
    .map(([, property, value]) => [property.toLowerCase(), value.trim().replace(/\s+/g, ' ')]),
})))
  .filter(({ selector, declarations }) => selector.toLowerCase().includes(':focus-visible') && declarations.length)

const focusRingRules = (source) => focusOutlineRules(source)
  .filter(({ declarations }) => declarations.some(([property, value]) => !(property === 'outline' && /^(0|none)$/.test(value))))
  .map(({ selector }) => selector)

// Everything outside the style block, where an inline style: or style="" would
// route around the allowlist. Comments come out here too, for the same reason.
const markup = (source) => source.replace(styleBlock, '')
  .replace(/<!--[\s\S]*?-->|\/\*[\s\S]*?\*\//g, '')
  .replace(/^\s*\/\/.*$/gm, '')

// Every component under src/, so a new one is held to an empty allowlist by
// default rather than escaping the rule until someone remembers to list it.
const components = (dir = 'src') => fs.readdirSync(path.join(root, dir), { withFileTypes: true })
  .flatMap((entry) => entry.isDirectory() ? components(`${dir}/${entry.name}`)
    : entry.name.endsWith('.svelte') ? [`${dir}/${entry.name}`] : [])
const COMPONENTS = components()

describe('§1.2 signal allowlist', () => {
  it.each(COMPONENTS)('%s references --signal only from allowed components', (file) => {
    expect(unpermitted(read(file), ALLOWED[file] ?? {})).toEqual([])
  })

  it.each(COMPONENTS)('%s keeps --signal out of the markup too', (file) => {
    expect(markup(read(file))).not.toContain('--signal')
  })

  it('reads inline styles as references and comments as prose', () => {
    expect(markup('<!-- --signal is prose -->\n  // so is this --signal\n<p style="color: var(--signal)">'))
      .toContain('--signal')
    expect(markup('<!-- --signal is prose -->\n  // so is this --signal\n<p>')).not.toContain('--signal')
  })

  it('allowlists only components and selectors that still exist', () => {
    expect(Object.keys(ALLOWED).filter((file) => !COMPONENTS.includes(file))).toEqual([])
    for (const [file, allowed] of Object.entries(ALLOWED)) {
      const used = new Set(signalSelectors(read(file)))
      expect(Object.keys(allowed).filter((selector) => !used.has(selector))).toEqual([])
    }
  })

  it('rejects a reference from a component that is not on the list', () => {
    const drift = `<style>
      /* Prose naming --signal is not a reference. */
      .streaming-rule { border-bottom: 2px solid var(--signal); }
      .some-badge { color: var(--signal); }
      .some-chip:hover { background: var(--signal-soft); }
    </style>`
    expect(signalSelectors(drift)).toEqual(['.streaming-rule', '.some-badge', '.some-chip:hover'])
    expect(unpermitted(drift, ALLOWED['src/App.svelte'])).toEqual(['.some-badge', '.some-chip:hover'])
  })

  it('judges each selector of a grouped rule separately', () => {
    const drift = '<style>.streaming-rule,\n  .some-badge { border-color: var(--signal); }</style>'
    expect(unpermitted(drift, ALLOWED['src/App.svelte'])).toEqual(['.some-badge'])
  })
})

describe('no focus ring', () => {
  it('is one global :focus-visible rule that draws nothing', () => {
    // The leading anchor keeps this to the bare `:focus-visible` selector, not
    // a component-qualified one.
    const rules = [...read('src/styles/base.css')
      .replace(/\/\*[\s\S]*?\*\//g, '')
      .matchAll(/(?:^|[};])\s*:focus-visible\s*\{([^}]*)\}/g)]
    expect(rules).toHaveLength(1)
    expect(rules[0][1].trim()).toMatch(/^outline:\s*0\s*;$/)
  })

  it.each(COMPONENTS)('%s draws no focus ring of its own', (file) => {
    expect(focusRingRules(read(file))).toEqual([])
  })

  it('rejects a component focus ring', () => {
    const drift = '<style>.theme-options button:focus-visible { outline: 1px solid var(--ink); outline-offset: 2px; }</style>'
    expect(focusRingRules(drift)).toEqual(['.theme-options button:focus-visible'])

    const mixedCaseDrift = '<style>.theme-options button:FOCUS-VISIBLE { Outline: 2px solid var(--muted); }</style>'
    expect(focusRingRules(mixedCaseDrift)).toEqual(['.theme-options button:FOCUS-VISIBLE'])
  })

  it('accepts a component rule that only clears the outline', () => {
    const cleared = '<style>.field:focus-visible { outline: 0; border-color: var(--muted); }</style>'
    expect(focusRingRules(cleared)).toEqual([])
  })
})

describe('§1.7 reduced motion', () => {
  it('blanket-collapses every animation and transition', () => {
    const source = read('src/styles/base.css').replace(/\/\*[\s\S]*?\*\//g, '')
    const media = [...source.matchAll(/@media\s*\(prefers-reduced-motion:\s*reduce\)\s*\{([\s\S]*?)\n\}/g)]
    expect(media).toHaveLength(1)
    expect(media[0][1]).toMatch(/\*,\s*\*::before,\s*\*::after\s*\{[^}]*animation-duration:\s*0\.001ms !important\s*;[^}]*animation-iteration-count:\s*1 !important\s*;[^}]*transition-duration:\s*0\.001ms !important\s*;/)
  })
})
