import fs from 'node:fs'
import path from 'node:path'

import { describe, expect, it } from 'vitest'

const RECORD_SELECTORS = new Map([
  ['src/App.svelte', ['.record', '.tool-card', '.provenance', '.receipt-record']],
  ['src/lib/Onboarding.svelte', ['.path-card strong']],
])

const styles = (source) => [...source.matchAll(/<style(?:\s[^>]*)?>([\s\S]*?)<\/style>/g)]
  .map((match) => match[1])
  .join('\n')

export function recordSelectorsWithoutMono(source, selectors) {
  const css = styles(source) || source
  return selectors.filter((selector) => {
    const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
    const rule = css.match(new RegExp(`(?:^|})\\s*${escaped}\\s*\\{([^}]*)}`, 'm'))
    return !rule || !/(?:font|font-family)\s*:[^;}]*(?:var\(--font-mono\))/m.test(rule[1])
  })
}

describe('record fonts', () => {
  it.each([...RECORD_SELECTORS])('%s renders every record class with the mono token', (file, selectors) => {
    const source = fs.readFileSync(path.join(process.cwd(), file), 'utf8')
    expect(recordSelectorsWithoutMono(source, selectors)).toEqual([])
  })

  it('rejects a record class that uses the human font token', () => {
    const fixture = '.record { font-family: var(--font-human); }'
    expect(recordSelectorsWithoutMono(fixture, ['.record'])).toEqual(['.record'])
  })

  it('rejects a record class with no matching rule', () => {
    expect(recordSelectorsWithoutMono('', ['.record'])).toEqual(['.record'])
  })
})
