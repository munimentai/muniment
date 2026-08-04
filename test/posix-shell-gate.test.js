import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

const ALLOWED = {
  'test/desktop-e2e-harness.test.js:288:28': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:293:19': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:594:20': "describe.skipIf(process.platform === 'win32')('desktop-ci payload extraction')",
  'test/desktop-e2e-harness.test.js:760:20': "describe.skipIf(process.platform === 'win32')('cleanup failure accounting')",
  'test/desktop-e2e-harness.test.js:776:20': "describe.skipIf(process.platform === 'win32')('cleanup failure accounting')",
  'test/desktop-e2e-harness.test.js:806:24': "describe.skipIf(process.platform === 'win32')('cleanup failure accounting')",
  'test/nightly-workflow.test.js:12:18': "it.skipIf(process.platform === 'win32') on every runReportFallback caller",
}

const root = process.cwd()
const excluded = (file) => file.split('/').some((segment) => [
  'node_modules',
  // Build output directories contain no test files.
  'target',
  'dist',
].includes(segment))
  || file.startsWith('.git/')
  || file.startsWith('test/e2e/')
const testFiles = (directory = '.') => fs.readdirSync(path.join(root, directory), { withFileTypes: true })
  .flatMap((entry) => {
    const file = path.posix.join(directory, entry.name).replace(/^\.\//, '')
    if (excluded(file)) return []
    if (entry.isDirectory()) return testFiles(file)
    return entry.name.endsWith('.test.js') ? [file] : []
  })

const bashCallSitesIn = (source, file) => [
  ...source.matchAll(/\bspawn(?:Sync)?\s*\(\s*(?:(?:\/\*[\s\S]*?\*\/|\/\/[^\n]*(?:\n|$))\s*)*(?:'bash'|"bash"|`bash`)/g),
].map((match) => {
  const before = source.slice(0, match.index)
  const line = before.split('\n').length
  const column = match.index - before.lastIndexOf('\n')
  return `${file}:${line}:${column}`
})

const bashCallSites = (file) => bashCallSitesIn(
  fs.readFileSync(path.join(root, file), 'utf8'),
  file,
)
const unexpectedSites = (used, allowed) => used.filter((site) => !(site in allowed))

describe('POSIX shell test gate', () => {
  const command = 'ba' + 'sh'

  it('excludes build output directories', () => {
    expect([
      'src-tauri/target/debug/deps',
      'dist/assets',
      'browser-control/dist',
    ].every(excluded)).toBe(true)
    expect(['test', 'src/lib'].some(excluded)).toBe(false)
  })

  it.each([
    ['single-quoted strings', `spawn('${command}', [])`],
    ['double-quoted strings', `spawnSync("${command}", [])`],
    ['template literals', 'spawn(`' + command + '`, [])'],
    ['block comments before the command', `spawn(/* command */ '${command}', [])`],
    ['line comments before the command', `spawn(// command\n'${command}', [])`],
  ])('finds bash commands in %s', (_form, source) => {
    expect(bashCallSitesIn(source, 'example.test.js')).toHaveLength(1)
  })

  it('gives same-line calls separate keys', () => {
    const used = bashCallSitesIn(`spawn('${command}'); spawnSync(\`${command}\`)`, 'example.test.js')
    expect(used).toHaveLength(2)
    expect(new Set(used).size).toBe(2)
    expect(unexpectedSites(used, { [used[0]]: 'Windows skip' })).toEqual([used[1]])
  })

  it('keeps every bash process call under a Windows skip', () => {
    const used = testFiles().flatMap(bashCallSites).sort()
    expect(unexpectedSites(used, ALLOWED)).toEqual([])
    expect(Object.keys(ALLOWED).filter((site) => !used.includes(site))).toEqual([])
  })
})
