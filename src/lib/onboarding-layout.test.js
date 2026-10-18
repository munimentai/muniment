import fs from 'node:fs'
import path from 'node:path'
import { describe, expect, it } from 'vitest'

const source = fs.readFileSync(path.join(process.cwd(), 'src/lib/Onboarding.svelte'), 'utf8')
const connect = fs.readFileSync(path.join(process.cwd(), 'src/lib/FirstRunConnect.svelte'), 'utf8')
const styles = source.match(/<style>([\s\S]*)<\/style>/)?.[1] ?? ''
const rules = new Map([...styles
  .replace(/\/\*[\s\S]*?\*\//g, '')
  .matchAll(/([^{}]+)\{([^{}]*)\}/g)]
  .map(([, selector, declarations]) => [selector.trim().replace(/\s+/g, ' '), declarations]))
const connectRules = new Map([...(connect.match(/<style>([\s\S]*)<\/style>/)?.[1] ?? '')
  .replace(/\/\*[\s\S]*?\*\//g, '')
  .matchAll(/([^{}]+)\{([^{}]*)\}/g)]
  .map(([, selector, declarations]) => [selector.trim().replace(/\s+/g, ' '), declarations]))

describe('onboarding layout', () => {
  it('bounds the screen column even when a Home path cannot wrap', () => {
    const app = fs.readFileSync(path.join(process.cwd(), 'src/App.svelte'), 'utf8')
    expect(app).toMatch(/main\.onboarding-active\s*\{[^}]*grid-template-columns:\s*minmax\(0, 1fr\)/)
  })

  it('bounds the column while only the connect list scrolls', () => {
    expect(rules.get('.onboarding')).toMatch(/max-height:\s*calc\(100% - 24px\)/)
    expect(rules.get('.onboarding')).toMatch(/display:\s*flex/)
    expect(rules.get('.onboarding')).toMatch(/flex-direction:\s*column/)
    expect(connectRules.get('.connect')).toMatch(/overflow-y:\s*auto/)
    expect(connectRules.get('.connect')).toMatch(/min-height:\s*0/)
  })

  it('asks one question: the connect list, then More providers, then the Home line', () => {
    expect(source).toContain('<FirstRunConnect ')
    expect(source).not.toContain('<textarea')
    expect(connect.indexOf('class="options"')).toBeLessThan(connect.indexOf('class="more"'))
    expect(connect.indexOf('class="more"')).toBeLessThan(connect.indexOf('class="home"'))
  })

  it('uses the mono record register for addresses and the UI font for errors', () => {
    expect(rules.get('.path')).toMatch(/font:\s*var\(--text-12\) var\(--font-mono\)/)
    expect(rules.get('.error')).toMatch(/font:\s*var\(--text-12\) var\(--font-human\)/)
    expect(connectRules.get('.mono')).toMatch(/font-family:\s*var\(--font-mono\)/)
    expect(source).toContain('{#if modelError}<p class="error" role="alert">{modelError}</p>{/if}')
  })

  it('keeps a long name inside its one-line card without a nested scroller', () => {
    expect(connectRules.get('li')).not.toMatch(/overflow-y\s*:/)
    expect(connectRules.get('strong')).toMatch(/min-width:\s*0/)
    expect(connectRules.get('strong')).toMatch(/text-overflow:\s*ellipsis/)
  })
})
