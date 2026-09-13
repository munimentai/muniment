import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

const ALLOWED = {
  'test/macos-local-mode-config.test.js:18:29': "The fixture shell runs only through it.skipIf(process.platform === 'win32').",
  'test/linux-sign-in-state.test.js:19:20': "describe.skipIf(process.platform === 'win32')('Linux sign-in state cleanup')",
  'test/desktop-e2e-harness.test.js:354:20': "it.skipIf(process.platform === 'win32')('Keeps distinct captures after two failed spec runs.')",
  'test/desktop-e2e-harness.test.js:455:20': "describe.skipIf(process.platform === 'win32')('Linux spec process isolation')",
  'test/desktop-e2e-harness.test.js:479:22': "describe.skipIf(process.platform === 'win32')('Linux spec process isolation')",
  'test/desktop-e2e-harness.test.js:499:20': "describe.skipIf(process.platform === 'win32')('Linux spec process isolation')",
  'test/desktop-e2e-harness.test.js:660:20': "describe.skipIf(process.platform === 'win32')('macOS WDIO spec process isolation')",
  'test/desktop-e2e-harness.test.js:761:20': "describe.skipIf(process.platform === 'win32')('macOS WDIO startup diagnostics')",
  'test/desktop-e2e-harness.test.js:922:20': "describe.skipIf(process.platform === 'win32')('macOS WDIO startup diagnostics')",
  'test/desktop-e2e-harness.test.js:1163:28': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:1169:20': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:1226:20': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:1243:19': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:1290:20': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:1310:20': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:1395:19': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:2145:20': "describe.skipIf(process.platform === 'win32')('Linux early abort reporting')",
  'test/desktop-e2e-harness.test.js:2154:19': "describe.skipIf(process.platform === 'win32')('Linux early abort reporting')",
  'test/desktop-e2e-harness.test.js:2186:20': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2197:19': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2208:20': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2219:19': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2231:20': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2244:19': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2255:20': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2270:20': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2290:20': "it.skipIf(process.platform === 'win32')('includes and escapes the captured runner reason')",
  'test/desktop-e2e-harness.test.js:2302:20': "it.skipIf(process.platform === 'win32')('Reports a runner error without replacing an existing spec report.')",
  'test/desktop-e2e-harness.test.js:2318:20': "it.skipIf(process.platform === 'win32')('Preserves an existing report for statuses %s/%s and cause \"%s\".')",
  'test/desktop-e2e-harness.test.js:2328:20': "it.skipIf(process.platform === 'win32')('does not copy the desktop-ci transcript into JUnit')",
  'test/desktop-e2e-harness.test.js:2411:20': "describe.skipIf(process.platform === 'win32')('desktop-ci payload extraction')",
  'test/desktop-e2e-harness.test.js:2435:19': "describe.skipIf(process.platform === 'win32')('desktop-ci payload extraction')",
  'test/desktop-e2e-harness.test.js:2593:20': "describe.skipIf(process.platform === 'win32')('cleanup failure accounting')",
  'test/desktop-e2e-harness.test.js:2609:20': "describe.skipIf(process.platform === 'win32')('cleanup failure accounting')",
  'test/desktop-e2e-harness.test.js:2635:19': "describe.skipIf(process.platform === 'win32')('cleanup failure accounting')",
  'test/desktop-e2e-harness.test.js:2662:24': "describe.skipIf(process.platform === 'win32')('cleanup failure accounting')",
  'test/desktop-e2e-harness.test.js:4115:20': "describe.skipIf(process.platform === 'win32')('macOS WDIO runtime endpoint')",
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
