import fs from 'node:fs'
import path from 'node:path'

import { describe, expect, it } from 'vitest'

const RECORD_SELECTORS = new Map([
  ['src/App.svelte', ['.record', '.tool-card', '.receipt-record']],
  ['src/lib/Onboarding.svelte', ['.path']],
  ['src/lib/FirstRunConnect.svelte', ['.mono']],
  ['src/lib/MemoryImports.svelte', ['.root', '.mono']],
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

// A model name, a mode and a time are not records, so the lines that name
// them take the UI face.
const HUMAN_SELECTORS = new Map([
  ['src/App.svelte', ['.provenance', '.response-meta']],
  ['src/activity/RunLine.svelte', ['.run-line']],
])

describe('record fonts', () => {
  it.each([...HUMAN_SELECTORS])('%s renders the run and provenance lines in the UI face', (file, selectors) => {
    const css = styles(fs.readFileSync(path.join(process.cwd(), file), 'utf8'))
    for (const selector of selectors) {
      const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
      const rule = css.match(new RegExp(`(?:^|})\\s*${escaped}\\s*\\{([^}]*)}`, 'm'))
      expect(rule?.[1], selector).toMatch(/font\s*:[^;}]*var\(--font-human\)/)
    }
  })

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
