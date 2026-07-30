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
  it('bounds the column while only its content region scrolls', () => {
    expect(rules.get('.onboarding')).toMatch(/max-height:\s*100%/)
    expect(rules.get('.onboarding')).toMatch(/display:\s*flex/)
    expect(rules.get('.onboarding')).toMatch(/flex-direction:\s*column/)
    expect(rules.get('.onboarding-content')).toMatch(/overflow-y:\s*auto/)
  })

  it('separates the fixed heading and footer from overflowing content', () => {
    expect(source).toMatch(/<header>[\s\S]*id="onboarding-title"[\s\S]*<\/header>\s*<div class="onboarding-content">/)
    expect(rules.get('.onboarding > header')).toMatch(/flex:\s*none/)
    expect(source).toMatch(/<\/div>\s*\{#if [\s\S]*<footer class="onboarding-footer">/)
    expect(rules.get('.onboarding-footer')).toMatch(/flex:\s*none/)
  })

  it('does not nest list scrollers inside the content region', () => {
    expect(rules.get('.manifest')).not.toMatch(/max-height\s*:/)
    expect(rules.get('.manifest')).not.toMatch(/overflow-y\s*:/)
    expect(rules.get('.manifest pre')).not.toMatch(/max-height\s*:/)
    expect(rules.get('.manifest pre')).not.toMatch(/overflow\s*:/)
    expect(rules.get('.approved-sources')).not.toMatch(/max-height\s*:/)
    expect(rules.get('.approved-sources')).not.toMatch(/overflow-y\s*:/)
  })
})
