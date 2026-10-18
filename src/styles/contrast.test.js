import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

const source = fs.readFileSync(path.join(process.cwd(), 'src/styles/tokens.css'), 'utf8')
const foregrounds = ['ink', 'muted', 'signal']
const backgrounds = ['paper', 'surface', 'faint']

const declarations = (body) => Object.fromEntries(
  [...body.matchAll(/--([\w-]+):\s*(#[\dA-F]{6})\s*;/gi)]
    .map(([, token, value]) => [token, value]),
)

const block = (pattern) => {
  const body = source.match(pattern)?.[1]
  expect(body, `Missing token block matching ${pattern}`).toBeDefined()
  return declarations(body)
}

const colors = ['moss', 'harbor', 'clay', 'iris']
const palettesOf = colors.flatMap((color) => [color, `${color}-vibrant`, `${color}-contrast`])
const themes = palettesOf.flatMap((palette) => [`${palette}-light`, `${palette}-dark`])
const themeBlock = (name) => new RegExp(`:root\\[data-theme="${name}"\\][^{]*\\{([^}]*)\\}`)

const palettes = {
  'default light': block(/^:root\s*\{([^}]*)\}/m),
  'default dark': block(/@media\s*\(prefers-color-scheme:\s*dark\)\s*\{\s*:root\s*\{([^}]*)\}/),
  ...Object.fromEntries(themes.map((name) => [name, block(themeBlock(name))])),
}

const linearChannel = (channel) => {
  const srgb = channel / 255
  return srgb <= 0.04045 ? srgb / 12.92 : ((srgb + 0.055) / 1.055) ** 2.4
}

const relativeLuminance = (hex) => {
  const channels = hex.match(/[\dA-F]{2}/gi).map((channel) => linearChannel(parseInt(channel, 16)))
  return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2]
}

const contrastRatio = (first, second) => {
  const [lighter, darker] = [relativeLuminance(first), relativeLuminance(second)]
    .sort((a, b) => b - a)
  return (lighter + 0.05) / (darker + 0.05)
}

describe('§1.3 text token contrast', () => {
  it.each(Object.entries(palettes))('%s clears 4.5:1 for every text/background pair', (_, palette) => {
    for (const foreground of foregrounds) {
      for (const background of backgrounds) {
        expect(
          contrastRatio(palette[foreground], palette[background]),
          `${foreground} on ${background}`,
        ).toBeGreaterThanOrEqual(4.5)
      }
    }
  })

  it('gives every theme all ten color tokens, a scheme, and a swatch selector', () => {
    const tokens = ['paper', 'surface', 'faint', 'ink', 'muted', 'border', 'signal', 'signal-soft', 'oxide', 'ochre']
    for (const name of themes) {
      const body = source.match(themeBlock(name))[1]
      for (const token of tokens) expect(body, `${name} --${token}`).toMatch(new RegExp(`--${token}:`))
      expect(body).toMatch(/color-scheme:\s*(light|dark)\s*;/)
      expect(source).toContain(`[data-swatch][data-theme="${name}"]`)
    }
    expect(themes).toHaveLength(24)
    expect(palettes['moss-vibrant-light']).toEqual(palettes['default light'])
    expect(palettes['moss-vibrant-dark']).toEqual(palettes['default dark'])
    // Every high contrast set holds pure ink on pure white paper in light and near-black paper in dark.
    for (const color of colors) {
      expect([palettes[`${color}-contrast-light`].ink, palettes[`${color}-contrast-light`].paper]).toEqual(['#000000', '#FFFFFF'])
      expect([palettes[`${color}-contrast-dark`].ink, palettes[`${color}-contrast-dark`].paper]).toEqual(['#FFFFFF', '#0A0A0A'])
    }
    // Every set carries an accent for its primary actions.
    for (const name of themes) for (const token of ['accent', 'accent-soft', 'on-accent']) expect(source.match(themeBlock(name))[1], `${name} --${token}`).toMatch(new RegExp(`--${token}:`))
  })

  it('detects a color below the contrast floor', () => {
    expect(contrastRatio('#777777', '#FFFFFF')).toBeLessThan(4.5)
    expect(contrastRatio('#000000', '#FFFFFF')).toBe(21)
  })
})

it('keeps every accent readable on action surfaces and filled buttons', () => {
  for (const name of [...themes, 'default light', 'default dark']) {
    const palette = palettes[name]
    expect(contrastRatio(palette.accent, palette['accent-soft'])).toBeGreaterThanOrEqual(4.5)
    expect(contrastRatio(palette.accent, palette['on-accent'])).toBeGreaterThanOrEqual(4.5)
    // Selection draws the accent on its soft fill over paper and surface.
    for (const background of ['paper', 'surface']) expect(contrastRatio(palette.accent, palette[background]), `${name} accent on ${background}`).toBeGreaterThanOrEqual(4.5)
    for (const foreground of ['ink', 'muted']) expect(contrastRatio(palette[foreground], palette['accent-soft']), `${name} ${foreground} on accent-soft`).toBeGreaterThanOrEqual(4.5)
  }
})

// Each accent stays in its color's hue family, so selection, focus and primary
// buttons sit in harmony with the surfaces the color tints.
const hue = (hex) => {
  const [r, g, b] = hex.match(/[\dA-F]{2}/gi).map((channel) => parseInt(channel, 16) / 255)
  const [max, min] = [Math.max(r, g, b), Math.min(r, g, b)]
  const delta = max - min
  const sector = max === r ? ((g - b) / delta) % 6 : max === g ? (b - r) / delta + 2 : (r - g) / delta + 4
  return (sector * 60 + 360) % 360
}
const hueDistance = (first, second) => Math.min(Math.abs(first - second), 360 - Math.abs(first - second))

it('keeps every accent in the hue family of its color', () => {
  for (const name of [...themes, 'default light', 'default dark']) {
    const palette = palettes[name]
    expect(hueDistance(hue(palette.accent), hue(palette.signal)), `${name} accent hue`).toBeLessThanOrEqual(30)
  }
})

// The state tokens hold a light and a dark value. Each clears 4.5:1 on the
// paper, surface and faint of every theme in its scheme, and on its own chip
// fill: 14% of the state color over surface or faint.
const states = Object.fromEntries([...source.matchAll(/--(success|active|danger|warning):\s*light-dark\((#[\dA-F]{6}),\s*(#[\dA-F]{6})\)/gi)]
  .map(([, token, light, dark]) => [token, { light, dark }]))
const mixed = (color, base, share) => `#${[0, 2, 4].map((at) => Math.round(parseInt(color.slice(1 + at, 3 + at), 16) * share + parseInt(base.slice(1 + at, 3 + at), 16) * (1 - share)).toString(16).padStart(2, '0')).join('')}`

it('keeps every state color readable in every theme and on its chip', () => {
  expect(Object.keys(states)).toEqual(['success', 'active', 'danger', 'warning'])
  for (const [name, palette] of Object.entries(palettes)) {
    const scheme = name.endsWith('dark') ? 'dark' : 'light'
    for (const [token, values] of Object.entries(states)) {
      const color = values[scheme]
      for (const background of [palette.paper, palette.surface, palette.faint, mixed(color, palette.surface, .14), mixed(color, palette.faint, .14)]) {
        expect(contrastRatio(color, background), `${name} --${token} on ${background}`).toBeGreaterThanOrEqual(4.5)
      }
    }
  }
})

// Starter tints are icon colors: each clears 3:1 on the paper, surface and
// faint of every theme in its scheme, and on its own soft fill.
const tints = Object.fromEntries([...source.matchAll(/--tint-([a-z]+):\s*light-dark\((#[\dA-F]{6}),\s*(#[\dA-F]{6})\)/gi)]
  .map(([, token, light, dark]) => [token, { light, dark }]))

it('keeps every starter tint visible in every theme', () => {
  expect(Object.keys(tints)).toEqual(['green', 'blue', 'amber', 'rose', 'teal', 'orange'])
  for (const [name, palette] of Object.entries(palettes)) {
    const scheme = name.endsWith('dark') ? 'dark' : 'light'
    for (const [token, values] of Object.entries(tints)) {
      const color = values[scheme]
      for (const background of [palette.paper, palette.surface, palette.faint, mixed(color, palette.surface, .14)]) {
        expect(contrastRatio(color, background), `${name} --tint-${token} on ${background}`).toBeGreaterThanOrEqual(3)
      }
    }
  }
})

// Secondary text keeps 4.5:1 wherever it lands, not only on the three base
// surfaces: on the selection fill, and on the sidebar the frames derive from
// the theme in src/styles/panels.css. Framed quiets the sidebar's ink toward
// paper over a paper-based faint and keeps the theme's muted. Frameless sets the sidebar on a panel
// a step from surface toward ink.
const frames = (palette) => ({
  framed: {
    ink: mixed(palette.ink, palette.paper, .78),
    muted: palette.muted,
    backgrounds: [palette.paper, mixed(palette.ink, palette.paper, .07)],
  },
  frameless: {
    ink: palette.ink,
    muted: palette.muted,
    backgrounds: [mixed(palette.ink, palette.surface, .05), mixed(palette.ink, palette.surface, .10)],
  },
})

it('keeps secondary text at 4.5:1 on the selection fill and on both sidebar frames', () => {
  for (const name of themes) {
    const palette = palettes[name]
    const selected = name.includes('-vibrant-') ? palette['accent-soft'] : palette.faint
    expect(contrastRatio(palette.muted, selected), `${name} muted on the selection fill`).toBeGreaterThanOrEqual(4.5)
    for (const [frame, { ink, muted, backgrounds }] of Object.entries(frames(palette))) {
      for (const background of backgrounds) {
        expect(contrastRatio(muted, background), `${name} ${frame} sidebar muted on ${background}`).toBeGreaterThanOrEqual(4.5)
        expect(contrastRatio(ink, background), `${name} ${frame} sidebar ink on ${background}`).toBeGreaterThanOrEqual(4.5)
      }
    }
  }
})

// The focus ring is the accent in a vibrant set and ink in the others. As a
// part of a control it clears 3:1 on every surface it can sit on.
it('keeps the focus ring at 3:1 on every surface', () => {
  for (const name of themes) {
    const palette = palettes[name]
    const ring = name.includes('-vibrant-') ? palette.accent : palette.ink
    for (const background of [palette.paper, palette.surface, palette.faint]) {
      expect(contrastRatio(ring, background), `${name} focus ring on ${background}`).toBeGreaterThanOrEqual(3)
    }
  }
})

// No dark theme paints a surface pure black. High contrast keeps a near-black
// paper, #0A0A0A, because its job is the widest step from white ink: white on
// it stays above 19:1 where pure black gives 21:1. Every other dark set lifts
// its paper to a dark gray.
it('paints no dark surface pure black', () => {
  for (const name of [...themes.filter((theme) => theme.endsWith('-dark')), 'default dark']) {
    const palette = palettes[name]
    for (const token of ['paper', 'surface', 'faint']) expect(palette[token].toUpperCase(), `${name} --${token}`).not.toBe('#000000')
    const floor = name.includes('-contrast-') ? relativeLuminance('#0A0A0A') : relativeLuminance('#0E1010')
    expect(relativeLuminance(palette.paper), `${name} --paper`).toBeGreaterThanOrEqual(floor)
    if (name.includes('-contrast-')) expect(contrastRatio(palette.ink, palette.paper)).toBeGreaterThan(19)
  }
})
