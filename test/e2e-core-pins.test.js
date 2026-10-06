// @vitest-environment node
import { afterEach, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

const repository = process.cwd()
const pinsModule = pathToFileURL(path.join(repository, 'scripts/check-agent-dependencies.mjs')).href
const temporary = []
afterEach(() => {
  for (const root of temporary.splice(0)) fs.rmSync(root, { recursive: true, force: true })
})

function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'e2e-core-pins-'))
  temporary.push(root)
  const core = path.join(root, 'resolved core')
  const home = path.join(root, 'spec-home')
  const tools = path.join(root, 'tools')
  fs.mkdirSync(path.join(core, 'pins'), { recursive: true })
  fs.mkdirSync(home)
  fs.mkdirSync(tools)
  fs.writeFileSync(path.join(core, 'pins/pins.toml'), `[pi]
version = "1.2.3"
[claude_code]
version = "4.5.6"
[[packages]]
name = "@example/extension"
version = "7.8.9"
`)
  fs.symlinkSync(process.execPath, path.join(tools, 'node'))
  fs.writeFileSync(path.join(tools, 'cargo'), `#!${process.execPath}
const fs = require('node:fs')
fs.appendFileSync(process.env.CARGO_CALLS, JSON.stringify(process.argv.slice(2)) + '\\n')
if (process.env.HOME === process.env.SPEC_HOME || process.env.CARGO_FIXTURE === 'failure') {
  console.error('error: rustup has no default toolchain')
  process.exit(1)
}
console.log(JSON.stringify({ packages: process.env.CARGO_FIXTURE === 'empty' ? [] : [
  { name: 'muniment-pins', manifest_path: process.env.CORE_FIXTURE + '/crates/pins/Cargo.toml' }
] }))
`, { mode: 0o755 })
  const env = { ...process.env, PATH: `${tools}${path.delimiter}${process.env.PATH}`,
    HOME: root, SPEC_HOME: home, CORE_FIXTURE: core, CARGO_CALLS: path.join(root, 'cargo-calls'),
    MUNIMENT_CORE_ROOT: '', CARGO_FIXTURE: '' }
  return { core, home, env }
}

const readPins = `import { readPins } from ${JSON.stringify(pinsModule)}; console.log(JSON.stringify(readPins()))`
const expected = { pi: '1.2.3', claude: '4.5.6', packages: { '@example/extension': '7.8.9' } }
const run = (args, env) => spawnSync(process.execPath, args, { cwd: repository, env, encoding: 'utf8', timeout: 10000 })

// POSIX fixtures exercise the shell runners. Windows keeps its own process helper.
describe.skipIf(process.platform === 'win32')('Installed E2E core pins', () => {
  it('reads supplied pins without a default Cargo toolchain', () => {
    const { core, home, env } = fixture()
    env.HOME = home
    const missing = run(['--input-type=module', '-e', readPins], env)
    expect(missing.status).not.toBe(0)
    expect(missing.stderr).toContain('rustup has no default toolchain')
    fs.unlinkSync(env.CARGO_CALLS)
    const supplied = run(['--input-type=module', '-e', readPins], { ...env, MUNIMENT_CORE_ROOT: core })
    expect(supplied.status, supplied.stderr).toBe(0)
    expect(JSON.parse(supplied.stdout)).toEqual(expected)
    expect(fs.existsSync(env.CARGO_CALLS)).toBe(false)
  })

  it.each(['missing', 'invalid'])('rejects %s pins without a Cargo fallback', state => {
    const { core, home, env } = fixture()
    const file = path.join(core, 'pins/pins.toml')
    if (state === 'missing') fs.unlinkSync(file)
    else fs.writeFileSync(file, 'not valid TOML\n')
    const result = run(['--input-type=module', '-e', readPins], { ...env, HOME: home, MUNIMENT_CORE_ROOT: core })
    expect(result.status).not.toBe(0)
    expect(result.stderr).toContain(state === 'missing' ? 'ENOENT' : 'Unsupported pins.toml line')
    expect(fs.existsSync(env.CARGO_CALLS)).toBe(false)
  })

  describe.each(['macos-wdio.sh', 'linux.sh'])('%s', runner => {
    const source = fs.readFileSync(path.join(repository, 'test/e2e/runner', runner), 'utf8')
    const start = source.indexOf('MUNIMENT_CORE_ROOT=$(')
    const end = source.indexOf('\nexport MUNIMENT_CORE_ROOT', start) + '\nexport MUNIMENT_CORE_ROOT'.length
    const resolution = source.slice(start, end)

    it.each(['', 'failure', 'empty'])('exports the build checkout or stops on Cargo result %s', cargoResult => {
      expect(start).toBeGreaterThan(-1)
      const isolation = runner === 'macos-wdio.sh' ? 'export HOME="$state_root/degraded"' : 'export XDG_DATA_HOME="$state_root/ready/data"'
      expect(end).toBeLessThan(source.indexOf(isolation))
      const { env } = fixture()
      const result = spawnSync('bash', ['-c', `
set -uo pipefail
status=0 first_failed_step=none
trap 'printf "first_failed_step=%s\\n" "$first_failed_step" >&2; exit "$status"' EXIT
run_step() { shift; "$@"; }
run_setup() { "$@"; }
${resolution}
export HOME="$SPEC_HOME"
node --input-type=module -e "$READ_PINS" || status=1
`], { cwd: repository, env: { ...env, CARGO_FIXTURE: cargoResult, READ_PINS: readPins }, encoding: 'utf8', timeout: 10000 })
      expect(result.status, result.stderr).toBe(cargoResult ? 1 : 0)
      if (cargoResult) {
        expect(result.stdout).toBe('')
        expect(result.stderr).toContain(cargoResult === 'empty' ? 'Cargo resolved no muniment-pins package' : 'rustup has no default toolchain')
        if (runner === 'macos-wdio.sh') expect(result.stderr).toContain('first_failed_step=resolve-core-root')
      } else expect(JSON.parse(result.stdout)).toEqual(expected)
      const calls = fs.readFileSync(env.CARGO_CALLS, 'utf8').trim().split('\n').map(line => JSON.parse(line))
      expect(calls).toEqual([['metadata', '--manifest-path', path.join(repository, 'src-tauri/Cargo.toml'), '--locked', '--format-version', '1']])
    })
  })
})

it('exports the core root before Windows isolates the profile', () => {
  const runner = fs.readFileSync('test/e2e/runner/windows.ps1', 'utf8')
  const resolution = '$env:MUNIMENT_CORE_ROOT = (Invoke-NativeCommand "node" "scripts/muniment-core.mjs" $installerLog "Core pins resolution failed.").Trim()'
  expect(runner).toContain(resolution)
  expect(runner.indexOf(resolution)).toBeLessThan(runner.indexOf('$env:APPDATA = Join-Path $stateRoot "Degraded\\Roaming"'))
})
