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
    expect(palettes['moss-dark'].paper).toBe('#000000')
    // Every high contrast set holds pure ink on pure paper.
    for (const color of colors) {
      expect([palettes[`${color}-contrast-light`].ink, palettes[`${color}-contrast-light`].paper]).toEqual(['#000000', '#FFFFFF'])
      expect([palettes[`${color}-contrast-dark`].ink, palettes[`${color}-contrast-dark`].paper]).toEqual(['#FFFFFF', '#000000'])
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
