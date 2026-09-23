import { it, expect } from 'vitest'
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync, existsSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'

const script = resolve('scripts/stage-cef-linux.mjs')
it.skipIf(process.platform === 'win32')('checks the installed sandbox helper trust rules with the native selector', () => {
  const directory = mkdtempSync(join(tmpdir(), 'cef-sandbox-test-'))
  try {
    const executable = join(directory, 'sandbox-tests')
    const compile = spawnSync('rustc', ['--edition', '2021', '--test', resolve('src-tauri/src/cef_linux_sandbox.rs'), '-o', executable], { encoding: 'utf8', timeout: 60000 })
    expect(compile.status, compile.stderr).toBe(0)
    const result = spawnSync(executable, [], { encoding: 'utf8', timeout: 10000 })
    expect(result.status, result.stdout + result.stderr).toBe(0)
  } finally {
    rmSync(directory, { recursive: true, force: true })
  }
})

it('stages CEF resources and credits and rejects an incomplete runtime', () => {
  const directory = mkdtempSync(join(tmpdir(), 'cef-package-'))
  try {
    const target = join(directory, 'src-tauri/target/release')
    const source = join(target, 'build/cef-dll-sys-fixture/out/cef_linux_x86_64')
    mkdirSync(source, { recursive: true })
    mkdirSync(join(target, 'locales'))
    writeFileSync(join(source, 'CREDITS.html'), 'credits')
    writeFileSync(join(target, 'locales/en-US.pak'), 'locale')
    const payload = ['libcef.so', 'icudtl.dat', 'chrome-sandbox', 'muniment-cef-helper', 'resources.pak']
    for (const name of payload) writeFileSync(join(target, name), name)
    const run = () => spawnSync(process.execPath, [script], { cwd: directory, encoding: 'utf8' })
    const result = run()
    expect(result.status, result.stderr).toBe(0)
    for (const name of payload) expect(readFileSync(join(target, 'cef-resources', name), 'utf8')).toBe(name)
    expect(readFileSync(join(target, 'cef-resources/locales/en-US.pak'), 'utf8')).toBe('locale')
    expect(readFileSync(join(target, 'cef-resources/CEF-CREDITS.html'), 'utf8')).toBe('credits')
    rmSync(join(target, 'libcef.so'))
    const missing = run()
    expect(missing.status).not.toBe(0)
    expect(missing.stderr).toContain('Missing CEF runtime file: libcef.so')
    expect(existsSync(join(target, 'cef-resources/libcef.so'))).toBe(false)
  } finally {
    rmSync(directory, { recursive: true, force: true })
  }
})
