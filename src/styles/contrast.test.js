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

const palettes = {
  'default light': block(/^:root\s*\{([^}]*)\}/m),
  'explicit light': block(/:root\[data-theme="light"\]\s*\{([^}]*)\}/),
  'default dark': block(/@media\s*\(prefers-color-scheme:\s*dark\)\s*\{\s*:root\s*\{([^}]*)\}/),
  'explicit dark': block(/:root\[data-theme="dark"\]\s*\{([^}]*)\}/),
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

  it('detects a color below the contrast floor', () => {
    expect(contrastRatio('#777777', '#FFFFFF')).toBeLessThan(4.5)
    expect(contrastRatio('#000000', '#FFFFFF')).toBe(21)
  })
})
