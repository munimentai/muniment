import { afterEach, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

const root = process.cwd()
const temporary = []
const temp = () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-failure-summary-'))
  temporary.push(directory)
  return directory
}
afterEach(() => {
  for (const directory of temporary.splice(0)) fs.rmSync(directory, { recursive: true, force: true })
})
const support = (name) => path.join(root, 'test/e2e/support', name)
const bash = process.platform === 'win32' ? path.join(process.env.ProgramFiles, 'Git/bin/bash.exe') : 'bash'
const shellPath = (file) => file.replaceAll('\\', '/')

function publish(helper, stdout, stderr, env) {
  const directory = temp()
  const raw = path.join(directory, 'raw')
  const artifacts = path.join(directory, 'artifacts')
  fs.mkdirSync(raw)
  if (helper === 'Windows runner') {
    const script = path.join(directory, 'failure.mjs')
    fs.writeFileSync(script, `process.stdout.write(${JSON.stringify(stdout)}); process.stderr.write(${JSON.stringify(stderr)}); process.exitCode = 7`)
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', path.join(root, 'test/e2e/runner/windows.ps1')], {
      encoding: 'utf8', env: { ...env, TEMP: directory, TMP: directory, DCI_ARTIFACTS_DIR: artifacts,
        MUNIMENT_E2E_NATIVE_COMMAND_TEST_SCRIPT: script },
    })
    expect(result.status, result.stderr).toBe(1)
  } else {
    fs.writeFileSync(path.join(raw, 'stdout.log'), stdout)
    fs.writeFileSync(path.join(raw, 'stderr.log'), stderr)
    if (helper === 'POSIX runner') {
      const result = spawnSync(bash, ['-c', `
        source "$1"
        raw=$2
        status=0
        run_setup bash -c 'cat "$1/stdout.log"; cat "$1/stderr.log" >&2; exit 7' bash "$raw"
      `, 'bash', support('runner-failure.sh'), raw], { encoding: 'utf8', env })
      expect(result.status, result.stderr).toBe(7)
      expect(result.stdout).toBe(stdout)
      expect(fs.readFileSync(path.join(raw, 'setup-stderr.log'), 'utf8')).toBe(stderr)
    } else {
      // Exercise the Windows JSON input contract on every host.
      const result = spawnSync(process.execPath, [support('failure-summary.mjs')], {
        input: JSON.stringify({ stdout, stderr, label: 'native command test failed', exitCode: 7 }), encoding: 'utf8', env,
      })
      expect(result.status, result.stderr).toBe(0)
      fs.writeFileSync(path.join(raw, 'runner-failure.txt'), `message: ${result.stdout}\ncategory: OperationStopped\nline: 100`)
    }
    const redaction = spawnSync(process.execPath, [support('redact.mjs'), raw, artifacts], { encoding: 'utf8', env })
    expect(redaction.status, redaction.stderr).toBe(0)
  }
  const junit = spawnSync(bash, [shellPath(support('ensure-junit-report.sh')), shellPath(artifacts), 'installed-test', '1', '0'], { encoding: 'utf8' })
  expect(junit.status, junit.stderr).toBe(0)
  return Object.fromEntries(fs.readdirSync(artifacts).map((name) => [name, fs.readFileSync(path.join(artifacts, name), 'utf8')]))
}

for (const helper of ['POSIX runner', 'Windows summary', 'Windows runner']) {
  describe.skipIf(helper === 'Windows runner' ? process.platform !== 'win32' : helper === 'POSIX runner' && process.platform === 'win32')(helper, { timeout: 30_000 }, () => {
    it.each([['stdout', false], ['stderr', false], ['stdout', true], ['stderr', true]])(
      'redacts secrets across both %s cutoffs with two streams=%s', (stream, both) => {
        const password = 'injected-password-boundary'
        const token = 'ghp_abcdefghijklmnopqrstuvwxyz0123456789'
        const head = both ? 200 : 425
        const tail = head - 5
        const text = 'p'.repeat(head - 14) + password + 'q'.repeat(1800) + ' ' + token + ' ' + 'r'.repeat(tail - 15)
        const other = both ? 'unrelated warning' : ''
        const files = publish(helper, stream === 'stdout' ? text : other, stream === 'stderr' ? text : other,
          { ...process.env, MUNIMENT_E2E_PASSWORD: password })
        for (const content of Object.values(files)) {
          expect(content).not.toContain('injected-passwo')
          expect(content).not.toContain('wxyz0123456789')
          expect(content).not.toContain(password)
          expect(content).not.toContain(token)
        }
        const log = files[helper === 'Windows runner' ? 'installer.log' : `${stream}.log`]
        expect(log).toContain('q'.repeat(1800))
        expect(log).toContain('[REDACTED]')
        expect(log).toContain('[REDACTED:github-token]')
        expect(files['runner-failure.txt']).toContain('exit code 7')
        expect(files['junit-infrastructure.xml']).toContain('exit code 7')
      },
    )

    it.each(['macos', 'windows'])('keeps the complete missing %s asset error through the JUnit cap', (platform) => {
      const sha = 'a'.repeat(40)
      const assetNames = [
        `nightly-${sha}-linux-muniment.deb`,
        `nightly-${sha}-${platform === 'macos' ? 'windows-muniment_0.0.1_x64_en-US.msi' : 'macos-muniment.app.zip'}`,
      ]
      const result = spawnSync(process.execPath, [support('asset-identity.mjs'), sha, platform], {
        input: JSON.stringify({ assets: assetNames.map((name, index) => ({ name, id: index + 1 })) }), encoding: 'utf8',
      })
      expect(result.status).toBe(1)
      const error = result.stderr.split('\n').find((line) => line.startsWith('Error: missing or duplicate'))
      expect(error).toContain('matches=0')
      const files = publish(helper, 'p'.repeat(1800), result.stderr, process.env)
      const escaped = error.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;').replaceAll("'", '&apos;')
      expect(files['runner-failure.txt']).toContain(error)
      expect(files['runner-failure.txt'].length).toBeLessThanOrEqual(1000)
      expect(files['junit-infrastructure.xml']).toContain(escaped)
      for (const name of assetNames) expect(files['junit-infrastructure.xml']).toContain(name)
      const log = files[helper === 'Windows runner' ? 'installer.log' : 'stderr.log']
      expect(log).toContain(result.stderr)
      expect(log).toContain('throw new Error')
      expect(log).toContain('    at ')
    })
  })
}
