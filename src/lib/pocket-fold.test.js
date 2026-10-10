import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { signInMarkSvg, signedInMarkSvg } from './pocket-fold.js'

describe('the sign-in marks', () => {
  it('ships the marks scripts/generate-graph-icons.py writes from the shared shapes', () => {
    expect(readFileSync('src-tauri/icons/muniment-mark-auth.svg', 'utf8').trim()).toBe(signInMarkSvg())
    expect(readFileSync('src-tauri/icons/muniment-mark-signed-in.svg', 'utf8').trim()).toBe(signedInMarkSvg())
  })

  it('plays the signed-in mark once with no script, and keeps it still under reduced motion', () => {
    const svg = signedInMarkSvg()
    expect(svg).not.toMatch(/<script|repeatCount|indefinite/)
    expect(svg.match(/fill="freeze"/g)).toHaveLength(4)
    expect(svg).toContain('@media(prefers-reduced-motion:reduce){.moving{display:none}.still{display:inline}}')
    expect(signInMarkSvg()).not.toContain('<animate')
  })
})
