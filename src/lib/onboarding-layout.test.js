import fs from 'node:fs'
import path from 'node:path'
import { describe, expect, it } from 'vitest'

const source = fs.readFileSync(path.join(process.cwd(), 'src/lib/Onboarding.svelte'), 'utf8')
const styles = source.match(/<style>([\s\S]*)<\/style>/)?.[1] ?? ''
const rules = new Map([...styles
  .replace(/\/\*[\s\S]*?\*\//g, '')
  .matchAll(/([^{}]+)\{([^{}]*)\}/g)]
  .map(([, selector, declarations]) => [selector.trim().replace(/\s+/g, ' '), declarations]))

describe('onboarding layout', () => {
  it('bounds the screen column even when a Home path cannot wrap', () => {
    const app = fs.readFileSync(path.join(process.cwd(), 'src/App.svelte'), 'utf8')
    expect(app).toMatch(/main\.onboarding-active\s*\{[^}]*grid-template-columns:\s*minmax\(0, 1fr\)/)
  })

  it('bounds the column while only the chip panel scrolls', () => {
    expect(rules.get('.onboarding')).toMatch(/max-height:\s*calc\(100% - 48px\)/)
    expect(rules.get('.onboarding')).toMatch(/display:\s*flex/)
    expect(rules.get('.onboarding')).toMatch(/flex-direction:\s*column/)
    expect(rules.get('.panel')).toMatch(/overflow-y:\s*auto/)
    expect(rules.get('.panel')).toMatch(/min-height:\s*0/)
  })

  it('keeps the composer and chips above the scrollable panel', () => {
    expect(source.indexOf('<div class="composer">')).toBeLessThan(source.indexOf('<div class="chips"'))
    expect(source.indexOf('<div class="chips"')).toBeLessThan(source.indexOf('<section class="panel"'))
    expect(rules.get('.composer')).toMatch(/flex:\s*none/)
    expect(rules.get('.chips')).toMatch(/flex:\s*none/)
    expect(rules.get('.chips')).toMatch(/flex-wrap:\s*wrap/)
  })

  it('uses the mono record register for model settings errors', () => {
    expect(rules.get('.path, li, .error')).toMatch(/font:\s*var\(--text-12\) var\(--font-mono\)/)
    expect(source).toContain('{#if modelError}<p class="error" role="alert">{modelError}</p>{/if}')
  })

  it('does not nest list scrollers inside the panel', () => {
    expect(rules.get('ul')).not.toMatch(/max-height\s*:/)
    expect(rules.get('ul')).not.toMatch(/overflow-y\s*:/)
    expect(rules.get('li')).not.toMatch(/max-height\s*:/)
    expect(rules.get('li')).not.toMatch(/overflow\s*:/)
    expect(rules.get('li')).toMatch(/overflow-wrap:\s*anywhere/)
    expect(rules.get('.home-chip')).toMatch(/text-overflow:\s*ellipsis/)
  })
})
