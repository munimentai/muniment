import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

const ALLOWED = {
  'test/linux-sign-in-state.test.js:19:20': "describe.skipIf(process.platform === 'win32')('Linux sign-in state cleanup')",
  'test/desktop-e2e-harness.test.js:354:20': "it.skipIf(process.platform === 'win32')('Keeps distinct captures after two failed spec runs.')",
  'test/desktop-e2e-harness.test.js:455:20': "describe.skipIf(process.platform === 'win32')('Linux spec process isolation')",
  'test/desktop-e2e-harness.test.js:479:22': "describe.skipIf(process.platform === 'win32')('Linux spec process isolation')",
  'test/desktop-e2e-harness.test.js:499:20': "describe.skipIf(process.platform === 'win32')('Linux spec process isolation')",
  'test/desktop-e2e-harness.test.js:653:20': "describe.skipIf(process.platform === 'win32')('macOS WDIO spec process isolation')",
  'test/desktop-e2e-harness.test.js:742:20': "describe.skipIf(process.platform === 'win32')('macOS WDIO startup diagnostics')",
  'test/desktop-e2e-harness.test.js:903:20': "describe.skipIf(process.platform === 'win32')('macOS WDIO startup diagnostics')",
  'test/desktop-e2e-harness.test.js:1144:28': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:1150:20': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:1207:20': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:1224:19': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:1271:20': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:1291:20': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:1376:19': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:2126:20': "describe.skipIf(process.platform === 'win32')('Linux early abort reporting')",
  'test/desktop-e2e-harness.test.js:2135:19': "describe.skipIf(process.platform === 'win32')('Linux early abort reporting')",
  'test/desktop-e2e-harness.test.js:2167:20': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2178:19': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2189:20': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2200:19': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2212:20': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2225:19': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2236:20': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2251:20': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2271:20': "it.skipIf(process.platform === 'win32')('includes and escapes the captured runner reason')",
  'test/desktop-e2e-harness.test.js:2283:20': "it.skipIf(process.platform === 'win32')('Reports a runner error without replacing an existing spec report.')",
  'test/desktop-e2e-harness.test.js:2299:20': "it.skipIf(process.platform === 'win32')('Preserves an existing report for statuses %s/%s and cause \"%s\".')",
  'test/desktop-e2e-harness.test.js:2309:20': "it.skipIf(process.platform === 'win32')('does not copy the desktop-ci transcript into JUnit')",
  'test/desktop-e2e-harness.test.js:2392:20': "describe.skipIf(process.platform === 'win32')('desktop-ci payload extraction')",
  'test/desktop-e2e-harness.test.js:2416:19': "describe.skipIf(process.platform === 'win32')('desktop-ci payload extraction')",
  'test/desktop-e2e-harness.test.js:2574:20': "describe.skipIf(process.platform === 'win32')('cleanup failure accounting')",
  'test/desktop-e2e-harness.test.js:2590:20': "describe.skipIf(process.platform === 'win32')('cleanup failure accounting')",
  'test/desktop-e2e-harness.test.js:2616:19': "describe.skipIf(process.platform === 'win32')('cleanup failure accounting')",
  'test/desktop-e2e-harness.test.js:2643:24': "describe.skipIf(process.platform === 'win32')('cleanup failure accounting')",
  'test/desktop-e2e-harness.test.js:3922:20': "describe.skipIf(process.platform === 'win32')('macOS WDIO runtime endpoint')",
  'test/nightly-workflow.test.js:13:18': "it.skipIf(process.platform === 'win32') on every runReportFallback caller",
  'test/nightly-workflow.test.js:48:20': "it.skipIf(process.platform === 'win32')('passes the remaining full build budget after Git and dependency setup')",
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
