import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

const ALLOWED = {
  'test/desktop-e2e-harness.test.js:283': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:288': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:589': "describe.skipIf(process.platform === 'win32')('desktop-ci payload extraction')",
  'test/desktop-e2e-harness.test.js:755': "describe.skipIf(process.platform === 'win32')('cleanup failure accounting')",
  'test/desktop-e2e-harness.test.js:771': "describe.skipIf(process.platform === 'win32')('cleanup failure accounting')",
  'test/desktop-e2e-harness.test.js:801': "describe.skipIf(process.platform === 'win32')('cleanup failure accounting')",
  'test/nightly-workflow.test.js:12': "it.skipIf(process.platform === 'win32') on every runReportFallback caller",
}

const root = process.cwd()
const excluded = (file) => file.split('/').includes('node_modules')
  || file.startsWith('.git/')
  || file.startsWith('editor-extension/')
  || file.startsWith('test/e2e/')
const testFiles = (directory = '.') => fs.readdirSync(path.join(root, directory), { withFileTypes: true })
  .flatMap((entry) => {
    const file = path.posix.join(directory, entry.name).replace(/^\.\//, '')
    if (excluded(file)) return []
    if (entry.isDirectory()) return testFiles(file)
    return entry.name.endsWith('.test.js') ? [file] : []
  })

const bashCallSites = (file) => {
  const source = fs.readFileSync(path.join(root, file), 'utf8')
  return [...source.matchAll(/\bspawn(?:Sync)?\s*\(\s*(['"])bash\1/g)]
    .map((match) => `${file}:${source.slice(0, match.index).split('\n').length}`)
}

describe('POSIX shell test gate', () => {
  it('keeps every bash process call under a Windows skip', () => {
    const used = testFiles().flatMap(bashCallSites).sort()
    expect(used.filter((site) => !(site in ALLOWED))).toEqual([])
    expect(Object.keys(ALLOWED).filter((site) => !used.includes(site))).toEqual([])
  })
})
