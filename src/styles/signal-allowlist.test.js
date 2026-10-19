import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

// DESIGN.md law 1 is a LAW with an exhaustive allowed list, and it
// asks for exactly this check: "the `--signal` token may only be referenced by
// components on the allowed list". Every entry names the clause that permits it;
// adding one is a spec decision, not a styling one.
const ALLOWED = {
  'src/lib/ui/Button.svelte': { 'button[data-variant="capability"][aria-pressed="true"]': 'Selected capability filters use the settings tab highlight.' },
  'src/lib/AppUpdate.svelte': { 'button': 'Verified update ready to install' },
  'src/lib/SettingsTabs.svelte': { 'button[aria-selected=true]': 'Selected settings tab' },
  'src/extend/ExtendSection.svelte': {
    '.catalog-card.connected': 'Installed server card',
    '.catalog-card.connected strong': 'Installed server name',
  },

  'src/App.svelte': {
    '.provenance .route-segment': '§1.2 the route segment of the provenance line',
    '.receipt-record .route-value': '§1.2 the route segment, expanded into the receipt (§2.2)',
  },
  // A theme tile shows the theme's own signal, a swatch of the token and not a use of it.
  'src/lib/Appearance.svelte': {
    '.swatch-signal': '§1.3 the signal mark on a theme swatch',
  },
  'src/lib/AssistantMarkdown.svelte': {
    '.assistant-markdown :global(.caret)': '§1.2 the caret on the active line',
  },
  'src/lib/PocketFold.svelte': {
    '.ear': "§1.2 the mark's folded ear on setup and sign-in",
  },
  'src/lib/RunMark.svelte': {
    '.thinking .accent': "§1.2 the mark's folded corner while thinking (§1.8)",
  },
  // Nothing in the access popover is computation: badges and device states
  // are all on §1.2's forbidden list.
  'src/lib/AccessPanel.svelte': {},
  // The show switch reads as on or off by signal, not by a shade of gray.

  // The same switch turns an account in a pool on, and turns routing on.
  'src/runtimes/MachinePicker.svelte': { '.state.online': 'A green globe marks a machine that answers.' },
  'src/lib/AgentAvatar.svelte': { '.agent-avatar.agent-avatar[data-activity="running"]': 'A running agent outlines its avatar, a workflow-run indicator.' },
  'src/lib/MemorySection.svelte': {
    '.quick .confirm': 'The Confirm check on an unconfirmed memory row',
    ':global(:root[data-theme^="moss-vibrant-"]) .quick .confirm': 'The Confirm check on an unconfirmed memory row in Moss vibrant',
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

// Every :focus-visible rule that sets an outline, so a component cannot drop
// or redraw the one focus ring through its own style block.
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

const focusRingRules = (source) => focusOutlineRules(source).map(({ selector }) => selector)

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
      .provenance .route-segment { border-bottom: 2px solid var(--signal); }
      .some-badge { color: var(--signal); }
      .some-chip:hover { background: var(--signal-soft); }
    </style>`
    expect(signalSelectors(drift)).toEqual(['.provenance .route-segment', '.some-badge', '.some-chip:hover'])
    expect(unpermitted(drift, ALLOWED['src/App.svelte'])).toEqual(['.some-badge', '.some-chip:hover'])
  })

  it('judges each selector of a grouped rule separately', () => {
    const drift = '<style>.provenance .route-segment,\n  .some-badge { border-color: var(--signal); }</style>'
    expect(unpermitted(drift, ALLOWED['src/App.svelte'])).toEqual(['.some-badge'])
  })
})

describe('one focus ring', () => {
  it('is one global :focus-visible rule that draws a 1px ring in --focus-ring', () => {
    // The leading anchor keeps this to the bare `:focus-visible` selector, not
    // a component-qualified one.
    const source = read('src/styles/base.css').replace(/\/\*[\s\S]*?\*\//g, '')
    const rules = [...source.matchAll(/(?:^|[};])\s*:focus-visible\s*\{([^}]*)\}/g)]
    expect(rules).toHaveLength(1)
    expect(rules[0][1]).toMatch(/outline:\s*1px solid var\(--focus-ring\) !important\s*;/)
    // A text field whose frame shows focus leaves the ring to it: the composer,
    // the Settings search, the command palette and the launcher.
    expect(source).toMatch(/\[data-focus-frame\]:focus-visible\s*\{\s*outline:\s*0 !important;\s*\}/)
    const framed = COMPONENTS.filter((file) => /data-focus-frame/.test(markup(read(file))))
    expect(framed).toEqual(['src/App.svelte', 'src/Launcher.svelte', 'src/sidebar/CommandPalette.svelte'])
    // Menu items and options, and every control while the pointer leads, draw no ring.
    expect(source).toMatch(/:root\[data-pointer\] :focus-visible:not\(input, textarea, select, \[contenteditable\]\)\s*\{\s*outline:\s*0 !important;\s*\}/)
    expect(read('src/composer/ChatComposer.svelte')).toMatch(/\.composer:focus-within\s*\{[^}]*outline:\s*1px solid var\(--focus-edge\)/)
  })

  it.each(COMPONENTS)('%s leaves the focus ring to the global rule', (file) => {
    expect(focusRingRules(read(file))).toEqual([])
  })

  it('rejects a component rule that redraws or clears the ring', () => {
    const drift = '<style>.theme-options button:focus-visible { outline: 1px solid var(--ink); outline-offset: 2px; }</style>'
    expect(focusRingRules(drift)).toEqual(['.theme-options button:focus-visible'])
    const mixedCaseDrift = '<style>.theme-options button:FOCUS-VISIBLE { Outline: 2px solid var(--muted); }</style>'
    expect(focusRingRules(mixedCaseDrift)).toEqual(['.theme-options button:FOCUS-VISIBLE'])
    const cleared = '<style>.field:focus-visible { outline: 0; border-color: var(--muted); }</style>'
    expect(focusRingRules(cleared)).toEqual(['.field:focus-visible'])
  })
})

describe('§1.7 reduced motion', () => {
  it('blanket-collapses every animation and transition under data-motion', () => {
    const source = read('src/styles/base.css').replace(/\/\*[\s\S]*?\*\//g, '')
    expect(source).toMatch(/:root\[data-motion="reduce"\] \*,\s*:root\[data-motion="reduce"\] \*::before,\s*:root\[data-motion="reduce"\] \*::after\s*\{[^}]*animation-duration:\s*0\.001ms !important\s*;[^}]*animation-iteration-count:\s*1 !important\s*;[^}]*transition-duration:\s*0\.001ms !important\s*;/)
  })

  // The root's data-motion is the one switch: the OS setting under System, or
  // the Appearance pick. A component media query would ignore the pick.
  it.each(COMPONENTS)('%s reads reduced motion from data-motion', (file) => {
    expect(read(file)).not.toMatch(/prefers-reduced-motion/)
  })
})
