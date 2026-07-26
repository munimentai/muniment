import fs from 'node:fs'
import path from 'node:path'
import { describe, expect, it } from 'vitest'

const source = fs.readFileSync(path.join(process.cwd(), 'src/lib/AccessPanel.svelte'), 'utf8')
const styles = source.match(/<style>([\s\S]*)<\/style>/)?.[1] ?? ''
const rules = new Map([...styles
  .replace(/\/\*[\s\S]*?\*\//g, '')
  .matchAll(/([^{}]+)\{([^{}]*)\}/g)]
  .map(([, selector, declarations]) => [selector.trim().replace(/\s+/g, ' '), declarations]))

describe('access popover layout', () => {
  it('bounds the column while only its content region scrolls', () => {
    expect(rules.get('.access-popover')).toMatch(/max-height:\s*min\(\d+px,\s*70vh\)/)
    expect(rules.get('.access-popover')).toMatch(/display:\s*flex/)
    expect(rules.get('.access-popover')).toMatch(/flex-direction:\s*column/)
    expect(rules.get('.access-popover')).not.toMatch(/overflow-y\s*:/)
    expect(rules.get('.access-content')).toMatch(/overflow-y:\s*auto/)
  })

  it('separates the fixed header and footer from overflowing content', () => {
    expect(rules.get('.access-popover header')).toMatch(/flex:\s*none/)
    expect(rules.get('.access-popover header')).toMatch(/border-bottom:\s*1px solid var\(--border\)/)
    expect(rules.get('.access-footer')).toMatch(/flex:\s*none/)
    expect(rules.get('.access-footer')).toMatch(/border-top:\s*1px solid var\(--border\)/)
  })
})
