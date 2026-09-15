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

const themes = [
  'paper', 'vellum', 'ledger', 'foolscap', 'parchment', 'manila', 'linen', 'broadsheet',
  'moss', 'vault', 'graphite', 'inkwell', 'lagoon', 'umber', 'fjord', 'plum', 'nocturne', 'nightshade', 'basalt', 'obsidian', 'carbon',
]
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
    expect(palettes.paper).toEqual(palettes['default light'])
    expect(palettes.vault).toEqual(palettes['default dark'])
    expect(palettes.vault.paper).toBe('#000000')
    // The two high contrast sets hold pure ink on pure paper.
    expect([palettes.broadsheet.ink, palettes.broadsheet.paper]).toEqual(['#000000', '#FFFFFF'])
    expect([palettes.carbon.ink, palettes.carbon.paper]).toEqual(['#FFFFFF', '#000000'])
  })

  it('detects a color below the contrast floor', () => {
    expect(contrastRatio('#777777', '#FFFFFF')).toBeLessThan(4.5)
    expect(contrastRatio('#000000', '#FFFFFF')).toBe(21)
  })
})
