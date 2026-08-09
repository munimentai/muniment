import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { codeDiffPresentation } from './code-diff.js'

const fixtureDirectory = join(process.cwd(), 'protocol-fixtures/code-diff/1')
const fixtures = readdirSync(fixtureDirectory)
  .filter((name) => name.endsWith('.json'))
  .map((name) => [name, JSON.parse(readFileSync(join(fixtureDirectory, name), 'utf8'))])

describe('code-diff/1 fixtures', () => {
  it.each(fixtures)('presents %s', (_name, value) => {
    expect(() => codeDiffPresentation(value)).not.toThrow()
  })

  it('presents the empty message', () => {
    expect(codeDiffPresentation(fixture('empty.json')).emptyMessage).toBe('No changes.')
  })

  it('presents the truncated warning', () => {
    expect(codeDiffPresentation(fixture('truncated.json')).truncatedWarning)
      .toBe('Warning: This diff is truncated.')
  })

  it('presents the binary row', () => {
    expect(codeDiffPresentation(fixture('binary.json')).binaryFiles).toEqual([
      expect.objectContaining({ newName: 'assets/icon.png', message: 'Binary file changed' }),
    ])
  })
})

function fixture(name) {
  return fixtures.find(([fixtureName]) => fixtureName === name)[1]
}
