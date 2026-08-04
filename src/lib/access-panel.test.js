// @vitest-environment jsdom

import fs from 'node:fs'
import path from 'node:path'
import { cleanup } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const source = fs.readFileSync(path.join(process.cwd(), 'src/lib/AccessPanel.svelte'), 'utf8')
const styles = source.match(/<style>([\s\S]*)<\/style>/)?.[1] ?? ''
const rules = new Map([...styles
  .replace(/\/\*[\s\S]*?\*\//g, '')
  .matchAll(/([^{}]+)\{([^{}]*)\}/g)]
  .map(([, selector, declarations]) => [selector.trim().replace(/\s+/g, ' '), declarations]))

beforeEach(() => {
  vi.stubGlobal('requestAnimationFrame', (callback) => callback())
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

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

  it('keeps profile metadata on one readable line', () => {
    expect(rules.get('.profile-button small')).toMatch(/overflow:\s*hidden/)
    expect(rules.get('.profile-button small')).toMatch(/text-overflow:\s*ellipsis/)
    expect(rules.get('.profile-button small')).toMatch(/white-space:\s*nowrap/)
    expect(source).toMatch(/class="profile-button" title=\{profileDetails\}/)
  })

  it('renders every connected program state and a missing approval time', () => {
    expect(source).toMatch(/Loading connected programs…/)
    expect(source).toMatch(/Connected programs could not be loaded\./)
    expect(source).toMatch(/No connected programs found/)
    expect(source).toMatch(/Approval time unavailable/)
    expect(source).toMatch(/onclick=\{loadCompanions\}>Try again/)
  })

  it('keeps claimed program text on one line with its full value available', () => {
    expect(source).toMatch(/<strong title=\{companion\.claimed_kind\}>/)
    expect(source).toMatch(/class="companion-version" title=\{companion\.claimed_version\}/)
    expect(rules.get('.companion-heading strong, .companion-version')).toMatch(/text-overflow:\s*ellipsis/)
    expect(rules.get('.companion-heading strong, .companion-version')).toMatch(/white-space:\s*nowrap/)
  })
})
