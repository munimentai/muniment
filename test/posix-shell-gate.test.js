import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

const ALLOWED = {
  '.github/lib/signing-env.test.js:33:22': "it.skipIf(process.platform === 'win32')('keeps the line out of the POSIX preamble's environment')",
  '.github/lib/signing-env.test.js:42:20': "it.skipIf(process.platform === 'win32')('matches the runner's od encoding')",
  'test/macos-wdio-tools.test.js:53:20': "it.skipIf(process.platform === 'win32')('launches the app with its login home and isolated state')",
  'test/macos-wdio-tools.test.js:23:22': "describe.skipIf(process.platform === 'win32')('macOS WDIO build tools')",
  'test/macos-keychain-session.test.js:120:22': "describe.skipIf(process.platform === 'win32')('macOS CI keychain session')",
  'test/desktop-e2e-harness.test.js:1349:22': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/ci-artifacts.test.js:38:27': "it.skipIf(process.platform === 'win32') on the artifact publisher test",
  'test/macos-local-mode-config.test.js:19:29': "The fixture shell runs only through it.skipIf(process.platform === 'win32').",
  'test/linux-sign-in-state.test.js:19:20': "describe.skipIf(process.platform === 'win32')('Linux sign-in state cleanup')",
  'test/e2e-core-pins.test.js:92:22': "describe.skipIf(process.platform === 'win32')('Installed E2E core pins')",
  'test/linux-build-download.test.js:35:35': "describe.skipIf(process.platform === 'win32')('Linux build downloads')",
  'test/desktop-e2e-harness.test.js:377:20': "it.skipIf(process.platform === 'win32')('Keeps distinct captures after two failed spec runs.')",
  'test/desktop-e2e-harness.test.js:478:20': "describe.skipIf(process.platform === 'win32')('Linux spec process isolation')",
  'test/desktop-e2e-harness.test.js:502:22': "describe.skipIf(process.platform === 'win32')('Linux spec process isolation')",
  'test/desktop-e2e-harness.test.js:522:20': "describe.skipIf(process.platform === 'win32')('Linux spec process isolation')",
  'test/desktop-e2e-harness.test.js:692:20': "describe.skipIf(process.platform === 'win32')('macOS WDIO spec process isolation')",
  'test/desktop-e2e-harness.test.js:793:20': "describe.skipIf(process.platform === 'win32')('macOS WDIO startup diagnostics')",
  'test/desktop-e2e-harness.test.js:954:20': "describe.skipIf(process.platform === 'win32')('macOS WDIO startup diagnostics')",
  'test/desktop-e2e-harness.test.js:1195:28': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:1201:20': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:1258:20': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:1275:19': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:1322:20': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:1383:20': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:1468:19': "describe.skipIf(process.platform === 'win32')('macOS installed launch harness')",
  'test/desktop-e2e-harness.test.js:2220:20': "describe.skipIf(process.platform === 'win32')('Linux early abort reporting')",
  'test/desktop-e2e-harness.test.js:4300:20': "describe.skipIf(process.platform === 'win32')('macOS WDIO spec homes')",
  'test/desktop-e2e-harness.test.js:2229:19': "describe.skipIf(process.platform === 'win32')('Linux early abort reporting')",
  'test/desktop-e2e-harness.test.js:2263:20': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2274:19': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2285:20': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2296:19': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2308:20': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2321:19': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2332:20': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2347:20': "describe.skipIf(process.platform === 'win32')('runner setup causes')",
  'test/desktop-e2e-harness.test.js:2367:20': "it.skipIf(process.platform === 'win32')('includes and escapes the captured runner reason')",
  'test/desktop-e2e-harness.test.js:2379:20': "it.skipIf(process.platform === 'win32')('Reports a runner error without replacing an existing spec report.')",
  'test/desktop-e2e-harness.test.js:2395:20': "it.skipIf(process.platform === 'win32')('Preserves an existing report for statuses %s/%s and cause \"%s\".')",
  'test/desktop-e2e-harness.test.js:2405:20': "it.skipIf(process.platform === 'win32')('does not copy the desktop-ci transcript into JUnit')",
  'test/desktop-e2e-harness.test.js:2488:20': "describe.skipIf(process.platform === 'win32')('desktop-ci payload extraction')",
  'test/desktop-e2e-harness.test.js:2512:19': "describe.skipIf(process.platform === 'win32')('desktop-ci payload extraction')",
  'test/desktop-e2e-harness.test.js:2670:20': "describe.skipIf(process.platform === 'win32')('cleanup failure accounting')",
  'test/desktop-e2e-harness.test.js:2686:20': "describe.skipIf(process.platform === 'win32')('cleanup failure accounting')",
  'test/desktop-e2e-harness.test.js:2712:19': "describe.skipIf(process.platform === 'win32')('cleanup failure accounting')",
  'test/desktop-e2e-harness.test.js:2739:24': "describe.skipIf(process.platform === 'win32')('cleanup failure accounting')",
  'test/nightly-workflow.test.js:13:18': "it.skipIf(process.platform === 'win32') on every runReportFallback caller",
  'test/nightly-workflow.test.js:50:20': "it.skipIf(process.platform === 'win32')('passes the remaining full build budget after Git and dependency setup')",
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
