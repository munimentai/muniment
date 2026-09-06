import { afterEach, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { lastAuthLogs, withAuthDiagnostics } from './e2e/support/auth-diagnostics.mjs'

const directories = []
async function fixture(files = {}) {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'muniment-auth-log-'))
  directories.push(directory)
  await Promise.all(Object.entries(files).map(([name, text]) => writeFile(path.join(directory, name), text)))
  return directory
}
afterEach(async () => {
  await Promise.all(directories.splice(0).map((directory) => rm(directory, { recursive: true, force: true })))
})

const start = 'muniment-desktop: native-auth start method=RPC path=session.sign_in'
const cloud = 'muniment-runtime: native-auth end method=POST path=/v1/auth/native/authorize status=401 elapsed_ms=19'
const timeout = 'muniment-desktop: native-auth end method=RPC path=session.sign_in error=Timeout elapsed_ms=120000'

describe('the auth failure diagnostics', () => {
  it('quotes runtime and forwarded desktop lines in the failure message', async () => {
    const directory = await fixture({
      'muniment-runtime.log': `${cloud}\n`,
      'wdio-0-0.log': `[Tauri:Backend] ${start}\r\n[INFO] ${timeout}\r\n`,
    })
    const context = { marker: true }
    const wrapped = withAuthDiagnostics(async function () {
      expect(this).toBe(context)
      throw new Error('The continuation did not open.')
    }, directory)
    await expect(wrapped.call(context)).rejects.toThrow(`The continuation did not open.\n"${cloud}"\n"${start}"\n"${timeout}"`)
  })

  it('reports missing and empty logs without replacing the failure', async () => {
    const directory = await fixture()
    expect(await lastAuthLogs(directory)).toBe('The auth logs contain no native-auth lines.')
    expect(await lastAuthLogs(path.join(directory, 'missing'))).toBe('The auth logs are unavailable.')
    await expect(withAuthDiagnostics(async () => { throw new Error('Original failure.') }, undefined)())
      .rejects.toThrow('Original failure.\nThe auth logs are unavailable.')
    await expect(withAuthDiagnostics(async () => { throw null }, directory)())
      .rejects.toThrow('The sign-in spec failed.\nThe auth logs contain no native-auth lines.')
    await expect(withAuthDiagnostics(async () => 42, directory)()).resolves.toBe(42)
  })

  it('quotes the fixed disconnected-runtime cause without nearby user data', async () => {
    const line = 'desktop native-auth response: runtime unavailable'
    const directory = await fixture({ 'driver-app.log': `private-user: ${line}\n${line} token=secret\n` })
    expect(await lastAuthLogs(directory)).toBe(`"${line}"`)
  })

  it('ignores secrets, unfinished lines, and non-log files', async () => {
    const directory = await fixture({
      'driver-app.log': `token=known-secret\n${cloud} cookie=known-secret\n${cloud}\n${timeout}`,
      'auth-url': 'https://example.com/known-secret',
    })
    expect(await lastAuthLogs(directory)).toBe(`"${cloud}"`)
  })

  it('quotes only the last six complete auth lines from each bounded log tail', async () => {
    const lines = Array.from({ length: 12 }, (_, index) => cloud.replace('elapsed_ms=19', `elapsed_ms=${index}`))
    const directory = await fixture({ 'muniment-runtime.log': `${'x'.repeat(70000)}\n${lines.join('\n')}\n` })
    expect(await lastAuthLogs(directory)).toBe(lines.slice(-6).map((line) => `"${line}"`).join('\n'))
  })

  it('preserves the auth lines and both log files through envelope redaction', async () => {
    const source = await fixture({ 'driver-app.log': `${start}\n${timeout}\n`, 'muniment-runtime.log': `${cloud}\n` })
    const destination = await fixture()
    const result = spawnSync(process.execPath, ['test/e2e/support/redact.mjs', source, destination], { encoding: 'utf8' })
    expect(result.status, result.stderr).toBe(0)
    expect(await readFile(path.join(destination, 'driver-app.log'), 'utf8')).toBe(`${start}\n${timeout}\n`)
    expect(await readFile(path.join(destination, 'muniment-runtime.log'), 'utf8')).toBe(`${cloud}\n`)
  })

  it('wraps the real sign-in spec before any wait can fail', async () => {
    const spec = await readFile('test/e2e/specs/real-sign-in.spec.js', 'utf8')
    expect(spec).toContain("it('signs in through the production UI', withAuthDiagnostics(async function () {")
    expect(spec).toContain('}, rawDir)).timeout(900000)')
  })

  it.each(['linux.sh', 'windows.ps1', 'macos.sh'])('%s collects driver-app.log through the raw artifact directory', async (name) => {
    const runner = await readFile(`test/e2e/runner/${name}`, 'utf8')
    expect(runner).toContain('driver-app.log')
    expect(runner).toContain('redact-artifacts')
  })
})
